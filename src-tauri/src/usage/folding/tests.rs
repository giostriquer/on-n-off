use serde_json::{json, Value};

use super::*;
use crate::paths::scratch_dir;
use crate::usage::transcripts::USAGE_TRANSCRIPT_PARSER_VERSION;

fn at(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

/// `lines` as the transcript at `path`, last written at `written`.
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
    fold_history_in(home, at(iso), &mut FoldChecks::default()).unwrap();
}

fn history_file(home: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(history_path_for(home)).unwrap()).unwrap()
}

/// One Claude transcript and one Codex rollout. On 2026-08-07 between 04:00 and 04:15, two priced
/// messages from two sessions and one with a reported cost; at 04:30 a request over 200k input
/// tokens; on 2026-08-15 one more message. The Codex turn is on 2026-08-09.
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

/// A fold on 2026-08-21 (cutoff 2026-08-14): the two priced messages share a row, the one with a
/// reported cost gets a row of its own, and the long request is flagged. The Codex turn is folded
/// too, and the Claude message on 2026-08-15 is not.
#[test]
fn a_fold_writes_the_rows_and_the_watermark_of_what_aged_past_the_cutoff() {
    let home = scratch_dir("usage-folding-rows");
    write_first_week(&home);

    fold_on(&home, "2026-08-21T12:00:00Z");

    let written = history_file(&home);
    let cutoff = at("2026-08-14T00:00:00Z");
    // [slot, provider, model, flags (1 reported, 2 over 200k input), uncached input, cached input,
    //  cache writes, one-hour cache writes, output, reasoning, records, reported cost, sessions]
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

/// A second fold on 2026-08-30 (cutoff 2026-08-23) appends the rows between the two cutoffs after
/// the ones already kept and moves the watermark on, extending the one segment: the message on
/// 2026-08-15 the first fold left, and a new session's message on 2026-08-20. The one on
/// 2026-08-25 stays in its transcript.
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
