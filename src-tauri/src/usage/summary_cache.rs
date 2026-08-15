//! Aggregated UsageSummaryDto cache keyed by window + transcript-source signature.
//! Avoids reloading/re-aggregating ~100k+ per-file records on every open.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::dto::{UsageSummaryDto, UsageSummaryInput};

use super::cache_io::atomic_write;

pub const USAGE_SUMMARY_CACHE_VERSION: u32 = 2;
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
mod tests {
    use super::*;
    use crate::dto::{UsagePricingDto, UsagePricingStatus, UsageSummaryDto, UsageSummaryInput};
    use crate::paths::scratch_dir;

    fn sample_dto() -> UsageSummaryDto {
        UsageSummaryDto {
            read_at: "2026-08-13T00:00:00Z".into(),
            time_zone: "UTC".into(),
            since_day: "2026-08-01".into(),
            until_day: "2026-08-13".into(),
            buckets: vec![],
            sources: vec![],
            pricing: UsagePricingDto {
                status: UsagePricingStatus::Unavailable,
                source: "none".into(),
                fetched_at: None,
                known_models: 0,
            },
            scan_duration_ms: 42,
            cache_hit: false,
        }
    }

    #[test]
    fn round_trip_hit_requires_matching_source_signature() {
        let home = scratch_dir("usage-summary-cache");
        let path = summary_cache_path_for(&home);
        let input = UsageSummaryInput {
            since_day: "2026-08-01".into(),
            until_day: "2026-08-13".into(),
            time_zone: "UTC".into(),
            resolution: Some("day".into()),
            since_time: None,
            until_time: None,
            force: false,
        };
        let key = window_key(&input);
        let signature = "v1-a";
        store_summary(&path, &key, signature, &sample_dto());
        let hit = load_summary_hit(&path, &key, signature).expect("hit");
        assert!(hit.cache_hit);
        assert_eq!(hit.scan_duration_ms, 0);
        assert!(load_summary_hit(&path, &key, "v1-b").is_none());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn window_key_includes_resolution_and_bounds() {
        let input = UsageSummaryInput {
            since_day: "a".into(),
            until_day: "b".into(),
            time_zone: "UTC".into(),
            resolution: Some("hour".into()),
            since_time: Some("t0".into()),
            until_time: Some("t1".into()),
            force: false,
        };
        assert_eq!(window_key(&input), "hour|a|b|UTC|t0|t1");
    }
}
