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
use super::files::{UsageFiles, WindowCount};
use super::pricing::{ensure_rates, RatesSnapshot, LITELLM_RATES_URL};
use super::window::UsageWindow;
use crate::provider_catalogue::UsageProvider as Provider;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

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
    let home = home()?;
    let rates = ensure_rates(&home, now_ms(), input.force);
    let force = input.force;
    Ok(
        UsageFiles::of(&home).summary(&window, rates.fetched_at_ms, force, |count| {
            summary_of(input, &window, &rates, count, started)
        }),
    )
}

fn summary_of(
    input: UsageSummaryInput,
    window: &UsageWindow,
    rates: &RatesSnapshot,
    count: &WindowCount,
    started: Instant,
) -> UsageSummaryDto {
    let mut aggregator = UsageAggregator::new(window, &rates.table);
    let mut session_ids: HashMap<Provider, HashSet<&str>> = HashMap::new();
    for record in count.records() {
        if aggregator.add(record) && !record.session_id.is_empty() {
            session_ids
                .entry(record.provider)
                .or_default()
                .insert(&record.session_id);
        }
    }
    for row in count.folded_rows() {
        if aggregator.add_folded(row) {
            session_ids
                .entry(row.provider)
                .or_default()
                .extend(row.sessions.iter().map(String::as_str));
        }
    }

    let sources = count
        .providers()
        .map(|source| match source.files {
            None => missing_source(source.provider, source.dir),
            Some(files) => UsageSourceDto {
                provider: source.provider.agent(),
                status: UsageSourceStatus::Ok,
                scanned_files: files.scanned,
                skipped_files: files.skipped,
                malformed_records: 0,
                distinct_sessions: session_ids.get(&source.provider).map_or(0, HashSet::len) as u64,
                message: None,
                resolved_path: source.dir.to_string_lossy().to_string(),
            },
        })
        .collect();
    let aggregated = aggregator.finish();
    let read_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let fetched_at = rates.fetched_at_ms.and_then(|ms| {
        Utc.timestamp_millis_opt(ms)
            .single()
            .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, true))
    });

    UsageSummaryDto {
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
    }
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
