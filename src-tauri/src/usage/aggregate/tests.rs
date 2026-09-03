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
