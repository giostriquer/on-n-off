//! The Usage summary: a window's usage counted from the transcript sources (`sources`) and the
//! folded rows of the usage history (`history`), priced (`pricing`) into a UsageSummaryDto. What is
//! its own is the window and when a count is final: when a stored summary may be served
//! (`summary_cache`), and when one may be stored.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, NaiveDate, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;

use crate::dto::{
    AdapterError, AgentId, UsageBucketDto, UsageCostSource, UsagePricingDto, UsageSourceDto,
    UsageSourceStatus, UsageSummaryDto, UsageSummaryInput, UsageTokenTotalsDto,
};
use crate::paths::user_home;

use super::aggregate::{
    AggregateOptions as AggOpts, CostSource, Resolution as AggResolution, UsageAggregator,
    UsageBucket,
};
use super::history::{history_fingerprint, history_path_for, HistoryStore};
use super::pricing::{ensure_rates, LITELLM_RATES_URL};
use super::sources::{lock_usage_files, Sources};
use super::summary_cache::{load_summary_hit, store_summary, summary_cache_path_for, summary_key};
use super::transcripts::UsageProvider as Provider;

const MAX_HOURLY_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
thread_local! {
    static BEFORE_PUBLISH_PAUSE: std::cell::RefCell<Option<(std::sync::Arc<std::sync::Barrier>, std::sync::Arc<std::sync::Barrier>)>> = const { std::cell::RefCell::new(None) };
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
    read_summary_from(input, user_home)
}

/// The home is resolved only once the input has passed validation, as `read_summary` always has.
fn read_summary_from(
    input: UsageSummaryInput,
    home: impl FnOnce() -> Result<PathBuf, AdapterError>,
) -> Result<UsageSummaryDto, AdapterError> {
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
    let home = home()?;
    let summary_path = summary_cache_path_for(&home);
    let history_path = history_path_for(&home);
    // Rates first: the table's age is part of the summary key, so a re-fetched table (a model
    // released today, a price change) never serves a summary priced with the old one. The
    // parsed table is memoised on the file, so this costs one metadata read on the fast path.
    let rates = ensure_rates(&home, started_ms, input.force);
    let key = summary_key(
        &input,
        rates.fetched_at_ms,
        &history_fingerprint(&history_path),
    );
    let (signature_start_ms, signature_end_ms) = source_window_bounds(&input);
    let since_ms = since_time_ms.unwrap_or_else(|| {
        DateTime::parse_from_rfc3339(&format!("{}T00:00:00Z", input.since_day))
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0)
    });

    // Read, never folded here: the background fold owns that (`folding`). Opened under the lock,
    // once the sources need its watermark, so a summary served from an unchanged index never
    // reads it.
    let open_history = || HistoryStore::open(history_path.clone());
    let history = OnceCell::new();
    let mut transcripts = Sources::open(lock_usage_files(), &home, || {
        history.get_or_init(open_history).watermark()
    });
    let source_signature = transcripts.signature(signature_start_ms, signature_end_ms);
    if transcripts.is_complete() && !input.force {
        if let Some(hit) = load_summary_hit(&summary_path, &key, &source_signature) {
            // What bringing the index up to date parsed is kept, though nothing else is read.
            transcripts.finish(|| history.get_or_init(open_history).watermark());
            return Ok(hit);
        }
    }
    let source_read = transcripts.read(since_ms, || history.get_or_init(open_history).watermark());
    let history = history.into_inner().unwrap_or_else(open_history);
    let seen_sources = transcripts.finish(|| history.watermark());

    let rates_arc = rates.table.clone();

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

    // Copies of one record across files (resumed Claude sessions, a Codex rollout listed under
    // both roots) collapse before anything is counted, so the totals and the session counts both
    // follow the copy that is counted. Below the watermark the history is the count: a
    // transcript's copy of a folded record, a resumed session's included, is not counted again.
    let records = source_read.records();
    let mut session_ids: HashMap<Provider, HashSet<&str>> = HashMap::new();
    for record in records {
        if aggregator.add(record) && !record.session_id.is_empty() {
            session_ids
                .entry(record.provider)
                .or_default()
                .insert(&record.session_id);
        }
    }
    let folded_rows = history.history().map_or(&[][..], |history| {
        history.rows_between(signature_start_ms, signature_end_ms)
    });
    for row in folded_rows {
        if aggregator.add_folded(row) {
            session_ids
                .entry(row.provider)
                .or_default()
                .extend(row.sessions.iter().map(String::as_str));
        }
    }

    let mut sources = Vec::new();
    for source in seen_sources.providers() {
        if !source.present {
            sources.push(missing_source(source.provider, source.dir));
            continue;
        }
        let (scanned_files, skipped_files) = source_read.files_read(source.provider);
        sources.push(UsageSourceDto {
            provider: provider_agent(source.provider),
            status: UsageSourceStatus::Ok,
            scanned_files,
            skipped_files,
            malformed_records: 0,
            distinct_sessions: session_ids.get(&source.provider).map_or(0, HashSet::len) as u64,
            message: None,
            resolved_path: source.dir.to_string_lossy().to_string(),
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
        let lock = lock_usage_files();
        // A summary counted around a history that did not read would undercount once it reads.
        if source_read.complete && history.history().is_some() && seen_sources.unchanged(&lock) {
            store_summary(&summary_path, &key, &source_signature, &dto);
        }
    }

    Ok(dto)
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
