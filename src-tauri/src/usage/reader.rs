//! Filesystem walk + streaming transcript read.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::transcripts::{
    might_carry_usage, parse_claude_line, parse_codex_line, CodexScanState, UsageProvider,
    UsageRecord,
};

#[derive(Debug, Clone)]
pub struct TranscriptFile {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ms: i64,
}

pub fn list_transcript_files(root: &Path, since_ms: i64) -> Vec<TranscriptFile> {
    let mut found = Vec::new();
    walk(root, since_ms, &mut found);
    found
}

fn walk(dir: &Path, since_ms: i64, found: &mut Vec<TranscriptFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(&path, since_ms, found);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if mtime_ms >= since_ms {
            found.push(TranscriptFile {
                path,
                size: meta.len(),
                mtime_ms,
            });
        }
    }
}

/// Streams one transcript. `None` = read failure (do not cache as empty).
pub fn read_transcript_records(
    file_path: &Path,
    provider: UsageProvider,
) -> Option<Vec<UsageRecord>> {
    let file = File::open(file_path).ok()?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    let mut codex_state = CodexScanState::new();

    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        match provider {
            UsageProvider::Codex => {
                if !might_carry_usage(&line, provider)
                    && !line.contains("\"turn_context\"")
                    && !line.contains("\"session_meta\"")
                {
                    continue;
                }
                if let Some(record) = parse_codex_line(&line, &mut codex_state) {
                    records.push(record);
                }
            }
            UsageProvider::Claude => {
                if !might_carry_usage(&line, provider) {
                    continue;
                }
                if let Some(record) = parse_claude_line(&line) {
                    records.push(record);
                }
            }
        }
    }
    Some(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::scratch_dir;

    #[test]
    fn list_finds_jsonl_by_mtime() {
        let root = scratch_dir("usage-list");
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        let path = nested.join("session.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        let files = list_transcript_files(&root, 0);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_claude_file() {
        let root = scratch_dir("usage-read");
        let path = root.join("t.jsonl");
        let line = serde_json::json!({
            "type": "assistant",
            "timestamp": "2026-08-07T04:05:13.944Z",
            "sessionId": "s1",
            "message": {
                "id": "msg_1",
                "model": "claude-fable-5",
                "usage": {
                    "input_tokens": 2,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": 0,
                    "output_tokens": 10
                }
            }
        })
        .to_string();
        std::fs::write(&path, format!("{line}\n")).unwrap();
        let records = read_transcript_records(&path, UsageProvider::Claude).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].totals.output_tokens, 10);
        let _ = std::fs::remove_dir_all(&root);
    }
}
