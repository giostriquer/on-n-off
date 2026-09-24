use super::*;
use crate::usage::history::{FoldedRow, UsageHistory};

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
            cache_creation_1h_tokens: 0,
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
            cache_creation_1h_cost_per_token: 2e-5,
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
fn add_says_whether_the_record_was_counted() {
    let mut agg = UsageAggregator::new(AggregateOptions {
        time_zone: "UTC".into(),
        since_day: "2026-08-01".into(),
        until_day: "2026-08-31".into(),
        resolution: Resolution::Day,
        since_time_ms: None,
        until_time_ms: None,
        rates: sample_rates(),
    })
    .unwrap();
    assert!(agg.add(&record(|r| r.dedupe_key = Some("msg_1:".into()))));
    assert!(!agg.add(&record(|r| {
        r.timestamp_ms = chrono::DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
            .unwrap()
            .timestamp_millis();
    })));
    let result = agg.finish();
    assert_eq!(result.buckets.len(), 1);
    assert_eq!(result.out_of_window, 1);
}

/// The hourly window is `[since, until)`: its first instant counts, its end does not.
#[test]
fn hourly_window_includes_its_start_and_excludes_its_end() {
    let since = chrono::DateTime::parse_from_rfc3339("2026-08-06T04:37:00.000Z")
        .unwrap()
        .timestamp_millis();
    let until = chrono::DateTime::parse_from_rfc3339("2026-08-07T04:37:00.000Z")
        .unwrap()
        .timestamp_millis();
    // Distinct output per instant, so a window shifted by a millisecond at both ends cannot
    // count the same total.
    let at = |timestamp_ms: i64, output: u64| {
        record(|r| {
            r.timestamp_ms = timestamp_ms;
            r.totals.output_tokens = output;
        })
    };
    let result = aggregate(
        &[
            at(since - 1, 1),
            at(since, 10),
            at(until - 1, 100),
            at(until, 1000),
        ],
        "UTC",
        Resolution::Hour,
    );
    let counted: u64 = result
        .buckets
        .iter()
        .map(|bucket| bucket.totals.output_tokens)
        .sum();
    assert_eq!(counted, 110);
    assert_eq!(result.out_of_window, 2);
}

#[test]
fn folds_every_record_it_is_given() {
    let result = aggregate(&[record(|_| {}), record(|_| {})], "UTC", Resolution::Day);
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

fn at(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

/// A day read of August, or an hourly read of `hours`, fed by `feed`.
fn read_with(
    time_zone: &str,
    hours: Option<(i64, i64)>,
    feed: impl FnOnce(&mut UsageAggregator),
) -> AggregateResult {
    let mut agg = UsageAggregator::new(AggregateOptions {
        time_zone: time_zone.into(),
        since_day: "2026-08-01".into(),
        until_day: "2026-08-31".into(),
        resolution: if hours.is_some() {
            Resolution::Hour
        } else {
            Resolution::Day
        },
        since_time_ms: hours.map(|(since, _)| since),
        until_time_ms: hours.map(|(_, until)| until),
        rates: sample_rates(),
    })
    .unwrap();
    feed(&mut agg);
    agg.finish()
}

fn fold_all(records: &[UsageRecord]) -> Vec<FoldedRow> {
    let mut history = UsageHistory::default();
    history.fold(records, i64::MAX, 0);
    history.rows().to_vec()
}

fn assert_same_read(from_records: &AggregateResult, from_rows: &AggregateResult) {
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    assert_eq!(from_rows.out_of_window, from_records.out_of_window);
    assert_eq!(from_rows.buckets.len(), from_records.buckets.len());
    for (row, record) in from_rows.buckets.iter().zip(&from_records.buckets) {
        let key = (&row.day, &row.hour_start, row.provider, &row.model);
        assert_eq!(
            key,
            (
                &record.day,
                &record.hour_start,
                record.provider,
                &record.model
            )
        );
        assert_eq!(row.totals, record.totals, "{key:?}");
        assert_eq!(row.records, record.records, "{key:?}");
        assert_eq!(row.unpriced_records, record.unpriced_records, "{key:?}");
        assert_eq!(row.sessions, record.sessions, "{key:?}");
        assert_eq!(row.cost_source, record.cost_source, "{key:?}");
        assert!(close(row.cost_usd, record.cost_usd), "{key:?}");
        assert!(
            close(row.cache_savings_usd, record.cache_savings_usd),
            "{key:?}"
        );
    }
}

/// Records either side of Kathmandu's midnight (UTC+5:45) and St. John's (UTC-2:30 in August),
/// a provider-reported cost, a model with no price, a second session and one before the window.
fn history_sample() -> Vec<UsageRecord> {
    vec![
        record(|r| r.timestamp_ms = at("2026-08-07T18:14:59.999Z")),
        record(|r| {
            r.timestamp_ms = at("2026-08-07T18:15:00Z");
            r.session_id = "session-b".into();
        }),
        record(|r| {
            r.timestamp_ms = at("2026-08-07T18:16:00Z");
            r.reported_cost_usd = Some(0.4);
        }),
        record(|r| {
            r.timestamp_ms = at("2026-08-07T18:17:00Z");
            r.model = "mystery-model".into();
        }),
        record(|r| {
            r.timestamp_ms = at("2026-08-08T02:29:59Z");
            r.provider = UsageProvider::Codex;
            r.model = "gpt-5.6-sol".into();
        }),
        record(|r| r.timestamp_ms = at("2026-08-08T02:30:00Z")),
        record(|r| r.timestamp_ms = at("2026-07-31T10:00:00Z")),
    ]
}

#[test]
fn folded_rows_read_like_the_records_they_hold_in_any_zone() {
    let records = history_sample();
    let rows = fold_all(&records);
    for zone in [
        "UTC",
        "Asia/Kathmandu",
        "America/St_Johns",
        "America/Los_Angeles",
    ] {
        let from_records = read_with(zone, None, |agg| {
            for record in &records {
                agg.add(record);
            }
        });
        let from_rows = read_with(zone, None, |agg| {
            for row in &rows {
                agg.add_folded(row);
            }
        });
        assert!(!from_records.buckets.is_empty());
        assert_same_read(&from_records, &from_rows);
    }
}

#[test]
fn folded_rows_read_like_their_records_by_the_hour() {
    let records = history_sample();
    let rows = fold_all(&records);
    let hours = Some((at("2026-08-07T18:00:00Z"), at("2026-08-08T02:30:00Z")));
    let from_records = read_with("Asia/Kathmandu", hours, |agg| {
        for record in &records {
            agg.add(record);
        }
    });
    let from_rows = read_with("Asia/Kathmandu", hours, |agg| {
        for row in &rows {
            agg.add_folded(row);
        }
    });

    assert!(from_records.buckets.len() > 1);
    assert_same_read(&from_records, &from_rows);
}

#[test]
fn add_folded_says_whether_the_row_was_counted() {
    let rows = fold_all(&history_sample());
    let inside = rows
        .iter()
        .find(|row| row.slot_start_ms == at("2026-08-07T18:00:00Z"))
        .unwrap();
    let outside = rows
        .iter()
        .find(|row| row.slot_start_ms == at("2026-07-31T10:00:00Z"))
        .unwrap();

    read_with("UTC", None, |agg| {
        assert!(agg.add_folded(inside));
        assert!(!agg.add_folded(outside));
    });
}

/// Cache savings are what the cached input would have cost at the input rate: 1000 cached tokens
/// at 1e-5 input against 1e-6 cache read, per record, summed over the bucket.
#[test]
fn cache_savings_add_up_over_a_bucket() {
    let result = aggregate(&[record(|_| {}), record(|_| {})], "UTC", Resolution::Day);
    assert!((result.buckets[0].cache_savings_usd - 0.018).abs() < 1e-12);
}
