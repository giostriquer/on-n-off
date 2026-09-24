use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::*;
use crate::usage::pricing;

/// A summary read under `home` rather than the process's home. Tests read through this instead
/// of setting `ON_N_OFF_HOME`: the variable is process-wide, so every other test running at the
/// time would follow it into a scratch home, and whatever first caches a path derived from it
/// (the CLI search path) keeps that scratch home for the rest of the run. A summary read also
/// changes the pricing module's process-wide state, so every test that reads one holds
/// `pricing::lock_rates_state` first.
pub(super) fn read_summary_in(
    home: &Path,
    input: UsageSummaryInput,
) -> Result<UsageSummaryDto, AdapterError> {
    read_summary_from(input, || Ok(home.to_path_buf()))
}

/// [`read_summary_in`] with the rate table's fetch failing, as it does offline.
pub(super) fn read_offline(home: &Path, input: UsageSummaryInput) -> UsageSummaryDto {
    pricing::with_test_fetch(None, || read_summary_in(home, input)).unwrap()
}

pub(super) fn write_claude_transcript(home: &Path) {
    let dir = home.join(".claude").join("projects").join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    let line = serde_json::json!({
        "type": "assistant",
        "timestamp": "2026-08-07T04:05:13.944Z",
        "sessionId": "sess-claude",
        "message": {
            "id": "msg_1",
            "model": "claude-fable-5",
            "usage": {
                "input_tokens": 10,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
                "output_tokens": 20
            }
        }
    });
    let dup = line.clone();
    std::fs::write(dir.join("session.jsonl"), format!("{line}\n{dup}\n")).unwrap();
}

pub(super) fn append_claude_record(home: &Path, message_id: &str, output_tokens: u64) {
    let path = home
        .join(".claude")
        .join("projects")
        .join("proj")
        .join("session.jsonl");
    let line = serde_json::json!({
        "type": "assistant",
        "timestamp": "2026-08-08T04:05:13.944Z",
        "sessionId": "sess-claude",
        "message": {
            "id": message_id,
            "model": "claude-fable-5",
            "usage": {
                "input_tokens": 1,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
                "output_tokens": output_tokens
            }
        }
    });
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{line}").unwrap();
}

/// The first day `august_input` reads.
const AUGUST_OPENS: &str = "2026-08-01";

pub(super) fn august_input(force: bool) -> UsageSummaryInput {
    day_input(AUGUST_OPENS, "2026-08-31", force)
}

pub(super) fn day_input(since_day: &str, until_day: &str, force: bool) -> UsageSummaryInput {
    UsageSummaryInput {
        since_day: since_day.into(),
        until_day: until_day.into(),
        time_zone: "UTC".into(),
        resolution: Some("day".into()),
        since_time: None,
        until_time: None,
        force,
    }
}

/// An hourly read of `since..until` on 2026-08-07; an empty bound is left out.
pub(super) fn hourly_input(since: &str, until: &str) -> UsageSummaryInput {
    UsageSummaryInput {
        resolution: Some("hour".into()),
        since_time: (!since.is_empty()).then(|| since.into()),
        until_time: (!until.is_empty()).then(|| until.into()),
        ..day_input("2026-08-07", "2026-08-08", false)
    }
}

pub(super) fn write_single_claude_record(
    home: &Path,
    name: &str,
    timestamp: &str,
    output_tokens: u64,
) -> PathBuf {
    let path = home
        .join(".claude")
        .join("projects")
        .join("proj")
        .join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let line = serde_json::json!({
        "type": "assistant",
        "timestamp": timestamp,
        "sessionId": "sess-claude",
        "message": {
            "id": name,
            "model": "claude-fable-5",
            "usage": {
                "input_tokens": 1,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
                "output_tokens": output_tokens
            }
        }
    });
    std::fs::write(&path, format!("{line}\n")).unwrap();
    path
}

/// Backdates `path` to 180 days before `august_input`'s window opens: outside that window and its
/// mtime slack, inside `full_time_input`'s. A fixed instant, because the tests' windows are fixed:
/// "now minus 180 days" enters August's window on runs after 2027-01-26, and the tests using it
/// would go on passing without a file that window skips.
pub(super) fn age_file(path: &Path) {
    let opens = DateTime::parse_from_rfc3339(&format!("{AUGUST_OPENS}T00:00:00Z")).unwrap();
    let modified = SystemTime::from(opens) - Duration::from_secs(180 * 24 * 60 * 60);
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

pub(super) fn full_time_input(force: bool) -> UsageSummaryInput {
    day_input("2020-01-01", "2026-08-31", force)
}

pub(super) fn output_tokens(summary: &UsageSummaryDto) -> u64 {
    summary
        .buckets
        .iter()
        .map(|bucket| bucket.totals.output_tokens)
        .sum()
}

pub(super) fn record_count(summary: &UsageSummaryDto) -> u64 {
    summary.buckets.iter().map(|bucket| bucket.records).sum()
}

/// One Codex rollout with a single turn: its session, its model, and one usage event.
pub(super) fn write_codex_rollout(dir: &Path, name: &str, session: &str, output_tokens: u64) {
    std::fs::create_dir_all(dir).unwrap();
    let lines = [
        serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-08-07T04:00:00.000Z",
            "payload": { "id": session }
        }),
        serde_json::json!({
            "type": "turn_context",
            "timestamp": "2026-08-07T04:00:01.000Z",
            "payload": { "model": "gpt-5.6-sol" }
        }),
        serde_json::json!({
            "type": "event_msg",
            "timestamp": "2026-08-07T04:05:13.944Z",
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {
                        "input_tokens": 100,
                        "cached_input_tokens": 0,
                        "output_tokens": output_tokens
                    }
                }
            }
        }),
    ];
    let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(dir.join(name), body).unwrap();
}

/// Claude lines for `summary_line_claude`: one assistant message's usage, as Claude Code writes it.
pub(super) fn claude_usage_line(
    message_id: &str,
    session: &str,
    timestamp: &str,
    usage: serde_json::Value,
) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": timestamp,
        "sessionId": session,
        "requestId": "req_1",
        "message": { "id": message_id, "model": "claude-fable-5", "usage": usage }
    })
    .to_string()
}

pub(super) fn write_claude_lines(home: &Path, name: &str, lines: &[String]) {
    let dir = home.join(".claude").join("projects").join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), lines.join("\n") + "\n").unwrap();
}

/// How many Claude transcripts a read scanned. A file counts only when its mtime puts it inside
/// the window read, so an aged file is missing from a read of August.
pub(super) fn claude_scanned_files(summary: &UsageSummaryDto) -> u64 {
    summary
        .sources
        .iter()
        .find(|source| source.provider == AgentId::Claude)
        .map_or(0, |source| source.scanned_files)
}
