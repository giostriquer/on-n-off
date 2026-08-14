//! Orchestrates transcript scan + pricing into a UsageSummaryDto.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat, TimeZone, Utc};

use crate::dto::{
    AdapterError, AgentId, UsageBucketDto, UsageCostSource, UsagePricingDto, UsageSourceDto,
    UsageSourceStatus, UsageSummaryDto, UsageSummaryInput, UsageTokenTotalsDto,
};
use crate::paths::{claude_root, codex_root, user_home};

use super::aggregate::{
    AggregateOptions as AggOpts, CostSource, Resolution as AggResolution, UsageAggregator,
    UsageBucket,
};
use super::pricing::{ensure_rates, LITELLM_RATES_URL};
use super::reader::{list_transcript_files, read_transcript_records};
use super::scan_cache::{
    decode_scan_cache, dedupe_within_file, encode_scan_cache, prune_scan_cache, CachedFile,
    PruneOptions, ScanCache,
};
use super::summary_cache::{
    load_summary_hit, scan_cache_identity, store_summary, summary_cache_path_for, window_key,
};
use super::transcripts::{UsageProvider as Provider, UsageRecord};

const MTIME_SLACK_MS: i64 = 36 * 60 * 60 * 1000;
const MAX_HOURLY_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
const CACHE_RETENTION_DAYS: i64 = 90;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn scan_cache_path() -> Result<PathBuf, AdapterError> {
    Ok(user_home()?.join(".on-n-off").join("usage-scan-cache.json"))
}

fn load_scan_cache(path: &Path) -> ScanCache {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ScanCache::new();
    };
    let Ok(doc) = serde_json::from_str(&raw) else {
        return ScanCache::new();
    };
    decode_scan_cache(&doc)
}

fn persist_scan_cache(path: &Path, cache: &ScanCache) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let doc = encode_scan_cache(cache);
    if let Ok(raw) = serde_json::to_string(&doc) {
        let _ = std::fs::write(path, raw);
    }
}

fn resolve_claude_transcript_dir() -> Result<PathBuf, AdapterError> {
    let root = claude_root()?;
    let nested = root.join("projects");
    if nested.is_dir() {
        return Ok(nested);
    }
    let flat = user_home()?.join("projects");
    if flat.is_dir() {
        return Ok(flat);
    }
    Ok(nested)
}

fn resolve_codex_transcript_dir() -> Result<PathBuf, AdapterError> {
    Ok(codex_root()?.join("sessions"))
}

fn parse_iso_ms(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn provider_agent(provider: Provider) -> AgentId {
    match provider {
        Provider::Claude => AgentId::Claude,
        Provider::Codex => AgentId::Codex,
    }
}

fn cost_source_dto(source: CostSource) -> UsageCostSource {
    match source {
        CostSource::ProviderReported => UsageCostSource::ProviderReported,
        CostSource::ModelPriced => UsageCostSource::ModelPriced,
        CostSource::Unpriced => UsageCostSource::Unpriced,
    }
}

fn read_file_records(
    cache: &mut ScanCache,
    cache_dirty: &mut bool,
    file_path: &Path,
    size: u64,
    mtime_ms: i64,
    provider: Provider,
) -> Vec<UsageRecord> {
    let key = file_path.to_string_lossy().to_string();
    if let Some(cached) = cache.get(&key) {
        if cached.size == size && cached.mtime_ms == mtime_ms && cached.provider == provider {
            return cached.records.clone();
        }
    }

    let Some(parsed) = read_transcript_records(file_path, provider) else {
        return Vec::new();
    };
    let records = dedupe_within_file(&parsed);
    cache.insert(
        key,
        CachedFile {
            size,
            mtime_ms,
            provider,
            records: records.clone(),
        },
    );
    *cache_dirty = true;
    records
}

fn bucket_to_dto(bucket: UsageBucket) -> UsageBucketDto {
    UsageBucketDto {
        day: bucket.day,
        hour_start: bucket.hour_start,
        provider: provider_agent(bucket.provider),
        model: bucket.model,
        totals: UsageTokenTotalsDto {
            uncached_input_tokens: bucket.totals.uncached_input_tokens,
            cached_input_tokens: bucket.totals.cached_input_tokens,
            cache_creation_tokens: bucket.totals.cache_creation_tokens,
            output_tokens: bucket.totals.output_tokens,
            reasoning_tokens: bucket.totals.reasoning_tokens,
        },
        cost_usd: bucket.cost_usd,
        cache_savings_usd: bucket.cache_savings_usd,
        cost_source: cost_source_dto(bucket.cost_source),
        records: bucket.records,
        unpriced_records: bucket.unpriced_records,
        sessions: bucket.sessions,
    }
}

fn missing_source(provider: Provider, dir: &Path) -> UsageSourceDto {
    UsageSourceDto {
        provider: provider_agent(provider),
        status: UsageSourceStatus::Missing,
        scanned_files: 0,
        skipped_files: 0,
        malformed_records: 0,
        distinct_sessions: 0,
        message: Some("No transcript directory on this environment.".into()),
        resolved_path: dir.to_string_lossy().to_string(),
    }
}

/// Scan local transcripts and return aggregated usage (priced when rates exist).
pub fn read_summary(input: UsageSummaryInput) -> Result<UsageSummaryDto, AdapterError> {
    if input.since_day > input.until_day {
        return Err(AdapterError::message(format!(
            "sinceDay '{}' is after untilDay '{}'",
            input.since_day, input.until_day
        )));
    }

    let resolution = match input.resolution.as_deref().unwrap_or("day") {
        "hour" => AggResolution::Hour,
        _ => AggResolution::Day,
    };

    let (since_time_ms, until_time_ms) = if resolution == AggResolution::Hour {
        let since = input
            .since_time
            .as_deref()
            .and_then(parse_iso_ms)
            .ok_or_else(|| {
                AdapterError::message(
                    "Hourly usage requires valid sinceTime and untilTime instants",
                )
            })?;
        let until = input
            .until_time
            .as_deref()
            .and_then(parse_iso_ms)
            .ok_or_else(|| {
                AdapterError::message(
                    "Hourly usage requires valid sinceTime and untilTime instants",
                )
            })?;
        let duration = until - since;
        if duration <= 0 || duration > MAX_HOURLY_WINDOW_MS {
            return Err(AdapterError::message(
                "Hourly usage window must be greater than zero and at most 24 hours",
            ));
        }
        (Some(since), Some(until))
    } else {
        (None, None)
    };

    let started = Instant::now();
    let started_ms = now_ms();
    let home = user_home()?;
    let cache_path = scan_cache_path()?;
    let summary_path = summary_cache_path_for(&home);
    let key = window_key(&input);

    if !input.force {
        if let Some(identity) = scan_cache_identity(&cache_path) {
            if let Some(hit) = load_summary_hit(&summary_path, &key, &identity) {
                return Ok(hit);
            }
        }
    }

    let rates = ensure_rates(&home, started_ms);
    let rates_arc = Arc::new(rates.table);
    let mut file_cache = load_scan_cache(&cache_path);
    let mut cache_dirty = false;

    let dirs = [
        (Provider::Claude, resolve_claude_transcript_dir()?),
        (Provider::Codex, resolve_codex_transcript_dir()?),
    ];

    let window_start_ms = since_time_ms.unwrap_or_else(|| {
        DateTime::parse_from_rfc3339(&format!("{}T00:00:00Z", input.since_day))
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0)
    }) - MTIME_SLACK_MS;

    let mut aggregator = UsageAggregator::new(AggOpts {
        time_zone: input.time_zone.clone(),
        since_day: input.since_day.clone(),
        until_day: input.until_day.clone(),
        resolution,
        since_time_ms,
        until_time_ms,
        rates: rates_arc.clone(),
    })
    .map_err(AdapterError::message)?;

    let mut sources = Vec::new();
    let mut live_paths = HashSet::new();
    let mut walked_roots = Vec::new();

    for (provider, dir) in &dirs {
        if !dir.is_dir() {
            sources.push(missing_source(*provider, dir));
            continue;
        }

        walked_roots.push(dir.to_string_lossy().to_string());
        let files = list_transcript_files(dir, window_start_ms);
        let mut scanned_files = 0u64;
        let mut skipped_files = 0u64;
        let mut session_ids = HashSet::new();

        for file in files {
            live_paths.insert(file.path.to_string_lossy().to_string());
            let records = read_file_records(
                &mut file_cache,
                &mut cache_dirty,
                &file.path,
                file.size,
                file.mtime_ms,
                *provider,
            );
            if records.is_empty() {
                skipped_files += 1;
                continue;
            }
            scanned_files += 1;
            for record in &records {
                if aggregator.add(record) && !record.session_id.is_empty() {
                    session_ids.insert(record.session_id.clone());
                }
            }
        }

        sources.push(UsageSourceDto {
            provider: provider_agent(*provider),
            status: UsageSourceStatus::Ok,
            scanned_files,
            skipped_files,
            malformed_records: 0,
            distinct_sessions: session_ids.len() as u64,
            message: None,
            resolved_path: dir.to_string_lossy().to_string(),
        });
    }

    let pruned = prune_scan_cache(
        &mut file_cache,
        PruneOptions {
            live_paths: &live_paths,
            walked_roots: &walked_roots,
            window_start_ms,
            retention_cutoff_ms: started_ms - CACHE_RETENTION_DAYS * 24 * 60 * 60 * 1000,
        },
    );
    if pruned > 0 {
        cache_dirty = true;
    }
    if cache_dirty {
        persist_scan_cache(&cache_path, &file_cache);
    }

    let aggregated = aggregator.finish();
    let read_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let fetched_at = rates.fetched_at_ms.and_then(|ms| {
        Utc.timestamp_millis_opt(ms)
            .single()
            .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, true))
    });

    let dto = UsageSummaryDto {
        read_at,
        time_zone: input.time_zone,
        since_day: input.since_day,
        until_day: input.until_day,
        buckets: aggregated.buckets.into_iter().map(bucket_to_dto).collect(),
        sources,
        pricing: UsagePricingDto {
            status: rates.status.to_dto(),
            source: LITELLM_RATES_URL.into(),
            fetched_at,
            known_models: rates_arc.len() as u64,
        },
        scan_duration_ms: started.elapsed().as_millis() as u64,
        cache_hit: false,
    };

    if let Some(identity) = scan_cache_identity(&cache_path) {
        store_summary(&summary_path, &key, &identity, &dto);
    } else if cache_path.exists() {
        // Scan cache write may have just landed; re-stat.
        if let Some(identity) = scan_cache_identity(&cache_path) {
            store_summary(&summary_path, &key, &identity, &dto);
        }
    }

    Ok(dto)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{UsageCostSource, UsagePricingStatus, UsageSourceStatus};
    use crate::paths::scratch_dir;
    use crate::usage::pricing;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn write_claude_transcript(home: &Path) {
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

    #[test]
    fn missing_dirs_report_missing_sources() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-missing");
        std::env::set_var("ON_N_OFF_HOME", &home);
        let summary = pricing::with_test_fetch(None, || {
            read_summary(UsageSummaryInput {
                since_day: "2026-08-01".into(),
                until_day: "2026-08-31".into(),
                time_zone: "UTC".into(),
                resolution: Some("day".into()),
                since_time: None,
                until_time: None,
                force: false,
            })
        })
        .unwrap();
        assert_eq!(summary.sources.len(), 2);
        assert!(summary
            .sources
            .iter()
            .all(|s| s.status == UsageSourceStatus::Missing));
        assert!(summary.buckets.is_empty());
        assert_eq!(summary.pricing.status, UsagePricingStatus::Unavailable);
        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn scans_claude_fixture_and_dedupes() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-scan");
        write_claude_transcript(&home);
        std::env::set_var("ON_N_OFF_HOME", &home);
        let rates_doc = serde_json::json!({
            "claude-fable-5": {
                "input_cost_per_token": 1e-5,
                "output_cost_per_token": 5e-5,
                "cache_read_input_token_cost": 1e-6,
                "cache_creation_input_token_cost": 1.25e-5
            }
        });
        let summary = pricing::with_test_fetch(Some(rates_doc), || {
            read_summary(UsageSummaryInput {
                since_day: "2026-08-01".into(),
                until_day: "2026-08-31".into(),
                time_zone: "UTC".into(),
                resolution: Some("day".into()),
                since_time: None,
                until_time: None,
                force: false,
            })
        })
        .unwrap();
        let claude = summary
            .sources
            .iter()
            .find(|s| s.provider == AgentId::Claude)
            .unwrap();
        assert_eq!(claude.status, UsageSourceStatus::Ok);
        assert_eq!(claude.scanned_files, 1);
        assert_eq!(summary.buckets.len(), 1);
        assert_eq!(summary.buckets[0].records, 1);
        assert_eq!(summary.buckets[0].totals.output_tokens, 20);
        assert_eq!(summary.buckets[0].cost_source, UsageCostSource::ModelPriced);
        let expected = 10.0 * 1e-5 + 20.0 * 5e-5;
        assert!((summary.buckets[0].cost_usd - expected).abs() < 1e-12);
        assert_eq!(summary.pricing.status, UsagePricingStatus::Fresh);
        assert!(summary.pricing.known_models >= 1);

        let again = pricing::with_test_fetch(None, || {
            read_summary(UsageSummaryInput {
                since_day: "2026-08-01".into(),
                until_day: "2026-08-31".into(),
                time_zone: "UTC".into(),
                resolution: Some("day".into()),
                since_time: None,
                until_time: None,
                force: true,
            })
        })
        .unwrap();
        assert_eq!(again.buckets[0].totals.output_tokens, 20);
        assert_eq!(again.pricing.status, UsagePricingStatus::Cached);
        assert!(!again.cache_hit);
        assert!(home
            .join(".on-n-off")
            .join("usage-scan-cache.json")
            .is_file());

        let cached = pricing::with_test_fetch(None, || {
            read_summary(UsageSummaryInput {
                since_day: "2026-08-01".into(),
                until_day: "2026-08-31".into(),
                time_zone: "UTC".into(),
                resolution: Some("day".into()),
                since_time: None,
                until_time: None,
                force: false,
            })
        })
        .unwrap();
        assert!(cached.cache_hit);
        assert_eq!(cached.scan_duration_ms, 0);
        assert_eq!(cached.buckets[0].totals.output_tokens, 20);

        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn tokens_still_returned_when_rates_unavailable() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-unpriced");
        write_claude_transcript(&home);
        std::env::set_var("ON_N_OFF_HOME", &home);
        let summary = pricing::with_test_fetch(None, || {
            read_summary(UsageSummaryInput {
                since_day: "2026-08-01".into(),
                until_day: "2026-08-31".into(),
                time_zone: "UTC".into(),
                resolution: Some("day".into()),
                since_time: None,
                until_time: None,
                force: false,
            })
        })
        .unwrap();
        assert_eq!(summary.pricing.status, UsagePricingStatus::Unavailable);
        assert_eq!(summary.buckets[0].cost_source, UsageCostSource::Unpriced);
        assert_eq!(summary.buckets[0].totals.output_tokens, 20);
        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn invalid_window_errors() {
        let err = read_summary(UsageSummaryInput {
            since_day: "2026-08-10".into(),
            until_day: "2026-08-01".into(),
            time_zone: "UTC".into(),
            resolution: None,
            since_time: None,
            until_time: None,
            force: false,
        })
        .unwrap_err();
        assert!(err.message.contains("after untilDay"));
    }

    /// Claim-check harness: time real-home 30d summary (warm cache if present).
    /// `cargo test -p on-n-off bench_real_home_usage_summary -- --ignored --nocapture`
    #[test]
    #[ignore = "real-home performance probe; not part of CI"]
    fn bench_real_home_usage_summary() {
        let until = chrono::Local::now().date_naive();
        let since = until - chrono::Duration::days(29);
        let input = UsageSummaryInput {
            since_day: since.format("%Y-%m-%d").to_string(),
            until_day: until.format("%Y-%m-%d").to_string(),
            time_zone: "America/Sao_Paulo".into(),
            resolution: Some("day".into()),
            since_time: None,
            until_time: None,
            force: false,
        };
        eprintln!(
            "bench window {} .. {} (UI default 30d)",
            input.since_day, input.until_day
        );
        for pass in 1..=2 {
            let wall = Instant::now();
            let dto = read_summary(UsageSummaryInput {
                since_day: input.since_day.clone(),
                until_day: input.until_day.clone(),
                time_zone: input.time_zone.clone(),
                resolution: input.resolution.clone(),
                since_time: None,
                until_time: None,
                force: pass == 1,
            })
            .expect("read_summary");
            let wall_ms = wall.elapsed().as_millis();
            let scanned: u64 = dto.sources.iter().map(|s| s.scanned_files).sum();
            let skipped: u64 = dto.sources.iter().map(|s| s.skipped_files).sum();
            let sessions: u64 = dto.sources.iter().map(|s| s.distinct_sessions).sum();
            eprintln!(
                "pass={pass} wall_ms={wall_ms} scan_duration_ms={} cache_hit={} buckets={} scanned_files={} skipped_files={} sessions={}",
                dto.scan_duration_ms,
                dto.cache_hit,
                dto.buckets.len(),
                scanned,
                skipped,
                sessions
            );
            for source in &dto.sources {
                eprintln!(
                    "  {:?} status={:?} scanned={} skipped={} sessions={}",
                    source.provider,
                    source.status,
                    source.scanned_files,
                    source.skipped_files,
                    source.distinct_sessions
                );
            }
        }
    }
}
