use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::dto::UsageSummaryDto;

use super::Locked;
use crate::usage::cache_io::atomic_write;
use crate::usage::window::CacheIdentity;

pub const USAGE_SUMMARY_CACHE_VERSION: u32 = 4;
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
    group: String,
    source_signature: String,
    dto: UsageSummaryDto,
}

pub fn summary_cache_path_for(home: &Path) -> PathBuf {
    home.join(".on-n-off").join("usage-summary-cache.json")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryKey {
    key: String,
    group: String,
}

pub fn summary_key(
    window: &CacheIdentity,
    rates_fetched_at_ms: Option<i64>,
    history_fingerprint: &str,
) -> SummaryKey {
    SummaryKey {
        key: format!(
            "{}|rates:{}|history:{history_fingerprint}",
            window.key(),
            rates_fetched_at_ms.unwrap_or(0)
        ),
        group: window.group().to_string(),
    }
}

pub fn load_summary_hit(
    path: &Path,
    key: &SummaryKey,
    source_signature: &str,
) -> Option<UsageSummaryDto> {
    let raw = std::fs::read_to_string(path).ok()?;
    let file: SummaryCacheFile = serde_json::from_str(&raw).ok()?;
    if file.version != USAGE_SUMMARY_CACHE_VERSION {
        return None;
    }
    let entry = file
        .entries
        .into_iter()
        .find(|entry| entry.key == key.key)?;
    if entry.source_signature != source_signature {
        return None;
    }
    let mut dto = entry.dto;
    dto.cache_hit = true;
    dto.scan_duration_ms = 0;
    Some(dto)
}

pub fn store_summary(
    _locked: &Locked,
    path: &Path,
    key: &SummaryKey,
    source_signature: &str,
    dto: &UsageSummaryDto,
) {
    let mut entries = load_entries(path);
    entries.retain(|entry| entry.group != key.group);
    let mut stored = dto.clone();
    stored.cache_hit = false;
    entries.insert(
        0,
        SummaryCacheEntry {
            key: key.key.clone(),
            group: key.group.clone(),
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
