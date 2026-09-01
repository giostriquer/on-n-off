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
use super::pricing::{ensure_rates, take_unpriced_seen, RatesRefresh, LITELLM_RATES_URL};
use super::scan_cache::{
    decode_scan_cache, encode_scan_cache, prune_scan_cache, PruneOptions, ScanCache,
};
use super::source_index::{
    inventory_sources, normalize_path, prepare_sources, reconcile_inventory, source_index_path_for,
    unchanged_snapshot, SourceRoot,
};
use super::summary_cache::{load_summary_hit, store_summary, summary_cache_path_for, summary_key};
use super::transcripts::UsageProvider as Provider;

const MTIME_SLACK_MS: i64 = 36 * 60 * 60 * 1000;
const MAX_HOURLY_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

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
    // Rates first: the table's age is part of the summary key, so a re-fetched table (a model
    // released today, a price change) never serves a summary priced with the old one. A scan
    // that met an unknown model lets the next one re-fetch early; the refresh button forces it.
    let refresh = if input.force {
        RatesRefresh::Forced
    } else if take_unpriced_seen() {
        RatesRefresh::Early
    } else {
        RatesRefresh::Scheduled
    };
    let rates = ensure_rates(&home, started_ms, refresh);
    let key = summary_key(&input, rates.fetched_at_ms);
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
        let inventory = inventory_sources(&roots);
        let unchanged = unchanged_snapshot(&source_index_path, &inventory);
        if let Some(snapshot) = unchanged.as_ref() {
            let signature = snapshot.signature(signature_start_ms, signature_end_ms);
            if snapshot.is_complete() && !input.force {
                if let Some(hit) = load_summary_hit(&summary_path, &key, &signature) {
                    return Ok(hit);
                }
            }
        }

        let mut file_cache = load_scan_cache(&cache_path);
        let (source_snapshot, scan_cache_dirty, summary_already_checked) = if let Some(snapshot) =
            unchanged
        {
            (snapshot, false, true)
        } else {
            let reconciled = reconcile_inventory(&source_index_path, inventory, &mut file_cache);
            (reconciled.snapshot, reconciled.scan_cache_dirty, false)
        };
        let source_signature = source_snapshot.signature(signature_start_ms, signature_end_ms);
        if !summary_already_checked && source_snapshot.is_complete() && !input.force {
            if let Some(hit) = load_summary_hit(&summary_path, &key, &source_signature) {
                return Ok(hit);
            }
        }
        let prepared_sources = prepare_sources(&source_snapshot, &mut file_cache, window_start_ms);
        let live_paths = source_snapshot.live_paths();
        let active_roots: Vec<String> = roots
            .iter()
            .map(|root| normalize_path(&root.path))
            .collect();
        let pruned = prune_scan_cache(
            &mut file_cache,
            PruneOptions {
                live_paths: &live_paths,
                active_roots: &active_roots,
                walked_roots: source_snapshot.successfully_walked_root_paths(),
            },
        );
        if scan_cache_dirty || prepared_sources.scan_cache_dirty || pruned > 0 {
            persist_scan_cache(&cache_path, &file_cache);
        }
        (source_snapshot, source_signature, prepared_sources)
    };

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
#[path = "summary_tests.rs"]
mod tests;
