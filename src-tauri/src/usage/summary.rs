use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, TimeZone, Utc};

use crate::dto::{
    AdapterError, UsageBucketDto, UsageCostSource, UsagePricingDto, UsageSourceDto,
    UsageSourceStatus, UsageSummaryDto, UsageSummaryInput, UsageTokenTotalsDto,
};
use crate::paths::user_home;

use super::aggregate::{CostSource, UsageAggregator, UsageBucket};
use super::history::{history_fingerprint, history_path_for, HistoryStore};
use super::pricing::{ensure_rates, LITELLM_RATES_URL};
use super::sources::{lock_usage_files, Sources};
use super::summary_cache::{load_summary_hit, store_summary, summary_cache_path_for, summary_key};
use super::window::UsageWindow;
use crate::provider_catalogue::UsageProvider as Provider;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
type PublishPause = (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);

#[cfg(test)]
thread_local! {
    static BEFORE_PUBLISH_PAUSE: std::cell::RefCell<Option<PublishPause>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn with_before_publish_pause<R>(
    reached: std::sync::mpsc::Sender<()>,
    resume: std::sync::mpsc::Receiver<()>,
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
            let _ = reached.send(());
            let _ = resume.recv();
        }
    });
}

#[cfg(not(test))]
fn pause_before_publish_if_requested() {}

fn cost_source_dto(source: CostSource) -> UsageCostSource {
    match source {
        CostSource::ProviderReported => UsageCostSource::ProviderReported,
        CostSource::ModelPriced => UsageCostSource::ModelPriced,
        CostSource::Unpriced => UsageCostSource::Unpriced,
    }
}

fn bucket_to_dto(bucket: UsageBucket) -> UsageBucketDto {
    UsageBucketDto {
        day: bucket.day,
        hour_start: bucket.hour_start,
        provider: bucket.provider.agent(),
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
        provider: provider.agent(),
        status: UsageSourceStatus::Missing,
        scanned_files: 0,
        skipped_files: 0,
        malformed_records: 0,
        distinct_sessions: 0,
        message: Some("No transcript directory on this environment.".into()),
        resolved_path: dir.to_string_lossy().to_string(),
    }
}

pub fn read_summary(input: UsageSummaryInput) -> Result<UsageSummaryDto, AdapterError> {
    read_summary_from(input, user_home)
}

fn read_summary_from(
    input: UsageSummaryInput,
    home: impl FnOnce() -> Result<PathBuf, AdapterError>,
) -> Result<UsageSummaryDto, AdapterError> {
    let window = UsageWindow::of(&input)?;

    let started = Instant::now();
    let started_ms = now_ms();
    let home = home()?;
    let summary_path = summary_cache_path_for(&home);
    let history_path = history_path_for(&home);
    let rates = ensure_rates(&home, started_ms, input.force);
    let key = summary_key(
        &window.cache_identity(),
        rates.fetched_at_ms,
        &history_fingerprint(&history_path),
    );
    let span = window.span();

    let open_history = || HistoryStore::open(history_path.clone());
    let history = OnceCell::new();
    let mut transcripts = Sources::open(lock_usage_files(), &home, || {
        history.get_or_init(open_history).watermark()
    });
    let source_signature = transcripts.signature(span.start_ms, span.end_ms);
    if transcripts.is_complete() && !input.force {
        if let Some(hit) = load_summary_hit(&summary_path, &key, &source_signature) {
            transcripts.finish(|| history.get_or_init(open_history).watermark());
            return Ok(hit);
        }
    }
    let history = history.get_or_init(open_history);
    let source_read = transcripts.read(window.earliest_mtime_ms(), || history.watermark());
    let seen_sources = transcripts.finish(|| history.watermark());

    let mut aggregator = UsageAggregator::new(&window, &rates.table);
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
        history.rows_between(span.start_ms, span.end_ms)
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
        let files = source_read.files_read(source.provider);
        sources.push(UsageSourceDto {
            provider: source.provider.agent(),
            status: UsageSourceStatus::Ok,
            scanned_files: files.scanned,
            skipped_files: files.skipped,
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
            known_models: rates.table.len() as u64,
        },
        scan_duration_ms: started.elapsed().as_millis() as u64,
        cache_hit: false,
    };

    pause_before_publish_if_requested();

    {
        let lock = lock_usage_files();
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
