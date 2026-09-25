//! What a read counts when a transcript moves or fails under it, and when a cached parse stands in
//! for reading it.

use super::*;

fn output_tokens(read: &SourceRead) -> Vec<u64> {
    read.records()
        .iter()
        .map(|record| record.totals.output_tokens)
        .collect()
}

/// Indexes and caches the transcripts as they are, then forgets their cached parses, so a read of
/// the unchanged index has to read each transcript itself.
fn index_then_forget_parse(home: &Path) {
    open_and_finish(home, Watermark::NONE);
    std::fs::remove_file(scan_cache_path_for(home)).unwrap();
}

/// A parse of a file that never held still neither resolves its index entry nor enters the scan
/// cache, so the sources are not complete and the next open reads it again.
#[test]
fn a_live_transcript_is_left_pending_uncached_and_indexed_again_next_time() {
    let home = scratch_dir("usage-sources-live-open");
    let path = transcript_path(&home, "session.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    let growth = record("2026-08-07T04:06:00.000Z", "msg-live", 5);

    let pending = with_live_transcript(&path, &growth, || {
        let sources = open(&home, Watermark::NONE);
        let complete = sources.is_complete();
        sources.finish(|| Watermark::NONE);
        complete
    });
    let pending_cached = cached_record_count(&home, &path);
    let settled = open(&home, Watermark::NONE);
    let settled_complete = settled.is_complete();
    settled.finish(|| Watermark::NONE);

    assert!(!pending);
    assert_eq!(pending_cached, None);
    assert!(settled_complete);
    // msg-1 and msg-live: the lines the live session appended are all copies of msg-live.
    assert_eq!(cached_record_count(&home, &path), Some(2));
    let _ = std::fs::remove_dir_all(home);
}

/// A transcript that changed since the walk, or is still being written while it is read (a live
/// session), counts what it holds; that parse is never cached, and the read is not final.
#[test]
fn a_transcript_changed_or_live_since_the_walk_counts_what_it_holds_uncached_and_not_final() {
    for live in [false, true] {
        let home = scratch_dir(if live {
            "usage-sources-live-read"
        } else {
            "usage-sources-changed-read"
        });
        let path = transcript_path(&home, "session.jsonl");
        write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
        index_then_forget_parse(&home);
        let growth = record("2026-08-07T04:06:00.000Z", "msg-live", 5);

        let mut sources = open(&home, Watermark::NONE);
        append_record(&path, &record("2026-08-07T04:05:30.000Z", "msg-2", 30));
        let read = if live {
            with_live_transcript(&path, &growth, || {
                sources.read(i64::MIN, || Watermark::NONE)
            })
        } else {
            sources.read(i64::MIN, || Watermark::NONE)
        };
        sources.finish(|| Watermark::NONE);

        let expected: &[u64] = if live { &[20, 30, 5] } else { &[20, 30] };
        assert_eq!(output_tokens(&read), expected, "live: {live}");
        assert!(!read.complete, "live: {live}");
        assert_eq!(cached_record_count(&home, &path), None, "live: {live}");
        let _ = std::fs::remove_dir_all(home);
    }
}

#[test]
fn a_same_size_rewrite_since_the_walk_counts_uncached_and_not_final() {
    let home = scratch_dir("usage-sources-rewritten-read");
    let path = transcript_path(&home, "session.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    index_then_forget_parse(&home);

    let mut sources = open(&home, Watermark::NONE);
    rewrite_same_size_later(&path, &record("2026-08-07T04:05:13.944Z", "msg-1", 21));
    let read = sources.read(i64::MIN, || Watermark::NONE);
    sources.finish(|| Watermark::NONE);

    assert_eq!(output_tokens(&read), [21]);
    assert!(!read.complete);
    assert_eq!(cached_record_count(&home, &path), None);
    let _ = std::fs::remove_dir_all(home);
}

/// A transcript that cannot be read when the records are (removed, locked) counts its last cached
/// parse, even one older than the index, for that read only, and that is never final. Here the
/// scan cache is behind the index because its last save failed.
#[test]
fn an_unreadable_transcript_counts_its_last_cached_parse_and_is_not_final() {
    let home = scratch_dir("usage-sources-unreadable");
    let path = transcript_path(&home, "session.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    open_and_finish(&home, Watermark::NONE);
    let stale = std::fs::read(scan_cache_path_for(&home)).unwrap();
    append_record(&path, &record("2026-08-07T04:05:30.000Z", "msg-2", 30));
    let mut appended = open(&home, Watermark::NONE);
    appended.read(i64::MIN, || Watermark::NONE);
    appended.finish(|| Watermark::NONE);
    std::fs::write(scan_cache_path_for(&home), stale).unwrap();

    let mut sources = open(&home, Watermark::NONE);
    std::fs::remove_file(&path).unwrap();
    let read = sources.read(i64::MIN, || Watermark::NONE);
    sources.finish(|| Watermark::NONE);

    assert_eq!(output_tokens(&read), [20]);
    assert!(!read.complete);
    assert_eq!(
        cached_record_count(&home, &path),
        Some(1),
        "kept, not replaced"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn an_unchanged_transcript_missing_from_the_scan_cache_is_read_cached_and_final() {
    let home = scratch_dir("usage-sources-cache-miss");
    let path = transcript_path(&home, "session.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    index_then_forget_parse(&home);
    reset_transcript_parse_count();

    let mut sources = open(&home, Watermark::NONE);
    let read = sources.read(i64::MIN, || Watermark::NONE);
    sources.finish(|| Watermark::NONE);

    assert_eq!(output_tokens(&read), [20]);
    assert!(read.complete);
    assert_eq!(transcript_parse_count(), 1);
    assert_eq!(cached_record_count(&home, &path), Some(1));
    let _ = std::fs::remove_dir_all(home);
}

/// The scan cache holds a parse of the same size as the transcript but an older mtime (its last
/// save failed after a same-size rewrite): the transcript is read again, not the stale parse.
#[test]
fn a_cached_parse_serves_only_while_both_size_and_mtime_match() {
    let home = scratch_dir("usage-sources-cache-hit");
    let path = transcript_path(&home, "session.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    open_and_finish(&home, Watermark::NONE);
    let stale = std::fs::read(scan_cache_path_for(&home)).unwrap();
    rewrite_same_size_later(&path, &record("2026-08-07T04:05:13.944Z", "msg-1", 21));
    open_and_finish(&home, Watermark::NONE);
    std::fs::write(scan_cache_path_for(&home), stale).unwrap();

    let mut sources = open(&home, Watermark::NONE);
    let read = sources.read(i64::MIN, || Watermark::NONE);
    sources.finish(|| Watermark::NONE);

    assert_eq!(output_tokens(&read), [21], "not the stale parse");
    assert!(read.complete);
    let _ = std::fs::remove_dir_all(home);
}

/// A read from an instant reads every transcript last written up to 36 hours before it, the
/// allowance for local days that begin before UTC midnight and clocks that disagree, and none
/// written earlier.
#[test]
fn a_read_from_an_instant_reads_transcripts_written_up_to_36_hours_before_it() {
    let home = scratch_dir("usage-sources-read-slack");
    let from_ms = AUGUST_START;
    let edge = transcript_path(&home, "edge.jsonl");
    let earlier = transcript_path(&home, "earlier.jsonl");
    write_records(&edge, &[record("2026-08-01T01:00:00.000Z", "msg-edge", 20)]);
    write_records(
        &earlier,
        &[record("2026-08-01T02:00:00.000Z", "msg-earlier", 30)],
    );
    set_mtime_ms(&edge, from_ms - 36 * 60 * 60 * 1000);
    set_mtime_ms(&earlier, from_ms - 36 * 60 * 60 * 1000 - 1);

    let mut sources = open(&home, Watermark::NONE);
    let read = sources.read(from_ms, || Watermark::NONE);
    sources.finish(|| Watermark::NONE);

    assert_eq!(output_tokens(&read), [20]);
    let _ = std::fs::remove_dir_all(home);
}
