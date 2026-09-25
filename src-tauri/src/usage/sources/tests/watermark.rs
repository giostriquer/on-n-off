//! The watermark: a transcript whose records the usage history holds is not read again, and its
//! parse leaves the scan cache.

use super::*;

fn at(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

fn watermark() -> Watermark {
    Watermark::at(at("2026-08-14T00:00:00Z"))
}

/// A transcript of one record on 2026-08-01, last written that day: 36 hours and more before the
/// watermark, so everything it holds is folded.
fn write_long_folded(home: &Path) -> PathBuf {
    let path = transcript_path(home, "long-folded.jsonl");
    write_records(&path, &[record("2026-08-01T04:05:13.944Z", "msg-1", 20)]);
    set_mtime_ms(&path, at("2026-08-01T05:00:00Z"));
    path
}

#[test]
fn a_transcript_last_written_well_before_the_watermark_is_indexed_unread() {
    let home = scratch_dir("usage-sources-indexed-unread");
    let path = write_long_folded(&home);
    reset_transcript_parse_count();

    let sources = open(&home, watermark());
    let complete = sources.is_complete();
    sources.finish(watermark);

    assert!(complete);
    assert_eq!(transcript_parse_count(), 0);
    assert_eq!(cached_record_count(&home, &path), None);
    let _ = std::fs::remove_dir_all(home);
}

/// Indexing records the same span for a transcript whose records the history holds whether or not
/// the scan cache still holds its parse, so a summary's signature does not change with the cache.
#[test]
fn a_transcript_last_written_well_before_the_watermark_signs_the_same_parsed_or_not() {
    let home = scratch_dir("usage-sources-folded-signature");
    write_long_folded(&home);
    open_and_finish(&home, Watermark::NONE);
    let index = source_index_path_for(&home);
    std::fs::remove_file(&index).unwrap();

    let with_parse = signature(&home, watermark(), AUGUST_START, SEPTEMBER_START);
    std::fs::remove_file(&index).unwrap();
    let _ = std::fs::remove_file(scan_cache_path_for(&home));
    let without_parse = signature(&home, watermark(), AUGUST_START, SEPTEMBER_START);

    assert_eq!(with_parse, without_parse);
    let _ = std::fs::remove_dir_all(home);
}

/// Written recently, so its mtime alone would have it read; its newest record is folded.
#[test]
fn a_transcript_whose_newest_record_is_folded_is_not_read_however_recently_written() {
    let home = scratch_dir("usage-sources-read-skips-folded");
    let path = transcript_path(&home, "recent.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    open_and_finish(&home, Watermark::NONE);
    std::fs::remove_file(scan_cache_path_for(&home)).unwrap();
    reset_transcript_parse_count();

    let mut sources = open(&home, watermark());
    let read = sources.read(i64::MIN, watermark);
    sources.finish(watermark);

    assert_eq!(transcript_parse_count(), 0);
    assert!(read.records().is_empty());
    assert_eq!(
        read.files_read(UsageProvider::Claude),
        FilesRead {
            scanned: 0,
            skipped: 0
        }
    );
    assert!(read.complete);
    let _ = std::fs::remove_dir_all(home);
}

/// The fold opens the sources with the watermark it found and finishes them with the one it moved
/// to: a transcript it folded leaves the scan cache then, and one with records after it stays.
#[test]
fn finishing_with_a_later_watermark_drops_the_parses_it_folded() {
    let home = scratch_dir("usage-sources-prune-folded");
    let folded = transcript_path(&home, "folded.jsonl");
    let kept = transcript_path(&home, "kept.jsonl");
    write_records(&folded, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    write_records(&kept, &[record("2026-08-20T04:05:13.944Z", "msg-2", 30)]);
    open_and_finish(&home, Watermark::NONE);

    let mut sources = open(&home, Watermark::NONE);
    sources.read(i64::MIN, || Watermark::NONE);
    sources.finish(watermark);

    assert_eq!(cached_record_count(&home, &folded), None);
    assert_eq!(cached_record_count(&home, &kept), Some(1));
    let _ = std::fs::remove_dir_all(home);
}

/// A message's copies collapse to the richest before the watermark splits them. Here the billed
/// copy is folded and a partial copy in a resumed session sits after the watermark: the message
/// is the history's, and the partial copy is not counted on top of it.
#[test]
fn a_read_counts_each_message_once_from_the_watermark_on() {
    let home = scratch_dir("usage-sources-records-from-watermark");
    let first = transcript_path(&home, "first.jsonl");
    let resumed = transcript_path(&home, "resumed.jsonl");
    write_records(
        &first,
        &[
            record("2026-08-13T23:59:59.999Z", "msg-1", 50),
            record("2026-08-14T01:00:00.000Z", "msg-2", 9),
        ],
    );
    write_records(
        &resumed,
        &[
            record("2026-08-14T00:00:00.001Z", "msg-1", 1),
            record("2026-08-14T02:00:00.000Z", "msg-3", 4),
        ],
    );

    let mut sources = open(&home, watermark());
    let read = sources.read(i64::MIN, watermark);
    sources.finish(watermark);

    let outputs: Vec<u64> = read
        .records()
        .iter()
        .map(|record| record.totals.output_tokens)
        .collect();
    assert_eq!(outputs, [9, 4]);
    let _ = std::fs::remove_dir_all(home);
}

/// A read uses the watermark the index was brought up to date with, not a second one: the summary
/// opens the history once.
#[test]
fn a_read_after_indexing_uses_the_watermark_the_index_was_brought_up_to_date_with() {
    let home = scratch_dir("usage-sources-read-keeps-watermark");
    write_long_folded(&home);
    write_records(
        &transcript_path(&home, "recent.jsonl"),
        &[record("2026-08-20T04:05:13.944Z", "msg-2", 30)],
    );

    let mut sources = open(&home, watermark());
    let read = sources.read(i64::MIN, || Watermark::NONE);
    sources.finish(watermark);

    let outputs: Vec<u64> = read
        .records()
        .iter()
        .map(|record| record.totals.output_tokens)
        .collect();
    assert_eq!(outputs, [30]);
    let _ = std::fs::remove_dir_all(home);
}
