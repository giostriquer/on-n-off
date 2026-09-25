//! What needs the index's own seams: a walk that could not finish, which no directory produces
//! on every platform, and how one parse attempt reads a file that moves. Everything else about
//! the index is tested through `Sources` (`sources/tests`).

use super::super::test_support::{record, transcript_path, write_records};
use super::*;
use crate::paths::scratch_dir;
use crate::usage::transcripts::parse_claude_line;

fn roots(home: &Path) -> Vec<SourceRoot> {
    vec![SourceRoot {
        provider: UsageProvider::Claude,
        path: home.join(".claude").join("projects"),
    }]
}

#[test]
fn incomplete_root_walk_retains_previous_entries_and_disables_hits() {
    let home = scratch_dir("usage-source-index-failed-walk");
    write_records(
        &transcript_path(&home, "retained.jsonl"),
        &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
    );
    let index = source_index_path_for(&home);
    let mut cache = ScanCache::new();
    let first = reconcile_inventory(
        &index,
        inventory_sources(&roots(&home)),
        &mut cache,
        Watermark::NONE,
    );
    let generation = first.snapshot.generation();

    let root = roots(&home).remove(0);
    let failed = reconcile_inventories(
        &index,
        vec![(
            root,
            RootInventory {
                files: Vec::new(),
                present: true,
                complete: false,
            },
        )],
        &mut cache,
        Watermark::NONE,
    );
    assert!(!failed.snapshot.is_complete());
    assert_eq!(failed.snapshot.generation(), generation);
    assert_eq!(failed.snapshot.entries.len(), 1);
    let _ = std::fs::remove_dir_all(home);
}

fn parsed(output_tokens: u64) -> UsageRecord {
    parse_claude_line(&record("2026-08-07T04:05:13.944Z", "msg-1", output_tokens)).unwrap()
}

fn outputs_of(read: TranscriptRead) -> Vec<u64> {
    match read {
        TranscriptRead::Moving(records) => records
            .iter()
            .map(|record| record.totals.output_tokens)
            .collect(),
        other => panic!("expected Moving, got {other:?}"),
    }
}

#[test]
fn a_file_that_changes_during_both_parse_attempts_reads_as_moving_with_its_last_parse() {
    let identities = std::cell::RefCell::new(vec![(2, 2), (3, 3)].into_iter());
    let attempts = std::cell::Cell::new(0);
    let read = read_stable_records_with(
        1,
        1,
        || {
            attempts.set(attempts.get() + 1);
            Some(vec![parsed(attempts.get())])
        },
        || identities.borrow_mut().next(),
    );
    assert_eq!(
        outputs_of(read),
        [2],
        "the last attempt is what it held last"
    );
}

/// Growth with an unchanged mtime is still movement: filesystems with coarse timestamps see an
/// append only through the size.
#[test]
fn a_size_change_alone_reads_as_moving() {
    let identities = std::cell::RefCell::new(vec![(2, 1), (3, 1)].into_iter());
    let read = read_stable_records_with(
        1,
        1,
        || Some(vec![parsed(20)]),
        || identities.borrow_mut().next(),
    );
    assert_eq!(outputs_of(read), [20]);
}

#[test]
fn a_file_gone_after_a_parse_keeps_that_parse() {
    let read = read_stable_records_with(1, 1, || Some(vec![parsed(20)]), || None);
    assert_eq!(outputs_of(read), [20]);
}

#[test]
fn a_failed_second_attempt_keeps_the_first_parse() {
    let attempts = std::cell::Cell::new(0);
    let read = read_stable_records_with(
        1,
        1,
        || {
            attempts.set(attempts.get() + 1);
            (attempts.get() == 1).then(|| vec![parsed(20)])
        },
        || Some((2, 2)),
    );
    assert_eq!(outputs_of(read), [20]);
}

#[test]
fn a_transcript_that_cannot_be_read_reads_as_failed() {
    let read = read_stable_records_with(1, 1, || None, || Some((1, 1)));
    assert!(matches!(read, TranscriptRead::Failed));
}
