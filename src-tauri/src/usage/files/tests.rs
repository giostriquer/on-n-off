use std::time::SystemTime;

use serde_json::{json, Value};

#[cfg(unix)]
use super::history::Watermark;
use super::sources::{
    reset_transcript_parse_count, source_index_file, transcript_parse_count, with_live_transcript,
};
use super::*;
use crate::paths::scratch_dir;
use crate::usage::transcripts::USAGE_TRANSCRIPT_PARSER_VERSION;

fn at(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

fn write_transcript(path: &Path, lines: &[Value], written: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(path, body).unwrap();
    let modified = SystemTime::from(chrono::DateTime::parse_from_rfc3339(written).unwrap());
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

fn claude_line(message_id: &str, session: &str, timestamp: &str, usage: Value) -> Value {
    json!({
        "type": "assistant",
        "timestamp": timestamp,
        "sessionId": session,
        "requestId": "req_1",
        "message": { "id": message_id, "model": "claude-fable-5", "usage": usage }
    })
}

fn fold_on(home: &Path, iso: &str) {
    UsageFiles::of(home)
        .fold(at(iso), &mut FoldChecks::default())
        .unwrap();
}

fn history_file(home: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(history_path_for(home)).unwrap()).unwrap()
}

fn write_first_week(home: &Path) {
    write_transcript(
        &home.join(".claude/projects/proj/a.jsonl"),
        &[
            claude_line(
                "msg_1",
                "sess-a",
                "2026-08-07T04:05:00.000Z",
                json!({ "input_tokens": 10, "output_tokens": 20 }),
            ),
            claude_line(
                "msg_2",
                "sess-b",
                "2026-08-07T04:10:00.000Z",
                json!({ "input_tokens": 30, "output_tokens": 40 }),
            ),
            json!({
                "type": "assistant",
                "timestamp": "2026-08-07T04:12:00.000Z",
                "sessionId": "sess-a",
                "costUSD": 0.5,
                "message": {
                    "id": "msg_3",
                    "model": "claude-fable-5",
                    "usage": { "input_tokens": 1, "output_tokens": 2 }
                }
            }),
            claude_line(
                "msg_4",
                "sess-a",
                "2026-08-07T04:30:00.000Z",
                json!({
                    "input_tokens": 150000,
                    "cache_read_input_tokens": 60000,
                    "output_tokens": 5
                }),
            ),
            claude_line(
                "msg_5",
                "sess-a",
                "2026-08-15T00:00:00.000Z",
                json!({ "input_tokens": 7, "output_tokens": 8 }),
            ),
        ],
        "2026-08-15T00:00:01Z",
    );
    write_transcript(
        &home.join(".codex/sessions/2026/08/09/rollout.jsonl"),
        &[
            json!({
                "type": "session_meta",
                "timestamp": "2026-08-09T09:58:00.000Z",
                "payload": { "id": "session-c" }
            }),
            json!({
                "type": "turn_context",
                "timestamp": "2026-08-09T09:58:01.000Z",
                "payload": { "model": "gpt-5.6-sol" }
            }),
            json!({
                "type": "event_msg",
                "timestamp": "2026-08-09T09:59:00.000Z",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "last_token_usage": {
                            "input_tokens": 100,
                            "cached_input_tokens": 40,
                            "output_tokens": 10,
                            "reasoning_output_tokens": 3
                        }
                    }
                }
            }),
        ],
        "2026-08-09T10:00:01Z",
    );
}

#[test]
fn a_fold_writes_the_rows_and_the_watermark_of_what_aged_past_the_cutoff() {
    let home = scratch_dir("usage-folding-rows");
    write_first_week(&home);

    fold_on(&home, "2026-08-21T12:00:00Z");

    let written = history_file(&home);
    let cutoff = at("2026-08-14T00:00:00Z");
    assert_eq!(
        written,
        json!({
            "version": 1,
            "foldedThroughMs": cutoff,
            "segments": [{
                "fromMs": null,
                "toMs": cutoff,
                "parserVersion": USAGE_TRANSCRIPT_PARSER_VERSION,
                "foldedAtMs": at("2026-08-21T12:00:00Z")
            }],
            "models": ["claude-fable-5", "gpt-5.6-sol"],
            "sessions": ["sess-a", "sess-b", "session-c"],
            "rows": [
                [at("2026-08-07T04:00:00Z"), "claude", 0, 0, 40, 0, 0, 0, 60, 0, 2, 0.0, [0, 1]],
                [at("2026-08-07T04:00:00Z"), "claude", 0, 1, 1, 0, 0, 0, 2, 0, 1, 0.5, [0]],
                [at("2026-08-07T04:30:00Z"), "claude", 0, 2, 150000, 60000, 0, 0, 5, 0, 1, 0.0, [0]],
                [at("2026-08-09T09:45:00Z"), "codex", 1, 0, 60, 40, 0, 0, 10, 3, 1, 0.0, [2]]
            ]
        })
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_second_fold_appends_the_rows_between_the_cutoffs_and_moves_the_watermark() {
    let home = scratch_dir("usage-folding-second");
    write_first_week(&home);
    fold_on(&home, "2026-08-21T12:00:00Z");
    write_transcript(
        &home.join(".claude/projects/proj/b.jsonl"),
        &[
            claude_line(
                "msg_6",
                "sess-d",
                "2026-08-20T10:05:00.000Z",
                json!({ "input_tokens": 3, "output_tokens": 4 }),
            ),
            claude_line(
                "msg_7",
                "sess-d",
                "2026-08-25T00:00:00.000Z",
                json!({ "input_tokens": 5, "output_tokens": 6 }),
            ),
        ],
        "2026-08-25T00:00:01Z",
    );

    fold_on(&home, "2026-08-30T12:00:00Z");

    let cutoff = at("2026-08-23T00:00:00Z");
    assert_eq!(
        history_file(&home),
        json!({
            "version": 1,
            "foldedThroughMs": cutoff,
            "segments": [{
                "fromMs": null,
                "toMs": cutoff,
                "parserVersion": USAGE_TRANSCRIPT_PARSER_VERSION,
                "foldedAtMs": at("2026-08-30T12:00:00Z")
            }],
            "models": ["claude-fable-5", "gpt-5.6-sol"],
            "sessions": ["sess-a", "sess-b", "session-c", "sess-d"],
            "rows": [
                [at("2026-08-07T04:00:00Z"), "claude", 0, 0, 40, 0, 0, 0, 60, 0, 2, 0.0, [0, 1]],
                [at("2026-08-07T04:00:00Z"), "claude", 0, 1, 1, 0, 0, 0, 2, 0, 1, 0.5, [0]],
                [at("2026-08-07T04:30:00Z"), "claude", 0, 2, 150000, 60000, 0, 0, 5, 0, 1, 0.0, [0]],
                [at("2026-08-09T09:45:00Z"), "codex", 1, 0, 60, 40, 0, 0, 10, 3, 1, 0.0, [2]],
                [at("2026-08-15T00:00:00Z"), "claude", 0, 0, 7, 0, 0, 0, 8, 0, 1, 0.0, [0]],
                [at("2026-08-20T10:00:00Z"), "claude", 0, 0, 3, 0, 0, 0, 4, 0, 1, 0.0, [3]]
            ]
        })
    );
    let _ = std::fs::remove_dir_all(&home);
}

const AFTER_AUGUST_FIRST_WEEK: &str = "2026-08-21T12:00:00Z";

fn fold(home: &Path) {
    fold_with(home, &mut FoldChecks::default());
}

fn fold_with(home: &Path, checks: &mut FoldChecks) {
    UsageFiles::of(home)
        .fold(at(AFTER_AUGUST_FIRST_WEEK), checks)
        .unwrap();
}

fn folded_outputs(home: &Path) -> Vec<u64> {
    let store = HistoryStore::open(history_path_for(home));
    let history = store.history().expect("a readable history");
    history
        .rows()
        .iter()
        .map(|row| row.totals.output_tokens)
        .collect()
}

#[cfg(unix)]
fn watermark(home: &Path) -> Watermark {
    HistoryStore::open(history_path_for(home)).watermark()
}

fn write_record(home: &Path, name: &str, timestamp: &str, output_tokens: u64) -> PathBuf {
    let path = home.join(".claude/projects/proj").join(name);
    let usage = json!({ "input_tokens": 1, "output_tokens": output_tokens });
    write_transcript(
        &path,
        &[claude_line(name, "sess-claude", timestamp, usage)],
        timestamp,
    );
    path
}

#[cfg(unix)]
fn set_mtime(path: &Path, iso: &str) {
    let modified = SystemTime::from(chrono::DateTime::parse_from_rfc3339(iso).unwrap());
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

#[test]
fn a_transcript_still_being_written_does_not_hold_the_fold_back() {
    let home = scratch_dir("usage-history-live-writer");
    let path = write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let still_writing = claude_line(
        "msg_2",
        "sess-claude",
        "2026-08-21T11:00:00.000Z",
        json!({ "input_tokens": 1, "output_tokens": 1 }),
    )
    .to_string();

    with_live_transcript(&path, &still_writing, || fold(&home));

    assert_eq!(folded_outputs(&home), [20]);
    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn folding_waits_until_every_directory_can_be_walked() {
    use std::os::unix::fs::PermissionsExt;
    let home = scratch_dir("usage-history-locked-dir");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = home.join(".claude/projects/locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    fold(&home);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(watermark(&home), Watermark::NONE);
    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn folding_waits_for_a_recent_transcript_that_cannot_be_read() {
    use std::os::unix::fs::PermissionsExt;
    let home = scratch_dir("usage-history-locked-file");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = write_record(&home, "b.jsonl", "2026-08-08T04:05:13.944Z", 300);
    set_mtime(&locked, "2026-08-08T04:05:14Z");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let mut checks = FoldChecks::default();

    fold_with(&home, &mut checks);
    fold_with(&home, &mut checks);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(watermark(&home), Watermark::NONE);
    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn a_transcript_unreadable_for_a_week_past_the_cutoff_stops_holding_the_fold_back() {
    use std::os::unix::fs::PermissionsExt;
    let home = scratch_dir("usage-history-long-unreadable");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = write_record(&home, "b.jsonl", "2026-08-06T04:05:13.944Z", 300);
    set_mtime(&locked, "2026-08-06T23:59:59Z");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let mut checks = FoldChecks::default();

    fold_with(&home, &mut checks);
    let after_one_check = watermark(&home);
    fold_with(&home, &mut checks);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(after_one_check, Watermark::NONE);
    assert_eq!(watermark(&home).ms(), Some(at("2026-08-14T00:00:00Z")));
    assert_eq!(folded_outputs(&home), [20]);
    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn an_old_transcript_that_fails_to_read_once_is_waited_for() {
    use std::os::unix::fs::PermissionsExt;
    let home = scratch_dir("usage-history-transient-unreadable");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = write_record(&home, "b.jsonl", "2026-08-06T04:05:13.944Z", 300);
    set_mtime(&locked, "2026-08-06T23:59:59Z");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let mut checks = FoldChecks::default();

    fold_with(&home, &mut checks);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    fold_with(&home, &mut checks);

    assert_eq!(watermark(&home).ms(), Some(at("2026-08-14T00:00:00Z")));
    let mut outputs = folded_outputs(&home);
    outputs.sort_unstable();
    assert_eq!(outputs, [20, 300]);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn history_status_says_how_far_back_usage_is_kept() {
    let home = scratch_dir("usage-history-status");
    let empty = UsageFiles::of(&home).status();
    assert_eq!(empty.state, UsageHistoryState::Empty);
    assert_eq!(
        (empty.kept_since, empty.folded_through, empty.bytes),
        (None, None, 0)
    );

    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    fold(&home);
    let kept = UsageFiles::of(&home).status();

    assert_eq!(kept.state, UsageHistoryState::Kept);
    assert_eq!(kept.kept_since.as_deref(), Some("2026-08-07T04:00:00.000Z"));
    assert_eq!(
        kept.folded_through.as_deref(),
        Some("2026-08-14T00:00:00.000Z")
    );
    assert_eq!(
        kept.bytes,
        std::fs::metadata(history_path_for(&home)).unwrap().len()
    );

    std::fs::write(history_path_for(&home), "{ torn").unwrap();
    assert_eq!(
        UsageFiles::of(&home).status().state,
        UsageHistoryState::Unreadable
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn the_background_check_reads_nothing_else_until_a_fold_is_due() {
    let home = scratch_dir("usage-history-idle-check");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    fold(&home);
    std::fs::remove_file(source_index_file(&home)).unwrap();
    reset_transcript_parse_count();

    fold(&home);

    assert!(!source_index_file(&home).exists());
    assert_eq!(transcript_parse_count(), 0);
    let _ = std::fs::remove_dir_all(&home);
}
