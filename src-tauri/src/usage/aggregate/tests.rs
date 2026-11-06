use super::*;
use crate::dto::UsageSummaryInput;
use crate::usage::files::testing::UsageHistory;
use crate::usage::files::FoldedRow;

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

fn sample_rates() -> RateTable {
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
    table
}

fn august(time_zone: &str) -> UsageWindow {
    UsageWindow::of(&UsageSummaryInput {
        since_day: "2026-08-01".into(),
        until_day: "2026-08-31".into(),
        time_zone: time_zone.into(),
        resolution: Some("day".into()),
        since_time: None,
        until_time: None,
        force: false,
    })
    .unwrap()
}

fn hours(time_zone: &str, since: &str, until: &str) -> UsageWindow {
    UsageWindow::of(&UsageSummaryInput {
        resolution: Some("hour".into()),
        since_time: Some(since.into()),
        until_time: Some(until.into()),
        time_zone: time_zone.into(),
        since_day: "2026-08-01".into(),
        until_day: "2026-08-31".into(),
        force: false,
    })
    .unwrap()
}

fn a_day_from_0437(time_zone: &str) -> UsageWindow {
    hours(
        time_zone,
        "2026-08-06T04:37:00.000Z",
        "2026-08-07T04:37:00.000Z",
    )
}

fn aggregate(records: &[UsageRecord], window: &UsageWindow) -> AggregateResult {
    read_with(window, |agg| {
        for r in records {
            agg.add(r);
        }
    })
}

#[test]
fn a_days_buckets_list_their_providers_in_tab_order_then_their_models() {
    let result = aggregate(
        &[
            record(|r| {
                r.provider = UsageProvider::Muse;
                r.model = "a-model".into();
            }),
            record(|r| {
                r.provider = UsageProvider::Codex;
                r.model = "b-model".into();
            }),
            record(|r| r.model = "c-model".into()),
            record(|r| r.model = "a-model".into()),
        ],
        &august("UTC"),
    );

    let order: Vec<(UsageProvider, &str)> = result
        .buckets
        .iter()
        .map(|bucket| (bucket.provider, bucket.model.as_str()))
        .collect();
    assert_eq!(
        order,
        [
            (UsageProvider::Claude, "a-model"),
            (UsageProvider::Claude, "c-model"),
            (UsageProvider::Codex, "b-model"),
            (UsageProvider::Muse, "a-model"),
        ]
    );
}

#[test]
fn add_says_whether_the_record_was_counted() {
    let (window, rates) = (august("UTC"), sample_rates());
    let mut agg = UsageAggregator::new(&window, &rates);
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

#[test]
fn hourly_window_includes_its_start_and_excludes_its_end() {
    let since = chrono::DateTime::parse_from_rfc3339("2026-08-06T04:37:00.000Z")
        .unwrap()
        .timestamp_millis();
    let until = chrono::DateTime::parse_from_rfc3339("2026-08-07T04:37:00.000Z")
        .unwrap()
        .timestamp_millis();
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
        &a_day_from_0437("UTC"),
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
fn buckets_by_timezone_day() {
    let utc = aggregate(&[record(|_| {})], &august("UTC"));
    let la = aggregate(&[record(|_| {})], &august("America/Los_Angeles"));
    assert_eq!(utc.buckets[0].day, "2026-08-07");
    assert_eq!(la.buckets[0].day, "2026-08-06");
}

#[test]
fn unknown_timezone_falls_back_to_utc() {
    let result = aggregate(&[record(|_| {})], &august("Not/AZone"));
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
    let result = aggregate(&[a, b], &a_day_from_0437("America/Los_Angeles"));
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
        &august("UTC"),
    );
    assert_eq!(result.buckets.len(), 0);
    assert_eq!(result.out_of_window, 1);
}

fn at(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

fn read_with(window: &UsageWindow, feed: impl FnOnce(&mut UsageAggregator)) -> AggregateResult {
    let rates = sample_rates();
    let mut agg = UsageAggregator::new(window, &rates);
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
        let from_records = read_with(&august(zone), |agg| {
            for record in &records {
                agg.add(record);
            }
        });
        let from_rows = read_with(&august(zone), |agg| {
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
    let window = hours(
        "Asia/Kathmandu",
        "2026-08-07T18:00:00Z",
        "2026-08-08T02:30:00Z",
    );
    let from_records = read_with(&window, |agg| {
        for record in &records {
            agg.add(record);
        }
    });
    let from_rows = read_with(&window, |agg| {
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

    read_with(&august("UTC"), |agg| {
        assert!(agg.add_folded(inside));
        assert!(!agg.add_folded(outside));
    });
}

#[test]
fn a_bucket_adds_up_its_records_tokens_and_cache_savings() {
    let result = aggregate(&[record(|_| {}), record(|_| {})], &august("UTC"));
    assert_eq!(result.buckets[0].totals.output_tokens, 100);
    assert!((result.buckets[0].cache_savings_usd - 0.018).abs() < 1e-12);
}
