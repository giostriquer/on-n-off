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

/// What opening the file finds: the history and whether it came from the backup, or `None` when
/// nothing reads.
fn opened(path: &Path) -> Option<(UsageHistory, bool)> {
    match HistoryStore::open(path.to_path_buf()).state {
        StoreState::Readable { history, recovered } => Some((history, recovered)),
        StoreState::Unreadable => None,
    }
}

fn one_fold() -> UsageHistory {
    folded(
        &[record("2026-08-07T04:01:00Z", |_| {})],
        "2026-08-10T00:00:00Z",
    )
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
    assert_eq!(history.watermark().ms(), Some(ms("2026-08-05T00:00:00Z")));
    assert_eq!(history.rows().len(), 1);

    // A later fold sees the already-folded record again and must not count it twice.
    history.fold(&records, ms("2026-08-15T00:00:00Z"), 0);
    assert_eq!(history.watermark().ms(), Some(ms("2026-08-15T00:00:00Z")));
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

    assert_eq!(history.watermark().ms(), Some(ms("2026-08-15T00:00:00Z")));
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
    assert!(fold_due(Watermark::NONE, now));
    assert!(fold_due(Watermark::at(ms("2026-09-16T00:00:00Z")), now));
    assert!(!fold_due(Watermark::at(ms("2026-09-17T00:00:00Z")), now));
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

    save(&path, &history, false).unwrap();

    assert_eq!(opened(&path), Some((history, false)));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn no_file_means_nothing_folded_yet() {
    let (root, path) = history_path("usage-history-missing");
    let store = HistoryStore::open(path);

    assert_eq!(store.history(), Some(&UsageHistory::default()));
    assert_eq!(store.watermark(), Watermark::NONE);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn save_keeps_the_previous_file_as_a_backup() {
    let (root, path) = history_path("usage-history-backup");
    let first = one_fold();
    save(&path, &first, false).unwrap();
    let mut second = first.clone();
    second.fold(
        &[record("2026-08-12T04:01:00Z", |_| {})],
        ms("2026-08-15T00:00:00Z"),
        0,
    );

    save(&path, &second, false).unwrap();

    let backup = std::fs::read_to_string(backup_path(&path)).unwrap();
    assert_eq!(decode(&backup).unwrap(), first);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unreadable_history_falls_back_to_its_backup() {
    let (root, path) = history_path("usage-history-recover");
    let good = one_fold();
    save(&path, &good, false).unwrap();
    save(&path, &good, false).unwrap();
    std::fs::write(&path, "{ torn").unwrap();

    assert_eq!(opened(&path), Some((good, true)));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn saving_over_an_unreadable_file_copies_it_aside_rather_than_deleting_it() {
    let (root, path) = history_path("usage-history-set-aside");
    std::fs::write(&path, "{ torn").unwrap();
    let history = one_fold();

    save(&path, &history, true).unwrap();

    let set_aside: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .filter(|body| body == "{ torn")
        .collect();
    assert_eq!(set_aside.len(), 1);
    assert_eq!(opened(&path), Some((history, false)));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn history_with_no_readable_copy_is_unreadable() {
    let (root, path) = history_path("usage-history-unreadable");
    std::fs::write(&path, "{ torn").unwrap();

    assert_eq!(opened(&path), None);
    assert_eq!(HistoryStore::open(path).watermark(), Watermark::NONE);
    let _ = std::fs::remove_dir_all(&root);
}

/// A file a newer on-n-off wrote is not this version's to replace, even with a readable backup.
#[test]
fn history_from_a_newer_version_is_left_alone() {
    let (root, path) = history_path("usage-history-newer");
    let good = one_fold();
    save(&path, &good, false).unwrap();
    save(&path, &good, false).unwrap();
    let newer = serde_json::json!({ "version": USAGE_HISTORY_VERSION + 1, "rows": "elsewhere" });
    std::fs::write(&path, newer.to_string()).unwrap();

    let mut store = HistoryStore::open(path.clone());

    assert_eq!(store.history(), None);
    assert!(store.fold(&[], ms("2026-09-01T00:00:00Z"), 0).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), newer.to_string());
    let _ = std::fs::remove_dir_all(&root);
}

/// A row that points past the model or session table is a damaged file, not a row to skip:
/// dropping it and writing the rest back would lose that usage for good.
#[test]
fn a_row_with_a_dangling_reference_makes_the_whole_file_unreadable() {
    let mut document: serde_json::Value = serde_json::from_str(&encode(&one_fold())).unwrap();
    document["models"] = serde_json::json!([]);

    assert!(decode(&document.to_string()).is_err());
}

/// Rows at or past the watermark, or rows with no watermark at all, would be counted a second
/// time from the transcripts; rows out of slot order would be missed by a window's slice.
#[test]
fn rows_a_read_would_miscount_make_the_file_unreadable() {
    let history = folded(
        &[
            record("2026-08-07T04:01:00Z", |_| {}),
            record("2026-08-08T04:01:00Z", |_| {}),
        ],
        "2026-08-10T00:00:00Z",
    );
    let document: serde_json::Value = serde_json::from_str(&encode(&history)).unwrap();
    assert!(decode(&document.to_string()).is_ok());

    let mut at_the_watermark = document.clone();
    at_the_watermark["foldedThroughMs"] = serde_json::json!(ms("2026-08-08T04:00:00Z"));
    let mut no_watermark = document.clone();
    no_watermark["foldedThroughMs"] = serde_json::Value::Null;
    let mut out_of_order = document.clone();
    let rows = out_of_order["rows"].as_array_mut().unwrap();
    rows.swap(0, 1);

    for damaged in [at_the_watermark, no_watermark, out_of_order] {
        assert!(decode(&damaged.to_string()).is_err(), "{damaged}");
    }
}

/// The history is written only once it is known to read back as itself; a fold that would not is
/// refused with the file and the store left as they were.
#[test]
fn a_fold_that_would_not_read_back_is_not_saved() {
    let (root, path) = history_path("usage-history-no-read-back");
    let mut store = HistoryStore::open(path.clone());
    store
        .fold(
            &[record("2026-08-07T04:01:00Z", |_| {})],
            ms("2026-08-10T00:00:00Z"),
            0,
        )
        .unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    // More one-hour cache writes than cache writes: a row the file refuses to read.
    let impossible = record("2026-08-12T04:01:00Z", |r| {
        r.totals.cache_creation_tokens = 5;
        r.totals.cache_creation_1h_tokens = 10;
    });

    let refused = store.fold(
        std::slice::from_ref(&impossible),
        ms("2026-08-15T00:00:00Z"),
        0,
    );

    assert!(refused.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    assert_eq!(store.watermark(), Watermark::at(ms("2026-08-10T00:00:00Z")));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn clear_removes_the_history_and_every_copy_of_it() {
    let (root, path) = history_path("usage-history-clear");
    let history = one_fold();
    std::fs::write(&path, "{ torn").unwrap();
    save(&path, &history, true).unwrap();
    save(&path, &history, false).unwrap();
    std::fs::write(root.join("unrelated.json"), "{}").unwrap();

    clear_history(&path).unwrap();

    let left: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["unrelated.json"]);
    assert_eq!(opened(&path), Some((UsageHistory::default(), false)));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_room_taken_counts_the_file_and_every_copy_of_it() {
    let (root, path) = history_path("usage-history-bytes");
    assert_eq!(history_bytes(&path), 0);
    std::fs::write(&path, "{ torn").unwrap();
    save(&path, &one_fold(), true).unwrap();
    save(&path, &one_fold(), false).unwrap();
    std::fs::write(root.join("unrelated.json"), "{}").unwrap();

    let expected: u64 = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_name() != "unrelated.json")
        .map(|entry| entry.metadata().unwrap().len())
        .sum();
    assert_eq!(history_bytes(&path), expected);
    assert!(expected > std::fs::metadata(&path).unwrap().len());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_fingerprint_changes_when_the_history_is_written_or_cleared() {
    let (root, path) = history_path("usage-history-fingerprint");
    let none = history_fingerprint(&path);
    save(&path, &one_fold(), false).unwrap();
    let first = history_fingerprint(&path);
    let mut longer = one_fold();
    longer.fold(
        &[record("2026-08-12T04:01:00Z", |_| {})],
        ms("2026-08-15T00:00:00Z"),
        0,
    );
    save(&path, &longer, false).unwrap();
    let second = history_fingerprint(&path);
    clear_history(&path).unwrap();

    assert_ne!(none, first);
    assert_ne!(first, second);
    assert_eq!(history_fingerprint(&path), none);
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

#[test]
fn rows_between_takes_the_slots_starting_inside_the_window() {
    let history = folded(
        &[
            record("2026-08-06T23:59:00Z", |_| {}),
            record("2026-08-07T00:00:00Z", |_| {}),
            record("2026-08-07T23:59:00Z", |_| {}),
            record("2026-08-08T00:00:00Z", |_| {}),
        ],
        "2026-08-10T00:00:00Z",
    );

    let slots: Vec<i64> = history
        .rows_between(ms("2026-08-07T00:00:00Z"), ms("2026-08-08T00:00:00Z"))
        .iter()
        .map(|row| row.slot_start_ms)
        .collect();
    assert_eq!(
        slots,
        [ms("2026-08-07T00:00:00Z"), ms("2026-08-07T23:45:00Z")]
    );
    assert!(history
        .rows_between(ms("2026-08-09T00:00:00Z"), ms("2026-08-01T00:00:00Z"))
        .is_empty());
}

#[test]
fn the_watermark_marks_what_the_history_already_holds() {
    let watermark = Watermark::at(ms("2026-08-10T00:00:00Z"));
    assert!(watermark.is_folded(ms("2026-08-09T23:59:59Z")));
    assert!(!watermark.is_folded(ms("2026-08-10T00:00:00Z")));
    // A transcript is done once its newest record is folded, or once it was last written more
    // than the 36-hour slack before the watermark.
    assert!(
        watermark.holds_only_folded(ms("2026-08-12T00:00:00Z"), Some(ms("2026-08-09T00:00:00Z")))
    );
    assert!(
        !watermark.holds_only_folded(ms("2026-08-12T00:00:00Z"), Some(ms("2026-08-10T00:00:00Z")))
    );
    assert!(watermark.holds_only_folded(ms("2026-08-08T11:59:59Z"), None));
    assert!(!watermark.holds_only_folded(ms("2026-08-08T12:00:00Z"), None));
    // Before the first fold nothing is folded.
    assert!(!Watermark::NONE.is_folded(i64::MIN + 1));
    assert!(!Watermark::NONE.holds_only_folded(i64::MIN, Some(i64::MIN)));
}

/// Recovering from the backup must survive a write that fails: the damaged file stays where it
/// is until the new one replaces it, so the next load falls back to the backup again rather than
/// finding no history at all and starting over.
#[test]
fn a_failed_write_while_recovering_keeps_the_backup_in_reach() {
    let (root, path) = history_path("usage-history-recover-write-fails");
    let good = one_fold();
    save(&path, &good, false).unwrap();
    save(&path, &good, false).unwrap();
    std::fs::write(&path, "{ torn").unwrap();
    let mut next = opened(&path).expect("the backup").0;
    next.fold(
        &[record("2026-08-12T04:01:00Z", |_| {})],
        ms("2026-08-15T00:00:00Z"),
        0,
    );

    let failed = save_with(&path, &next, true, |_, _| {
        Err(io::Error::other("disk full"))
    });

    assert!(failed.is_err());
    assert_eq!(opened(&path), Some((good, true)));
    let _ = std::fs::remove_dir_all(&root);
}
