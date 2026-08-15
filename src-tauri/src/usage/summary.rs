//! Orchestrates transcript scan + pricing into a UsageSummaryDto.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, NaiveDate, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;

use crate::dto::{
    AdapterError, AgentId, UsageBucketDto, UsageCostSource, UsagePricingDto, UsageSourceDto,
    UsageSourceStatus, UsageSummaryDto, UsageSummaryInput, UsageTokenTotalsDto,
};
use crate::paths::{claude_root, codex_root, user_home};

use super::aggregate::{
    AggregateOptions as AggOpts, CostSource, Resolution as AggResolution, UsageAggregator,
    UsageBucket,
};
use super::cache_io::atomic_write;
use super::pricing::{ensure_rates, LITELLM_RATES_URL};
use super::scan_cache::{
    decode_scan_cache, encode_scan_cache, prune_scan_cache, PruneOptions, ScanCache,
};
use super::source_index::{
    normalize_path, prepare_sources, reconcile, source_index_path_for, SourceRoot,
};
use super::summary_cache::{load_summary_hit, store_summary, summary_cache_path_for, window_key};
use super::transcripts::UsageProvider as Provider;

const MTIME_SLACK_MS: i64 = 36 * 60 * 60 * 1000;
const MAX_HOURLY_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
const CACHE_RETENTION_DAYS: i64 = 90;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn usage_cache_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
thread_local! {
    static BEFORE_PUBLISH_PAUSE: std::cell::RefCell<Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn with_before_publish_pause<R>(
    reached: Arc<std::sync::Barrier>,
    resume: Arc<std::sync::Barrier>,
    action: impl FnOnce() -> R,
) -> R {
    BEFORE_PUBLISH_PAUSE.with(|pause| *pause.borrow_mut() = Some((reached, resume)));
    let result = action();
    BEFORE_PUBLISH_PAUSE.with(|pause| *pause.borrow_mut() = None);
    result
}

#[cfg(test)]
fn pause_before_publish_if_requested() {
    BEFORE_PUBLISH_PAUSE.with(|pause| {
        if let Some((reached, resume)) = pause.borrow().as_ref() {
            reached.wait();
            resume.wait();
        }
    });
}

#[cfg(not(test))]
fn pause_before_publish_if_requested() {}

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
    let doc = encode_scan_cache(cache);
    if let Ok(raw) = serde_json::to_string(&doc) {
        let _ = atomic_write(path, &raw);
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

fn source_window_bounds(input: &UsageSummaryInput) -> (i64, i64) {
    if let (Some(since), Some(until)) = (
        input.since_time.as_deref().and_then(parse_iso_ms),
        input.until_time.as_deref().and_then(parse_iso_ms),
    ) {
        return (since, until);
    }

    let Some(since_day) = NaiveDate::parse_from_str(&input.since_day, "%Y-%m-%d").ok() else {
        return (i64::MIN, i64::MAX);
    };
    let Some(after_until) = NaiveDate::parse_from_str(&input.until_day, "%Y-%m-%d")
        .ok()
        .and_then(|day| day.succ_opt())
    else {
        return (i64::MIN, i64::MAX);
    };
    let zone: Tz = input.time_zone.parse().unwrap_or(chrono_tz::UTC);
    let Some(start) = since_day
        .and_hms_opt(0, 0, 0)
        .and_then(|local| zone.from_local_datetime(&local).earliest())
    else {
        return (i64::MIN, i64::MAX);
    };
    let Some(end) = after_until
        .and_hms_opt(0, 0, 0)
        .and_then(|local| zone.from_local_datetime(&local).latest())
    else {
        return (i64::MIN, i64::MAX);
    };
    (start.timestamp_millis(), end.timestamp_millis())
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
    let source_index_path = source_index_path_for(&home);
    let key = window_key(&input);
    let dirs = [
        (Provider::Claude, resolve_claude_transcript_dir()?),
        (Provider::Codex, resolve_codex_transcript_dir()?),
    ];
    let roots: Vec<SourceRoot> = dirs
        .iter()
        .map(|(provider, path)| SourceRoot {
            provider: *provider,
            path: path.clone(),
        })
        .collect();
    let (signature_start_ms, signature_end_ms) = source_window_bounds(&input);
    let window_start_ms = since_time_ms.unwrap_or_else(|| {
        DateTime::parse_from_rfc3339(&format!("{}T00:00:00Z", input.since_day))
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0)
    }) - MTIME_SLACK_MS;

    let (source_snapshot, source_signature, prepared_sources) = {
        let _cache_guard = usage_cache_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut file_cache = load_scan_cache(&cache_path);
        let reconciled = reconcile(&source_index_path, &roots, &mut file_cache);
        let source_snapshot = reconciled.snapshot;
        let source_signature = source_snapshot.signature(signature_start_ms, signature_end_ms);
        if source_snapshot.is_complete() && !input.force {
            if let Some(hit) = load_summary_hit(&summary_path, &key, &source_signature) {
                return Ok(hit);
            }
        }
        let prepared_sources = prepare_sources(&source_snapshot, &mut file_cache, window_start_ms);
        let live_paths: HashSet<String> = prepared_sources
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect();
        let walked_roots: Vec<String> = roots
            .iter()
            .filter(|root| source_snapshot.root_is_present(root))
            .map(|root| normalize_path(&root.path))
            .collect();
        let pruned = prune_scan_cache(
            &mut file_cache,
            PruneOptions {
                live_paths: &live_paths,
                walked_roots: &walked_roots,
                window_start_ms,
                retention_cutoff_ms: started_ms - CACHE_RETENTION_DAYS * 24 * 60 * 60 * 1000,
            },
        );
        if reconciled.scan_cache_dirty || prepared_sources.scan_cache_dirty || pruned > 0 {
            persist_scan_cache(&cache_path, &file_cache);
        }
        (source_snapshot, source_signature, prepared_sources)
    };

    let rates = ensure_rates(&home, started_ms);
    let rates_arc = Arc::new(rates.table);

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

    for ((provider, dir), root) in dirs.iter().zip(&roots) {
        if !source_snapshot.root_is_present(root) {
            sources.push(missing_source(*provider, dir));
            continue;
        }

        let mut scanned_files = 0u64;
        let mut skipped_files = 0u64;
        let mut session_ids = HashSet::new();

        for file in prepared_sources
            .files
            .iter()
            .filter(|file| file.provider == *provider)
        {
            if file.records.is_empty() {
                skipped_files += 1;
                continue;
            }
            scanned_files += 1;
            for record in file.records.iter() {
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

    pause_before_publish_if_requested();

    {
        let _cache_guard = usage_cache_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if source_snapshot.is_complete()
            && prepared_sources.complete
            && source_snapshot.persisted_generation_is_current(&source_index_path)
            && source_snapshot.inventory_is_current(&roots)
        {
            store_summary(&summary_path, &key, &source_signature, &dto);
        }
    }

    Ok(dto)
}

#[cfg(test)]
#[path = "summary_test_support.rs"]
mod test_support;

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::dto::{UsageCostSource, UsagePricingStatus, UsageSourceStatus};
    use crate::paths::scratch_dir;
    use crate::usage::pricing;

    #[test]
    fn cached_summary_is_invalidated_when_transcript_is_appended() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-summary-append");
        write_claude_transcript(&home);
        std::env::set_var("ON_N_OFF_HOME", &home);

        let initial = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!initial.cache_hit);
        assert_eq!(initial.buckets[0].totals.output_tokens, 20);

        append_claude_record(&home, "msg_2", 25);
        let refreshed =
            pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!refreshed.cache_hit);
        assert_eq!(output_tokens(&refreshed), 45);

        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn cached_summary_is_invalidated_when_transcript_is_created() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-summary-create");
        std::fs::create_dir_all(home.join(".claude").join("projects")).unwrap();
        std::env::set_var("ON_N_OFF_HOME", &home);
        let initial = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(initial.buckets.is_empty());

        write_single_claude_record(&home, "created.jsonl", "2026-08-07T04:05:13.944Z", 20);
        let refreshed =
            pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!refreshed.cache_hit);
        assert_eq!(output_tokens(&refreshed), 20);

        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn cached_summary_is_invalidated_by_same_size_rewrite() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-summary-rewrite");
        let path =
            write_single_claude_record(&home, "rewrite.jsonl", "2026-08-07T04:05:13.944Z", 20);
        std::env::set_var("ON_N_OFF_HOME", &home);
        let initial = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert_eq!(output_tokens(&initial), 20);
        let original_size = std::fs::metadata(&path).unwrap().len();

        std::thread::sleep(std::time::Duration::from_millis(20));
        write_single_claude_record(&home, "rewrite.jsonl", "2026-08-07T04:05:13.944Z", 25);
        assert_eq!(std::fs::metadata(path).unwrap().len(), original_size);
        let refreshed =
            pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!refreshed.cache_hit);
        assert_eq!(output_tokens(&refreshed), 25);

        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn cached_summary_is_invalidated_when_transcript_is_deleted() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-summary-delete");
        let path =
            write_single_claude_record(&home, "delete.jsonl", "2026-08-07T04:05:13.944Z", 20);
        std::env::set_var("ON_N_OFF_HOME", &home);
        let initial = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert_eq!(output_tokens(&initial), 20);

        std::fs::remove_file(path).unwrap();
        let refreshed =
            pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!refreshed.cache_hit);
        assert!(refreshed.buckets.is_empty());

        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn cached_summary_survives_disjoint_window_change() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-summary-disjoint");
        write_single_claude_record(&home, "july.jsonl", "2026-07-07T04:05:13.944Z", 10);
        let august_path =
            write_single_claude_record(&home, "august.jsonl", "2026-08-07T04:05:13.944Z", 20);
        std::env::set_var("ON_N_OFF_HOME", &home);
        let july_input = || day_input("2026-07-01", "2026-07-31", false);
        let initial = pricing::with_test_fetch(None, || read_summary(july_input())).unwrap();
        assert_eq!(output_tokens(&initial), 10);

        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut august = std::fs::read_to_string(&august_path).unwrap();
        august.push_str(
            &serde_json::json!({
                "type": "assistant",
                "timestamp": "2026-08-08T04:05:13.944Z",
                "sessionId": "sess-claude",
                "message": {
                    "id": "august-2",
                    "model": "claude-fable-5",
                    "usage": { "input_tokens": 1, "output_tokens": 25 }
                }
            })
            .to_string(),
        );
        august.push('\n');
        std::fs::write(august_path, august).unwrap();

        let unchanged = pricing::with_test_fetch(None, || read_summary(july_input())).unwrap();
        assert!(unchanged.cache_hit);
        assert_eq!(output_tokens(&unchanged), 10);

        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn corrupt_summary_cache_is_rebuilt() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-summary-corrupt");
        write_claude_transcript(&home);
        std::env::set_var("ON_N_OFF_HOME", &home);
        pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        let summary_path = summary_cache_path_for(&home);
        std::fs::write(&summary_path, "{partial").unwrap();

        let rebuilt = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!rebuilt.cache_hit);
        assert_eq!(output_tokens(&rebuilt), 20);
        assert!(serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(summary_path).unwrap()
        )
        .is_ok());

        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn incompatible_and_partial_cache_documents_rebuild_together() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-summary-cache-migration");
        write_claude_transcript(&home);
        let cache_root = home.join(".on-n-off");
        std::fs::create_dir_all(&cache_root).unwrap();
        std::fs::write(
            cache_root.join("usage-scan-cache.json"),
            r#"{"version":1,"models":[],"sessions":[],"files":{}}"#,
        )
        .unwrap();
        std::fs::write(
            cache_root.join("usage-source-index.json"),
            r#"{"version":1,"generation":99,"entries":{}}"#,
        )
        .unwrap();
        std::fs::write(
            cache_root.join("usage-summary-cache.json"),
            r#"{"version":1,"entries":[]}"#,
        )
        .unwrap();
        std::env::set_var("ON_N_OFF_HOME", &home);

        let migrated =
            pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!migrated.cache_hit);
        assert_eq!(output_tokens(&migrated), 20);
        for (cache_name, expected_version) in [
            (
                "usage-source-index.json",
                super::super::source_index::USAGE_SOURCE_INDEX_VERSION,
            ),
            (
                "usage-scan-cache.json",
                super::super::scan_cache::USAGE_SCAN_CACHE_VERSION,
            ),
            (
                "usage-summary-cache.json",
                super::super::summary_cache::USAGE_SUMMARY_CACHE_VERSION,
            ),
        ] {
            let document: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(cache_root.join(cache_name)).unwrap(),
            )
            .unwrap();
            assert_eq!(
                document["version"].as_u64(),
                Some(u64::from(expected_version))
            );
        }

        for cache_name in [
            "usage-source-index.json",
            "usage-scan-cache.json",
            "usage-summary-cache.json",
        ] {
            std::fs::write(cache_root.join(cache_name), "{partial").unwrap();
        }
        let rebuilt = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!rebuilt.cache_hit);
        assert_eq!(output_tokens(&rebuilt), 20);

        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn older_generation_cannot_overwrite_newer_summary() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-summary-concurrent");
        write_claude_transcript(&home);
        std::env::set_var("ON_N_OFF_HOME", &home);

        let reached = Arc::new(std::sync::Barrier::new(2));
        let resume = Arc::new(std::sync::Barrier::new(2));
        let old_reached = Arc::clone(&reached);
        let old_resume = Arc::clone(&resume);
        let older = std::thread::spawn(move || {
            with_before_publish_pause(old_reached, old_resume, || {
                pricing::with_test_fetch(None, || read_summary(august_input(true))).unwrap()
            })
        });

        reached.wait();
        append_claude_record(&home, "msg_2", 25);
        let newer = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert_eq!(output_tokens(&newer), 45);
        resume.wait();

        let older = older.join().unwrap();
        assert_eq!(output_tokens(&older), 20);
        let final_hit =
            pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(final_hit.cache_hit);
        assert_eq!(output_tokens(&final_hit), 45);
        for cache_name in [
            "usage-source-index.json",
            "usage-scan-cache.json",
            "usage-summary-cache.json",
        ] {
            let raw = std::fs::read_to_string(home.join(".on-n-off").join(cache_name)).unwrap();
            assert!(serde_json::from_str::<serde_json::Value>(&raw).is_ok());
        }

        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn cache_write_failures_do_not_fail_correct_summary() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-summary-write-failure");
        write_claude_transcript(&home);
        std::fs::write(home.join(".on-n-off"), "blocks cache directory").unwrap();
        std::env::set_var("ON_N_OFF_HOME", &home);

        let summary = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!summary.cache_hit);
        assert_eq!(output_tokens(&summary), 20);

        std::env::remove_var("ON_N_OFF_HOME");
        let _ = std::fs::remove_dir_all(home);
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
    fn cached_missing_source_tracks_empty_root_creation_and_removal() {
        let _guard = env_lock().lock().unwrap();
        let home = scratch_dir("usage-root-presence");
        std::env::set_var("ON_N_OFF_HOME", &home);

        let missing = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert_eq!(
            missing
                .sources
                .iter()
                .find(|source| source.provider == AgentId::Claude)
                .unwrap()
                .status,
            UsageSourceStatus::Missing
        );
        let cached_missing =
            pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(cached_missing.cache_hit);

        std::fs::create_dir_all(home.join(".claude").join("projects")).unwrap();
        let present = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!present.cache_hit);
        assert_eq!(
            present
                .sources
                .iter()
                .find(|source| source.provider == AgentId::Claude)
                .unwrap()
                .status,
            UsageSourceStatus::Ok
        );

        std::fs::remove_dir_all(home.join(".claude")).unwrap();
        let missing_again =
            pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
        assert!(!missing_again.cache_hit);
        assert_eq!(
            missing_again
                .sources
                .iter()
                .find(|source| source.provider == AgentId::Claude)
                .unwrap()
                .status,
            UsageSourceStatus::Missing
        );

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
