//! Fold usage records into `(day, hour?, provider, model)` buckets.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;

use super::pricing::{cache_savings_usd, price_usage, RateTable};
use super::transcripts::{TokenTotals, UsageProvider, UsageRecord};

pub use super::pricing::CostSource;

const HOUR_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Day,
    Hour,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageBucket {
    pub day: String,
    pub hour_start: Option<String>,
    pub provider: UsageProvider,
    pub model: String,
    pub totals: TokenTotals,
    pub cost_usd: f64,
    pub cache_savings_usd: f64,
    pub cost_source: CostSource,
    pub records: u64,
    pub unpriced_records: u64,
    pub sessions: u64,
}

#[derive(Debug, Clone)]
pub struct AggregateOptions {
    pub time_zone: String,
    pub since_day: String,
    pub until_day: String,
    pub resolution: Resolution,
    pub since_time_ms: Option<i64>,
    pub until_time_ms: Option<i64>,
    pub rates: Arc<RateTable>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AggregateResult {
    pub buckets: Vec<UsageBucket>,
    pub duplicates_dropped: u64,
    pub out_of_window: u64,
}

struct MutableBucket {
    totals: TokenTotals,
    cost_usd: f64,
    cache_savings_usd: f64,
    records: u64,
    unpriced_records: u64,
    provider_reported_records: u64,
    sessions: HashSet<String>,
}

pub struct UsageAggregator {
    buckets: HashMap<String, MutableBucket>,
    seen: HashSet<String>,
    zone: Tz,
    hourly: Option<(i64, i64)>,
    options: AggregateOptions,
    duplicates_dropped: u64,
    out_of_window: u64,
}

impl UsageAggregator {
    pub fn new(options: AggregateOptions) -> Result<Self, String> {
        let hourly = match options.resolution {
            Resolution::Hour => {
                let since = options
                    .since_time_ms
                    .ok_or_else(|| "Hourly usage aggregation requires exact time bounds".to_string())?;
                let until = options
                    .until_time_ms
                    .ok_or_else(|| "Hourly usage aggregation requires exact time bounds".to_string())?;
                Some((since, until))
            }
            Resolution::Day => None,
        };
        let zone: Tz = options.time_zone.parse().unwrap_or(chrono_tz::UTC);
        Ok(Self {
            buckets: HashMap::new(),
            seen: HashSet::new(),
            zone,
            hourly,
            options,
            duplicates_dropped: 0,
            out_of_window: 0,
        })
    }

    pub fn add(&mut self, record: &UsageRecord) -> bool {
        if let Some(key) = &record.dedupe_key {
            if !self.seen.insert(key.clone()) {
                self.duplicates_dropped += 1;
                return false;
            }
        }

        if let Some((since, until)) = self.hourly {
            if record.timestamp_ms < since || record.timestamp_ms >= until {
                self.out_of_window += 1;
                return false;
            }
        }

        let day = day_in_zone(record.timestamp_ms, self.zone);
        if self.hourly.is_none()
            && (day.as_str() < self.options.since_day.as_str()
                || day.as_str() > self.options.until_day.as_str())
        {
            self.out_of_window += 1;
            return false;
        }

        let hour_start = self.hourly.map(|(since, _)| {
            let offset = ((record.timestamp_ms - since) / HOUR_MS) * HOUR_MS;
            ms_to_iso(since + offset)
        });

        let key = format!(
            "{}\0{}\0{}\0{}",
            day,
            hour_start.as_deref().unwrap_or(""),
            record.provider.as_str(),
            record.model
        );

        let bucket = self.buckets.entry(key).or_insert_with(|| MutableBucket {
            totals: TokenTotals::default(),
            cost_usd: 0.0,
            cache_savings_usd: 0.0,
            records: 0,
            unpriced_records: 0,
            provider_reported_records: 0,
            sessions: HashSet::new(),
        });

        let priced = price_usage(
            &self.options.rates,
            &record.model,
            &record.totals,
            record.reported_cost_usd,
        );
        bucket.totals = bucket.totals.add(&record.totals);
        bucket.cost_usd += priced.cost_usd;
        bucket.cache_savings_usd +=
            cache_savings_usd(&self.options.rates, &record.model, &record.totals);
        bucket.records += 1;
        match priced.cost_source {
            CostSource::Unpriced => bucket.unpriced_records += 1,
            CostSource::ProviderReported => bucket.provider_reported_records += 1,
            CostSource::ModelPriced => {}
        }
        if !record.session_id.is_empty() {
            bucket.sessions.insert(record.session_id.clone());
        }
        true
    }

    pub fn finish(self) -> AggregateResult {
        let mut buckets = Vec::with_capacity(self.buckets.len());
        for (key, bucket) in self.buckets {
            let mut parts = key.split('\0');
            let day = parts.next().unwrap_or("").to_string();
            let hour = parts.next().unwrap_or("");
            let provider = match parts.next().unwrap_or("") {
                "codex" => UsageProvider::Codex,
                _ => UsageProvider::Claude,
            };
            let model = parts.next().unwrap_or("").to_string();
            let cost_source = resolve_cost_source(&bucket);
            buckets.push(UsageBucket {
                day,
                hour_start: if hour.is_empty() {
                    None
                } else {
                    Some(hour.to_string())
                },
                provider,
                model,
                totals: bucket.totals,
                cost_usd: bucket.cost_usd,
                cache_savings_usd: bucket.cache_savings_usd,
                cost_source,
                records: bucket.records,
                unpriced_records: bucket.unpriced_records,
                sessions: bucket.sessions.len() as u64,
            });
        }
        buckets.sort_by(|a, b| {
            a.day
                .cmp(&b.day)
                .then(a.hour_start.cmp(&b.hour_start))
                .then(provider_ord(a.provider).cmp(&provider_ord(b.provider)))
                .then(a.model.cmp(&b.model))
        });
        AggregateResult {
            buckets,
            duplicates_dropped: self.duplicates_dropped,
            out_of_window: self.out_of_window,
        }
    }
}

fn provider_ord(provider: UsageProvider) -> u8 {
    match provider {
        UsageProvider::Claude => 0,
        UsageProvider::Codex => 1,
    }
}

fn resolve_cost_source(bucket: &MutableBucket) -> CostSource {
    if bucket.unpriced_records == bucket.records {
        CostSource::Unpriced
    } else if bucket.provider_reported_records == bucket.records {
        CostSource::ProviderReported
    } else {
        CostSource::ModelPriced
    }
}

fn day_in_zone(timestamp_ms: i64, zone: Tz) -> String {
    let Some(utc) = Utc.timestamp_millis_opt(timestamp_ms).single() else {
        return "1970-01-01".to_string();
    };
    utc.with_timezone(&zone).format("%Y-%m-%d").to_string()
}

fn ms_to_iso(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(overrides: impl FnOnce(&mut UsageRecord)) -> UsageRecord {
        let mut r = UsageRecord {
            provider: UsageProvider::Claude,
            timestamp_ms: chrono::DateTime::parse_from_rfc3339("2026-08-07T04:05:13.944Z")
                .unwrap()
                .timestamp_millis(),
            model: "claude-fable-5".into(),
            session_id: "session-a".into(),
            totals: TokenTotals {
                uncached_input_tokens: 100,
                cached_input_tokens: 1000,
                cache_creation_tokens: 10,
                output_tokens: 50,
                reasoning_tokens: 0,
            },
            reported_cost_usd: None,
            dedupe_key: None,
        };
        overrides(&mut r);
        r
    }

    fn sample_rates() -> Arc<RateTable> {
        let mut table = RateTable::new();
        table.insert(
            "claude-fable-5".into(),
            super::super::pricing::ModelRate {
                input_cost_per_token: 1e-5,
                output_cost_per_token: 5e-5,
                cache_read_cost_per_token: 1e-6,
                cache_creation_cost_per_token: 1.25e-5,
            },
        );
        Arc::new(table)
    }

    fn aggregate(records: &[UsageRecord], time_zone: &str, resolution: Resolution) -> AggregateResult {
        let (since_time_ms, until_time_ms) = if resolution == Resolution::Hour {
            (
                Some(
                    chrono::DateTime::parse_from_rfc3339("2026-08-06T04:37:00.000Z")
                        .unwrap()
                        .timestamp_millis(),
                ),
                Some(
                    chrono::DateTime::parse_from_rfc3339("2026-08-07T04:37:00.000Z")
                        .unwrap()
                        .timestamp_millis(),
                ),
            )
        } else {
            (None, None)
        };
        let mut agg = UsageAggregator::new(AggregateOptions {
            time_zone: time_zone.into(),
            since_day: "2026-08-01".into(),
            until_day: "2026-08-31".into(),
            resolution,
            since_time_ms,
            until_time_ms,
            rates: sample_rates(),
        })
        .unwrap();
        for r in records {
            agg.add(r);
        }
        agg.finish()
    }

    #[test]
    fn hourly_requires_bounds() {
        let result = UsageAggregator::new(AggregateOptions {
            time_zone: "UTC".into(),
            since_day: "2026-08-01".into(),
            until_day: "2026-08-31".into(),
            resolution: Resolution::Hour,
            since_time_ms: None,
            until_time_ms: None,
            rates: sample_rates(),
        });
        match result {
            Err(err) => assert!(err.contains("exact time bounds")),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn dedupe_keeps_first() {
        let result = aggregate(
            &[
                record(|r| r.dedupe_key = Some("msg_1:".into())),
                record(|r| r.dedupe_key = Some("msg_1:".into())),
                record(|r| r.dedupe_key = Some("msg_1:".into())),
            ],
            "UTC",
            Resolution::Day,
        );
        assert_eq!(result.duplicates_dropped, 2);
        assert_eq!(result.buckets.len(), 1);
        assert_eq!(result.buckets[0].records, 1);
        assert_eq!(result.buckets[0].totals.output_tokens, 50);
        assert_eq!(result.buckets[0].cost_source, CostSource::ModelPriced);
    }

    #[test]
    fn sums_records_without_dedupe_key() {
        let result = aggregate(&[record(|_| {}), record(|_| {})], "UTC", Resolution::Day);
        assert_eq!(result.duplicates_dropped, 0);
        assert_eq!(result.buckets[0].totals.output_tokens, 100);
    }

    #[test]
    fn buckets_by_timezone_day() {
        let utc = aggregate(&[record(|_| {})], "UTC", Resolution::Day);
        let la = aggregate(&[record(|_| {})], "America/Los_Angeles", Resolution::Day);
        assert_eq!(utc.buckets[0].day, "2026-08-07");
        assert_eq!(la.buckets[0].day, "2026-08-06");
    }

    #[test]
    fn unknown_timezone_falls_back_to_utc() {
        let result = aggregate(&[record(|_| {})], "Not/AZone", Resolution::Day);
        assert_eq!(result.buckets[0].day, "2026-08-07");
    }

    #[test]
    fn hourly_buckets_anchor_to_window_start() {
        let a = record(|r| {
            r.timestamp_ms = chrono::DateTime::parse_from_rfc3339("2026-08-07T02:40:13.944Z")
                .unwrap()
                .timestamp_millis();
        });
        let b = record(|r| {
            r.timestamp_ms = chrono::DateTime::parse_from_rfc3339("2026-08-07T03:40:13.944Z")
                .unwrap()
                .timestamp_millis();
        });
        let result = aggregate(&[a, b], "America/Los_Angeles", Resolution::Hour);
        let pairs: Vec<_> = result
            .buckets
            .iter()
            .map(|b| (b.day.as_str(), b.hour_start.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("2026-08-06", "2026-08-07T02:37:00.000Z"),
                ("2026-08-06", "2026-08-07T03:37:00.000Z"),
            ]
        );
    }

    #[test]
    fn out_of_window_day_dropped() {
        let result = aggregate(
            &[record(|r| {
                r.timestamp_ms = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
                    .unwrap()
                    .timestamp_millis();
            })],
            "UTC",
            Resolution::Day,
        );
        assert_eq!(result.buckets.len(), 0);
        assert_eq!(result.out_of_window, 1);
    }
}
