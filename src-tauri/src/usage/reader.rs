//! Filesystem walk + streaming transcript read.

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::transcripts::{
    might_carry_usage, parse_claude_line, parse_codex_line, CodexScanState, UsageProvider,
    UsageRecord,
};

/// A transcript last written more than this before an instant holds no record from that instant
/// on. The allowance covers local days that begin before UTC midnight (up to 14 hours) and record
/// clocks that disagree with the filesystem's.
pub const MTIME_SLACK_MS: i64 = 36 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
pub struct TranscriptFile {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ms: i64,
}

pub struct TranscriptInventory {
    pub files: Vec<TranscriptFile>,
    pub complete: bool,
}

pub fn inventory_transcript_files(root: &Path, since_ms: i64) -> TranscriptInventory {
    let mut found = Vec::new();
    let complete = walk(root, since_ms, &mut found);
    TranscriptInventory {
        files: found,
        complete,
    }
}

fn walk(dir: &Path, since_ms: i64, found: &mut Vec<TranscriptFile>) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut complete = true;
    for entry in entries {
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            complete = false;
            continue;
        };
        if file_type.is_dir() {
            complete &= walk(&path, since_ms, found);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_transcript_name(name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            complete = false;
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
    complete
}

/// A transcript, or one Claude Code set aside as `<session>.jsonl.superseded-<ms>` instead of
/// overwriting it. Turns a rewrite dropped live only in the set-aside copy; the turns both hold
/// share a message id and collapse to one (`transcripts::richest_copies`).
fn is_transcript_name(name: &str) -> bool {
    name.ends_with(".jsonl") || name.contains(".jsonl.superseded-")
}

/// Streams one transcript. `None` = read failure (do not cache as empty).
pub fn read_transcript_records(
    file_path: &Path,
    provider: UsageProvider,
) -> Option<Vec<UsageRecord>> {
    let file = File::open(file_path).ok()?;
    let reader = BufReader::new(file);
    read_transcript_records_from_lines(reader.lines(), provider)
}

fn read_transcript_records_from_lines(
    lines: impl Iterator<Item = io::Result<String>>,
    provider: UsageProvider,
) -> Option<Vec<UsageRecord>> {
    let mut records = Vec::new();
    let mut codex_state = CodexScanState::new();

    for line in lines {
        let line = line.ok()?;
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
mod tests;
