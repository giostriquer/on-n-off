mod claude;
mod codex;
mod muse;

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::provider_catalogue::UsageProvider;

pub const USAGE_TRANSCRIPT_PARSER_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TokenTotals {
    pub uncached_input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_creation_1h_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
}

impl TokenTotals {
    pub fn total_tokens(&self) -> u64 {
        self.uncached_input_tokens
            + self.cached_input_tokens
            + self.cache_creation_tokens
            + self.output_tokens
    }

    pub fn add(&self, other: &Self) -> Self {
        Self {
            uncached_input_tokens: self.uncached_input_tokens + other.uncached_input_tokens,
            cached_input_tokens: self.cached_input_tokens + other.cached_input_tokens,
            cache_creation_tokens: self.cache_creation_tokens + other.cache_creation_tokens,
            cache_creation_1h_tokens: self.cache_creation_1h_tokens
                + other.cache_creation_1h_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            reasoning_tokens: self.reasoning_tokens + other.reasoning_tokens,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageRecord {
    pub provider: UsageProvider,
    pub timestamp_ms: i64,
    pub model: String,
    pub session_id: String,
    pub totals: TokenTotals,
    pub reported_cost_usd: Option<f64>,
    pub copy_key: Option<String>,
}

impl UsageRecord {
    fn is_richer_than(&self, kept: &Self) -> bool {
        (self.totals.output_tokens, self.totals.total_tokens())
            > (kept.totals.output_tokens, kept.totals.total_tokens())
    }
}

type LineReader = Box<dyn FnMut(&str) -> Option<UsageRecord>>;

type CopyKeys = Box<dyn FnMut(&UsageRecord) -> Option<String>>;

trait Format {
    fn dirs(&self, home: &Path) -> TranscriptDirs;

    fn markers(&self) -> &'static [&'static str];

    fn line_reader(&self) -> LineReader;

    fn copy_keys(&self) -> CopyKeys;
}

fn format_of(provider: UsageProvider) -> &'static dyn Format {
    match provider {
        UsageProvider::Claude => &claude::Claude,
        UsageProvider::Codex => &codex::Codex,
        UsageProvider::Muse => &muse::Muse,
    }
}

pub struct TranscriptDirs {
    reported: PathBuf,
    more: Vec<PathBuf>,
}

impl TranscriptDirs {
    fn only(reported: PathBuf) -> Self {
        Self {
            reported,
            more: Vec::new(),
        }
    }

    pub fn reported(&self) -> &Path {
        &self.reported
    }

    pub fn all(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.reported.as_path()).chain(self.more.iter().map(PathBuf::as_path))
    }
}

pub fn transcript_dirs(provider: UsageProvider, home: &Path) -> TranscriptDirs {
    format_of(provider).dirs(home)
}

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

fn is_transcript_name(name: &str) -> bool {
    name.ends_with(".jsonl") || name.contains(".jsonl.superseded-")
}

pub fn read_transcript(path: &Path, provider: UsageProvider) -> Option<Vec<UsageRecord>> {
    let file = File::open(path).ok()?;
    let records = records_of(BufReader::new(file).lines(), format_of(provider))?;
    Some(
        richest_copies([(provider, records.as_slice())])
            .into_iter()
            .cloned()
            .collect(),
    )
}

fn records_of(
    lines: impl Iterator<Item = io::Result<String>>,
    format: &dyn Format,
) -> Option<Vec<UsageRecord>> {
    let mut record_of = format.line_reader();
    let mut records = Vec::new();
    for line in lines {
        let line = line.ok()?;
        if may_matter(format, &line) {
            records.extend(record_of(&line));
        }
    }
    Some(records)
}

fn may_matter(format: &dyn Format, line: &str) -> bool {
    format.markers().iter().any(|marker| line.contains(marker))
}

pub fn richest_copies<'a>(
    files: impl IntoIterator<Item = (UsageProvider, &'a [UsageRecord])>,
) -> Vec<&'a UsageRecord> {
    let mut kept: Vec<&UsageRecord> = Vec::new();
    let mut slot_of: HashMap<(UsageProvider, String), usize> = HashMap::new();
    for (provider, records) in files {
        let mut key_of = format_of(provider).copy_keys();
        for record in records {
            let Some(key) = key_of(record).map(|key| (provider, key)) else {
                kept.push(record);
                continue;
            };
            if let Some(&slot) = slot_of.get(&key) {
                if record.is_richer_than(kept[slot]) {
                    kept[slot] = record;
                }
            } else {
                slot_of.insert(key, kept.len());
                kept.push(record);
            }
        }
    }
    kept
}

fn record_key(record: &UsageRecord) -> Option<String> {
    record.copy_key.clone()
}

fn positive_int(value: &Value) -> u64 {
    match value {
        Value::Number(n) => n
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.0)
            .map(|v| v.trunc() as u64)
            .unwrap_or(0),
        _ => 0,
    }
}

fn parse_timestamp_ms(value: &Value) -> Option<i64> {
    let raw = value.as_str()?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp_millis())
        .or_else(|| {
            chrono::DateTime::parse_from_rfc3339(&format!("{raw}Z"))
                .ok()
                .map(|dt| dt.timestamp_millis())
        })
}

#[cfg(test)]
mod tests;
