//! Aggregated UsageSummaryDto cache keyed by window + transcript-source signature.
//! Avoids reloading/re-aggregating ~100k+ per-file records on every open.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::dto::{UsageSummaryDto, UsageSummaryInput};

use super::cache_io::atomic_write;

pub const USAGE_SUMMARY_CACHE_VERSION: u32 = 3;
const MAX_ENTRIES: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryCacheFile {
    version: u32,
    entries: Vec<SummaryCacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryCacheEntry {
    key: String,
    source_signature: String,
    dto: UsageSummaryDto,
}

pub fn summary_cache_path_for(home: &Path) -> PathBuf {
    home.join(".on-n-off").join("usage-summary-cache.json")
}

/// The cache key for one window priced with one rate table: a re-fetched table (a newly listed
/// model, a price change) must not serve yesterday's costs.
pub fn summary_key(input: &UsageSummaryInput, rates_fetched_at_ms: Option<i64>) -> String {
    format!(
        "{}|rates:{}",
        window_key(input),
        rates_fetched_at_ms.unwrap_or(0)
    )
}

pub fn window_key(input: &UsageSummaryInput) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        input.resolution.as_deref().unwrap_or("day"),
        input.since_day,
        input.until_day,
        input.time_zone,
        input.since_time.as_deref().unwrap_or(""),
        input.until_time.as_deref().unwrap_or(""),
    )
}

pub fn load_summary_hit(path: &Path, key: &str, source_signature: &str) -> Option<UsageSummaryDto> {
    let raw = std::fs::read_to_string(path).ok()?;
    let file: SummaryCacheFile = serde_json::from_str(&raw).ok()?;
    if file.version != USAGE_SUMMARY_CACHE_VERSION {
        return None;
    }
    let entry = file.entries.into_iter().find(|entry| entry.key == key)?;
    if entry.source_signature != source_signature {
        return None;
    }
    let mut dto = entry.dto;
    dto.cache_hit = true;
    dto.scan_duration_ms = 0;
    Some(dto)
}

pub fn store_summary(path: &Path, key: &str, source_signature: &str, dto: &UsageSummaryDto) {
    let mut entries = load_entries(path);
    entries.retain(|entry| entry.key != key);
    let mut stored = dto.clone();
    stored.cache_hit = false;
    entries.insert(
        0,
        SummaryCacheEntry {
            key: key.to_string(),
            source_signature: source_signature.to_string(),
            dto: stored,
        },
    );
    entries.truncate(MAX_ENTRIES);
    let file = SummaryCacheFile {
        version: USAGE_SUMMARY_CACHE_VERSION,
        entries,
    };
    if let Ok(raw) = serde_json::to_string(&file) {
        let _ = atomic_write(path, &raw);
    }
}

fn load_entries(path: &Path) -> Vec<SummaryCacheEntry> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<SummaryCacheFile>(&raw) else {
        return Vec::new();
    };
    if file.version != USAGE_SUMMARY_CACHE_VERSION {
        return Vec::new();
    }
    file.entries
}

#[cfg(test)]
mod tests;
