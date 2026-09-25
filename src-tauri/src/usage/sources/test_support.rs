//! Transcripts the `sources` tests write into a scratch home.

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

pub(super) fn at(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

/// The UTC midnight that begins `month` of 2026.
pub(super) fn month_start(month: u32) -> i64 {
    at(&format!("2026-{month:02}-01T00:00:00Z"))
}

/// A Claude transcript under the one Claude root.
pub(super) fn transcript_path(home: &Path, name: &str) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join("fixture")
        .join(name)
}

/// One Claude assistant message's usage, as Claude Code writes it.
pub(super) fn record(timestamp: &str, message_id: &str, output_tokens: u64) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": timestamp,
        "sessionId": "sources-session",
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
    })
    .to_string()
}

pub(super) fn write_records(path: &Path, records: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("{}\n", records.join("\n"))).unwrap();
}

pub(super) fn mtime_ms(path: &Path) -> i64 {
    let modified = std::fs::metadata(path).unwrap().modified().unwrap();
    modified.duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

pub(super) fn set_mtime_ms(path: &Path, mtime_ms: i64) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_millis(mtime_ms as u64))
        .unwrap();
}
