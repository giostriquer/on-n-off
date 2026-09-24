use super::*;
use crate::paths::scratch_dir;

fn ms(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

fn record(at: &str, overrides: impl FnOnce(&mut UsageRecord)) -> UsageRecord {
    let mut record = UsageRecord {
        provider: UsageProvider::Claude,
        timestamp_ms: ms(at),
        model: "claude-fable-5".into(),
        session_id: "session-a".into(),
        totals: TokenTotals {
            uncached_input_tokens: 100,
            cached_input_tokens: 1000,
            cache_creation_tokens: 10,
            cache_creation_1h_tokens: 4,
            output_tokens: 50,
            reasoning_tokens: 5,
        },
        reported_cost_usd: None,
        dedupe_key: None,
    };
    overrides(&mut record);
    record
}

fn folded(records: &[UsageRecord], cutoff: &str) -> UsageHistory {
    let mut history = UsageHistory::default();
    history.fold(records, ms(cutoff), ms("2026-09-24T12:00:00Z"));
    history
}

fn history_path(name: &str) -> (PathBuf, PathBuf) {
    let root = scratch_dir(name);
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("usage-history.json");
    (root, path)
}

#[test]
fn fold_sums_records_into_quarter_hour_utc_slots() {
    let history = folded(
        &[
            record("2026-08-07T04:00:00.000Z", |_| {}),
            record("2026-08-07T04:14:59.999Z", |r| r.totals.output_tokens = 7),
            record("2026-08-07T04:15:00.000Z", |_| {}),
        ],
        "2026-09-01T00:00:00Z",
    );

    let rows = history.rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].slot_start_ms, ms("2026-08-07T04:00:00Z"));
    assert_eq!(rows[0].records, 2);
    assert_eq!(rows[0].totals.output_tokens, 57);
    assert_eq!(rows[0].totals.uncached_input_tokens, 200);
    assert_eq!(rows[0].totals.cache_creation_1h_tokens, 8);
    assert_eq!(rows[0].totals.reasoning_tokens, 10);
    assert_eq!(rows[1].slot_start_ms, ms("2026-08-07T04:15:00Z"));
    assert_eq!(rows[1].records, 1);
}

#[test]
fn fold_keeps_provider_reported_cost_apart_from_priced_usage() {
    let history = folded(
        &[
            record("2026-08-07T04:01:00Z", |r| r.reported_cost_usd = Some(0.25)),
            record("2026-08-07T04:02:00Z", |r| r.reported_cost_usd = Some(0.5)),
            record("2026-08-07T04:03:00Z", |_| {}),
        ],
        "2026-09-01T00:00:00Z",
    );

    let reported: Vec<_> = history.rows().iter().filter(|row| row.reported).collect();
    let priced: Vec<_> = history.rows().iter().filter(|row| !row.reported).collect();
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].records, 2);
    assert!((reported[0].reported_cost_usd - 0.75).abs() < 1e-12);
    assert_eq!(priced.len(), 1);
    assert_eq!(priced[0].records, 1);
    assert_eq!(priced[0].reported_cost_usd, 0.0);
}

#[test]
fn fold_marks_requests_over_two_hundred_thousand_input_tokens() {
    let history = folded(
        &[
            record("2026-08-07T04:01:00Z", |r| {
                r.totals.uncached_input_tokens = 1_000;
                r.totals.cached_input_tokens = 199_000;
                r.totals.cache_creation_tokens = 0;
            }),
            record("2026-08-07T04:02:00Z", |r| {
                r.totals.uncached_input_tokens = 1_001;
                r.totals.cached_input_tokens = 199_000;
                r.totals.cache_creation_tokens = 0;
            }),
        ],
        "2026-09-01T00:00:00Z",
    );

    let long: Vec<_> = history.rows().iter().map(|row| row.long_context).collect();
    assert_eq!(long, [false, true]);
}

#[test]
fn fold_lists_each_session_of_a_slot_once() {
    let history = folded(
        &[
            record("2026-08-07T04:01:00Z", |r| r.session_id = "b".into()),
            record("2026-08-07T04:02:00Z", |r| r.session_id = "a".into()),
            record("2026-08-07T04:03:00Z", |r| r.session_id = "b".into()),
            record("2026-08-07T04:04:00Z", |r| r.session_id = String::new()),
        ],
        "2026-09-01T00:00:00Z",
    );

    assert_eq!(history.rows().len(), 1);
    assert_eq!(history.rows()[0].sessions, ["a", "b"]);
    assert_eq!(history.rows()[0].records, 4);
}

#[test]
fn fold_splits_rows_by_provider_and_model() {
    let history = folded(
        &[
            record("2026-08-07T04:01:00Z", |_| {}),
            record("2026-08-07T04:02:00Z", |r| r.model = "claude-other".into()),
            record("2026-08-07T04:03:00Z", |r| {
                r.provider = UsageProvider::Codex;
                r.model = "gpt-5.6-sol".into();
            }),
        ],
        "2026-09-01T00:00:00Z",
    );

    let keys: Vec<_> = history
        .rows()
        .iter()
        .map(|row| (row.provider, row.model.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            (UsageProvider::Claude, "claude-fable-5"),
            (UsageProvider::Claude, "claude-other"),
            (UsageProvider::Codex, "gpt-5.6-sol"),
        ]
    );
}

#[test]
fn fold_takes_only_records_between_the_watermark_and_the_cutoff() {
    let early = record("2026-08-01T00:00:00Z", |r| r.totals.output_tokens = 1);
    let middle = record("2026-08-10T00:00:00Z", |r| r.totals.output_tokens = 10);
    let late = record("2026-08-20T00:00:00Z", |r| r.totals.output_tokens = 100);
    let records = [early.clone(), middle.clone(), late.clone()];

    let mut history = UsageHistory::default();
    history.fold(&records, ms("2026-08-05T00:00:00Z"), 0);
    assert_eq!(
        history.folded_through_ms(),
        Some(ms("2026-08-05T00:00:00Z"))
    );
    assert_eq!(history.rows().len(), 1);

    // A later fold sees the already-folded record again and must not count it twice.
    history.fold(&records, ms("2026-08-15T00:00:00Z"), 0);
    assert_eq!(
        history.folded_through_ms(),
        Some(ms("2026-08-15T00:00:00Z"))
    );
    let outputs: Vec<u64> = history
        .rows()
        .iter()
        .map(|row| row.totals.output_tokens)
        .collect();
    assert_eq!(outputs, [1, 10]);
}

#[test]
fn fold_never_moves_the_watermark_back() {
    let mut history = UsageHistory::default();
    history.fold(&[], ms("2026-08-15T00:00:00Z"), 0);
    history.fold(
        &[record("2026-08-10T00:00:00Z", |_| {})],
        ms("2026-08-12T00:00:00Z"),
        0,
    );

    assert_eq!(
        history.folded_through_ms(),
        Some(ms("2026-08-15T00:00:00Z"))
    );
    assert!(history.rows().is_empty());
}

#[test]
fn fold_cutoff_is_the_utc_midnight_seven_days_back() {
    assert_eq!(
        fold_cutoff_ms(ms("2026-09-24T15:30:00Z")),
        ms("2026-09-17T00:00:00Z")
    );
    assert_eq!(
        fold_cutoff_ms(ms("2026-09-24T00:00:00Z")),
        ms("2026-09-17T00:00:00Z")
    );
}

#[test]
fn fold_is_due_once_the_cutoff_passes_the_watermark() {
    let now = ms("2026-09-24T15:30:00Z");
    assert!(fold_due(None, now));
    assert!(fold_due(Some(ms("2026-09-16T00:00:00Z")), now));
    assert!(!fold_due(Some(ms("2026-09-17T00:00:00Z")), now));
}

#[test]
fn history_round_trips_through_its_file() {
    let (root, path) = history_path("usage-history-round-trip");
    let history = folded(
        &[
            record("2026-08-07T04:01:00Z", |r| r.reported_cost_usd = Some(0.25)),
            record("2026-08-07T04:02:00Z", |r| r.session_id = "other".into()),
            record("2026-08-08T09:30:00Z", |r| {
                r.provider = UsageProvider::Codex;
                r.model = "gpt-5.6-sol".into();
                r.totals.cached_input_tokens = 300_000;
            }),
        ],
        "2026-09-01T00:00:00Z",
    );

    persist_history(&path, &history, false).unwrap();

    match load_history(&path) {
        LoadedHistory::Ready {
            history: loaded,
            recovered,
        } => {
            assert!(!recovered);
            assert_eq!(loaded, history);
        }
        other => panic!("expected a readable history, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn no_file_means_nothing_folded_yet() {
    let (root, path) = history_path("usage-history-missing");
    assert!(matches!(load_history(&path), LoadedHistory::Missing));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn persist_keeps_the_previous_file_as_a_backup() {
    let (root, path) = history_path("usage-history-backup");
    let first = folded(
        &[record("2026-08-07T04:01:00Z", |_| {})],
        "2026-08-10T00:00:00Z",
    );
    persist_history(&path, &first, false).unwrap();
    let mut second = first.clone();
    second.fold(
        &[record("2026-08-12T04:01:00Z", |_| {})],
        ms("2026-08-15T00:00:00Z"),
        0,
    );

    persist_history(&path, &second, false).unwrap();

    let backup = std::fs::read_to_string(backup_path(&path)).unwrap();
    assert_eq!(decode(&backup).unwrap(), first);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unreadable_history_falls_back_to_its_backup() {
    let (root, path) = history_path("usage-history-recover");
    let good = folded(
        &[record("2026-08-07T04:01:00Z", |_| {})],
        "2026-08-10T00:00:00Z",
    );
    persist_history(&path, &good, false).unwrap();
    persist_history(&path, &good, false).unwrap();
    std::fs::write(&path, "{ torn").unwrap();

    match load_history(&path) {
        LoadedHistory::Ready { history, recovered } => {
            assert!(recovered);
            assert_eq!(history, good);
        }
        other => panic!("expected the backup, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn persisting_over_an_unreadable_file_sets_it_aside_rather_than_deleting_it() {
    let (root, path) = history_path("usage-history-set-aside");
    std::fs::write(&path, "{ torn").unwrap();
    let history = folded(
        &[record("2026-08-07T04:01:00Z", |_| {})],
        "2026-08-10T00:00:00Z",
    );

    persist_history(&path, &history, true).unwrap();

    let set_aside: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .filter(|body| body == "{ torn")
        .collect();
    assert_eq!(set_aside.len(), 1);
    assert!(matches!(load_history(&path), LoadedHistory::Ready { .. }));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn history_with_no_readable_copy_is_unreadable() {
    let (root, path) = history_path("usage-history-unreadable");
    std::fs::write(&path, "{ torn").unwrap();

    assert!(matches!(load_history(&path), LoadedHistory::Unreadable));
    let _ = std::fs::remove_dir_all(&root);
}

/// A file a newer on-n-off wrote is not this version's to replace, even with a readable backup.
#[test]
fn history_from_a_newer_version_is_left_alone() {
    let (root, path) = history_path("usage-history-newer");
    let good = folded(
        &[record("2026-08-07T04:01:00Z", |_| {})],
        "2026-08-10T00:00:00Z",
    );
    persist_history(&path, &good, false).unwrap();
    persist_history(&path, &good, false).unwrap();
    let newer = serde_json::json!({ "version": USAGE_HISTORY_VERSION + 1, "rows": "elsewhere" });
    std::fs::write(&path, newer.to_string()).unwrap();

    assert!(matches!(load_history(&path), LoadedHistory::Unreadable));
    let _ = std::fs::remove_dir_all(&root);
}

/// A row that points past the model or session table is a damaged file, not a row to skip:
/// dropping it and writing the rest back would lose that usage for good.
#[test]
fn a_row_with_a_dangling_reference_makes_the_whole_file_unreadable() {
    let history = folded(
        &[record("2026-08-07T04:01:00Z", |_| {})],
        "2026-08-10T00:00:00Z",
    );
    let mut document: serde_json::Value = serde_json::from_str(&encode(&history)).unwrap();
    document["models"] = serde_json::json!([]);

    assert!(decode(&document.to_string()).is_err());
}

#[test]
fn clear_removes_the_history_and_every_copy_of_it() {
    let (root, path) = history_path("usage-history-clear");
    let history = folded(
        &[record("2026-08-07T04:01:00Z", |_| {})],
        "2026-08-10T00:00:00Z",
    );
    std::fs::write(&path, "{ torn").unwrap();
    persist_history(&path, &history, true).unwrap();
    persist_history(&path, &history, false).unwrap();
    std::fs::write(root.join("unrelated.json"), "{}").unwrap();

    clear_history(&path).unwrap();

    let left: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["unrelated.json"]);
    assert!(matches!(load_history(&path), LoadedHistory::Missing));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn kept_since_is_the_first_folded_slot() {
    let history = folded(
        &[
            record("2026-08-09T04:01:00Z", |_| {}),
            record("2026-08-07T23:59:00Z", |_| {}),
        ],
        "2026-08-10T00:00:00Z",
    );

    assert_eq!(history.kept_since_ms(), Some(ms("2026-08-07T23:45:00Z")));
    assert_eq!(UsageHistory::default().kept_since_ms(), None);
}
