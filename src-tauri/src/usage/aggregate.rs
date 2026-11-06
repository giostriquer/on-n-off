use std::collections::{HashMap, HashSet};

use super::history::FoldedRow;
use super::pricing::{cache_savings_usd, price_usage, RateTable};
use super::transcripts::{TokenTotals, UsageRecord};
use super::window::{Period, UsageWindow};
use crate::provider_catalogue::UsageProvider;

pub use super::pricing::CostSource;

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

#[derive(Debug, Clone, PartialEq)]
pub struct AggregateResult {
    pub buckets: Vec<UsageBucket>,
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

struct Usage<'a> {
    at_ms: i64,
    provider: UsageProvider,
    model: &'a str,
    totals: &'a TokenTotals,
    records: u64,
    reported_cost_usd: Option<f64>,
}

#[derive(PartialEq, Eq, Hash)]
struct BucketKey {
    period: Period,
    provider: UsageProvider,
    model: String,
}

pub struct UsageAggregator<'w> {
    buckets: HashMap<BucketKey, MutableBucket>,
    window: &'w UsageWindow,
    rates: &'w RateTable,
    out_of_window: u64,
}

impl<'w> UsageAggregator<'w> {
    pub fn new(window: &'w UsageWindow, rates: &'w RateTable) -> Self {
        Self {
            buckets: HashMap::new(),
            window,
            rates,
            out_of_window: 0,
        }
    }

    pub fn add(&mut self, record: &UsageRecord) -> bool {
        self.add_usage(
            Usage {
                at_ms: record.timestamp_ms,
                provider: record.provider,
                model: &record.model,
                totals: &record.totals,
                records: 1,
                reported_cost_usd: record.reported_cost_usd,
            },
            [record.session_id.as_str()],
        )
    }

    pub fn add_folded(&mut self, row: &FoldedRow) -> bool {
        self.add_usage(
            Usage {
                at_ms: row.slot_start_ms,
                provider: row.provider,
                model: &row.model,
                totals: &row.totals,
                records: row.records,
                reported_cost_usd: row.reported.then_some(row.reported_cost_usd),
            },
            row.sessions.iter().map(String::as_str),
        )
    }

    fn add_usage<'s>(
        &mut self,
        usage: Usage<'_>,
        sessions: impl IntoIterator<Item = &'s str>,
    ) -> bool {
        let Some(period) = self.window.period_of(usage.at_ms) else {
            self.out_of_window += usage.records;
            return false;
        };
        let key = BucketKey {
            period,
            provider: usage.provider,
            model: usage.model.to_string(),
        };

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
            self.rates,
            usage.model,
            usage.totals,
            usage.reported_cost_usd,
        );
        bucket.totals = bucket.totals.add(usage.totals);
        bucket.cost_usd += priced.cost_usd;
        bucket.cache_savings_usd += cache_savings_usd(self.rates, usage.model, usage.totals);
        bucket.records += usage.records;
        match priced.cost_source {
            CostSource::Unpriced => bucket.unpriced_records += usage.records,
            CostSource::ProviderReported => bucket.provider_reported_records += usage.records,
            CostSource::ModelPriced => {}
        }
        for session in sessions {
            if !session.is_empty() {
                bucket.sessions.insert(session.to_string());
            }
        }
        true
    }

    pub fn finish(self) -> AggregateResult {
        let mut buckets = Vec::with_capacity(self.buckets.len());
        for (key, bucket) in self.buckets {
            let cost_source = resolve_cost_source(&bucket);
            buckets.push(UsageBucket {
                day: key.period.day(),
                hour_start: key.period.hour_start(),
                provider: key.provider,
                model: key.model,
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
            (&a.day, &a.hour_start, a.provider, &a.model).cmp(&(
                &b.day,
                &b.hour_start,
                b.provider,
                &b.model,
            ))
        });
        AggregateResult {
            buckets,
            out_of_window: self.out_of_window,
        }
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

#[cfg(test)]
mod tests;
