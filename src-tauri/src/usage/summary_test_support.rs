use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::*;

pub(super) fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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

pub(super) fn august_input(force: bool) -> UsageSummaryInput {
    day_input("2026-08-01", "2026-08-31", force)
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

pub(super) fn output_tokens(summary: &UsageSummaryDto) -> u64 {
    summary
        .buckets
        .iter()
        .map(|bucket| bucket.totals.output_tokens)
        .sum()
}
