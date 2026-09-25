//! Usage kept after a transcript is deleted: what a fold keeps, the watermark between folded rows
//! and live transcripts, what holds a fold back, and a history that does not read.

use super::super::test_support::*;
use super::super::*;
use crate::dto::UsageHistoryState;
use crate::paths::scratch_dir;
use crate::usage::folding::{clear_history_in, fold_history_in, history_status_in, FoldChecks};
use crate::usage::history::{history_path_for, FoldedRow, HistoryStore, Watermark};
use crate::usage::pricing;
use crate::usage::source_index::normalize_path;
use crate::usage::source_index::{
    reset_transcript_parse_count, source_index_path_for, transcript_parse_count,
    with_live_transcript,
};
use crate::usage::sources::{load_scan_cache, scan_cache_path_for};
use crate::usage::summary_cache::summary_cache_path_for;

/// Two weeks after the fixtures' 2026-08-07: the fold cutoff is 2026-08-14, so August's first week
/// is folded and a record from 2026-08-15 on stays in its transcript.
const AFTER_AUGUST_FIRST_WEEK: &str = "2026-08-21T12:00:00Z";

/// One background check, as the first after launch: no memory of earlier checks.
fn fold(home: &Path) {
    fold_with(home, &mut FoldChecks::default());
}

/// One background check that remembers the checks before it.
fn fold_with(home: &Path, checks: &mut FoldChecks) {
    fold_history_in(home, at(AFTER_AUGUST_FIRST_WEEK), checks).unwrap();
}

fn folded_outputs(home: &Path) -> Vec<u64> {
    let store = HistoryStore::open(history_path_for(home));
    let history = store.history().expect("a readable history");
    history
        .rows()
        .iter()
        .map(|row: &FoldedRow| row.totals.output_tokens)
        .collect()
}

fn watermark(home: &Path) -> Watermark {
    HistoryStore::open(history_path_for(home)).watermark()
}

fn claude_line(message_id: &str, timestamp: &str, output_tokens: u64) -> String {
    claude_usage_line(
        message_id,
        "sess-a",
        timestamp,
        serde_json::json!({ "input_tokens": 1, "output_tokens": output_tokens }),
    )
}

/// One Claude record, its transcript last written when the record was, as an agent leaves it.
fn write_record(home: &Path, name: &str, timestamp: &str, output_tokens: u64) -> PathBuf {
    let path = write_single_claude_record(home, name, timestamp, output_tokens);
    set_mtime(&path, timestamp);
    path
}

/// A transcript of `lines`, last written at `written`.
fn write_lines(home: &Path, name: &str, lines: &[String], written: &str) -> PathBuf {
    write_claude_lines(home, name, lines);
    let path = home.join(".claude/projects/proj").join(name);
    set_mtime(&path, written);
    path
}

fn scan_cache_holds(home: &Path, path: &Path) -> bool {
    load_scan_cache(&scan_cache_path_for(home)).contains_key(&normalize_path(path))
}

#[test]
fn a_deleted_transcript_still_counts_once_its_usage_is_folded() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-deleted");
    let path = write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);

    fold(&home);
    std::fs::remove_file(&path).unwrap();
    let after = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&after), 20);
    assert_eq!(record_count(&after), 1);
    let claude = after
        .sources
        .iter()
        .find(|source| source.provider == AgentId::Claude)
        .unwrap();
    assert_eq!(claude.distinct_sessions, 1);
    let _ = std::fs::remove_dir_all(&home);
}

/// Reading is not folding: until a fold runs, a deleted transcript's usage is gone from the read.
#[test]
fn a_read_never_folds() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-read-only");
    let path = write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);

    read_offline(&home, august_input(false));
    std::fs::remove_file(&path).unwrap();
    let after = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&after), 0);
    assert!(!history_path_for(&home).exists());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn nothing_is_folded_before_it_is_a_week_old() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-too-recent");
    let path = write_record(&home, "a.jsonl", "2026-08-15T04:05:13.944Z", 20);

    fold(&home);
    std::fs::remove_file(&path).unwrap();
    let after = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&after), 0);
    assert!(folded_outputs(&home).is_empty());
    assert_eq!(watermark(&home).ms(), Some(at("2026-08-14T00:00:00Z")));
    let _ = std::fs::remove_dir_all(&home);
}

/// A resumed session copies earlier messages into its new transcript under their original
/// timestamps. Once the original is folded, the copy is below the watermark and not counted again.
#[test]
fn a_live_copy_of_a_folded_record_is_not_counted_twice() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-copy");
    let original = claude_line("msg_1", "2026-08-07T04:05:13.944Z", 20);
    let later = claude_line("msg_2", "2026-08-20T04:05:13.944Z", 300);
    write_lines(
        &home,
        "original.jsonl",
        std::slice::from_ref(&original),
        "2026-08-07T04:05:14Z",
    );
    fold(&home);

    write_lines(
        &home,
        "resumed.jsonl",
        &[original, later],
        "2026-08-20T04:05:14Z",
    );
    let with_both = read_offline(&home, august_input(false));
    std::fs::remove_file(home.join(".claude/projects/proj/original.jsonl")).unwrap();
    let with_copy_only = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&with_both), 320);
    assert_eq!(output_tokens(&with_copy_only), 320);
    assert_eq!(record_count(&with_copy_only), 2);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn records_after_the_watermark_are_read_from_their_transcript() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-live-side");
    let first = claude_line("msg_1", "2026-08-07T04:05:13.944Z", 20);
    let recent = claude_line("msg_2", "2026-08-18T04:05:13.944Z", 300);
    write_lines(
        &home,
        "a.jsonl",
        &[first.clone(), recent.clone()],
        "2026-08-18T04:05:14Z",
    );
    fold(&home);
    let folded = read_offline(&home, august_input(false));

    let appended = claude_line("msg_3", "2026-08-20T04:05:13.944Z", 4000);
    write_lines(
        &home,
        "a.jsonl",
        &[first, recent, appended],
        "2026-08-20T04:05:14Z",
    );
    let grown = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&folded), 320);
    assert_eq!(output_tokens(&grown), 4320);
    assert_eq!(folded_outputs(&home), [20]);
    let _ = std::fs::remove_dir_all(&home);
}

/// An agent is almost always writing some transcript. Its records older than the cutoff were
/// written days ago, so what it holds is enough to fold them.
#[test]
fn a_transcript_still_being_written_does_not_hold_the_fold_back() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-live-writer");
    write_claude_lines(
        &home,
        "a.jsonl",
        &[claude_line("msg_1", "2026-08-07T04:05:13.944Z", 20)],
    );
    let path = home.join(".claude/projects/proj/a.jsonl");
    let still_writing = claude_line("msg_2", "2026-08-21T11:00:00.000Z", 1);

    with_live_transcript(&path, &still_writing, || fold(&home));

    assert_eq!(folded_outputs(&home), [20]);
    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn folding_waits_until_every_directory_can_be_walked() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-locked-dir");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = home.join(".claude/projects/locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    fold(&home);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(watermark(&home), Watermark::NONE);
    let _ = std::fs::remove_dir_all(&home);
}

/// A transcript that cannot be read may just be mid-write: while it was written recently, the
/// fold waits for it, however many checks it fails.
#[cfg(unix)]
#[test]
fn folding_waits_for_a_recent_transcript_that_cannot_be_read() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-locked-file");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = write_record(&home, "b.jsonl", "2026-08-08T04:05:13.944Z", 300);
    set_mtime(&locked, "2026-08-08T04:05:14Z");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let mut checks = FoldChecks::default();

    fold_with(&home, &mut checks);
    fold_with(&home, &mut checks);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(watermark(&home), Watermark::NONE);
    let _ = std::fs::remove_dir_all(&home);
}

/// One a week past the cutoff that failed on the previous check too never will read; waiting on
/// it would let the provider delete every other transcript before it is kept.
#[cfg(unix)]
#[test]
fn a_transcript_unreadable_for_a_week_past_the_cutoff_stops_holding_the_fold_back() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-long-unreadable");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = write_record(&home, "b.jsonl", "2026-08-06T04:05:13.944Z", 300);
    set_mtime(&locked, "2026-08-06T23:59:59Z");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let mut checks = FoldChecks::default();

    fold_with(&home, &mut checks);
    let after_one_check = watermark(&home);
    fold_with(&home, &mut checks);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(after_one_check, Watermark::NONE);
    assert_eq!(watermark(&home).ms(), Some(at("2026-08-14T00:00:00Z")));
    assert_eq!(folded_outputs(&home), [20]);
    let _ = std::fs::remove_dir_all(&home);
}

/// An old transcript that fails to read once (another process held it for a moment) is waited
/// for, and its usage is kept once it reads.
#[cfg(unix)]
#[test]
fn an_old_transcript_that_fails_to_read_once_is_waited_for() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-transient-unreadable");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = write_record(&home, "b.jsonl", "2026-08-06T04:05:13.944Z", 300);
    set_mtime(&locked, "2026-08-06T23:59:59Z");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let mut checks = FoldChecks::default();

    fold_with(&home, &mut checks);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    fold_with(&home, &mut checks);

    assert_eq!(watermark(&home).ms(), Some(at("2026-08-14T00:00:00Z")));
    let mut outputs = folded_outputs(&home);
    outputs.sort_unstable();
    assert_eq!(outputs, [20, 300]);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_history_that_does_not_read_is_counted_around_and_never_written_over() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-torn");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let history = history_path_for(&home);
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    std::fs::write(&history, "{ torn").unwrap();

    fold(&home);
    let summary = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&summary), 20);
    assert_eq!(std::fs::read_to_string(&history).unwrap(), "{ torn");
    let _ = std::fs::remove_dir_all(&home);
}

/// Once folded, a transcript is done: it leaves the scan cache, and even a rebuilt source index
/// (a parser or index version bump) does not parse it again.
#[test]
fn a_folded_transcript_is_never_parsed_again() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-no-reparse");
    let path = write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    fold(&home);

    assert!(!scan_cache_holds(&home, &path));
    std::fs::remove_file(source_index_path_for(&home)).unwrap();
    reset_transcript_parse_count();
    let again = read_offline(&home, full_time_input(false));

    assert_eq!(transcript_parse_count(), 0);
    assert_eq!(output_tokens(&again), 20);
    let _ = std::fs::remove_dir_all(&home);
}

/// Clearing forgets what only the history held; usage whose transcript is still on disk is
/// counted again from it, and a summary stored before the clear is not served after it.
#[test]
fn clearing_the_history_recounts_what_is_still_on_disk() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-clear");
    let gone = write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    write_record(&home, "b.jsonl", "2026-08-08T04:05:13.944Z", 300);
    fold(&home);
    std::fs::remove_file(&gone).unwrap();
    let before_clear = read_offline(&home, august_input(false));
    assert!(summary_cache_path_for(&home).exists());

    clear_history_in(&home).unwrap();
    let after_clear = read_offline(&home, august_input(false));
    fold(&home);

    assert_eq!(output_tokens(&before_clear), 320);
    assert_eq!(output_tokens(&after_clear), 300);
    assert!(!after_clear.cache_hit);
    assert_eq!(folded_outputs(&home), [300]);
    let _ = std::fs::remove_dir_all(&home);
}

/// A fold changes what a read counts from where, so a summary stored before it is not served.
#[test]
fn a_summary_stored_before_a_fold_is_not_served_after_it() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-summary-key");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    read_offline(&home, august_input(false));
    assert!(read_offline(&home, august_input(false)).cache_hit);

    fold(&home);
    let after_fold = read_offline(&home, august_input(false));

    assert!(!after_fold.cache_hit);
    assert_eq!(output_tokens(&after_fold), 20);
    let _ = std::fs::remove_dir_all(&home);
}

/// Claude Code sets a replaced transcript aside instead of overwriting it; a turn the rewrite
/// dropped still counts, and a turn both copies hold counts once.
#[test]
fn a_superseded_transcript_keeps_the_turns_its_rewrite_dropped() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-superseded");
    let kept = claude_line("msg_1", "2026-08-18T04:05:13.944Z", 20);
    let dropped = claude_line("msg_0", "2026-08-18T04:00:00.000Z", 300);
    write_lines(
        &home,
        "s.jsonl",
        std::slice::from_ref(&kept),
        "2026-08-18T04:05:14Z",
    );
    write_lines(
        &home,
        "s.jsonl.superseded-1755489600000",
        &[dropped, kept],
        "2026-08-18T04:05:14Z",
    );

    let summary = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&summary), 320);
    assert_eq!(record_count(&summary), 2);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn history_status_says_how_far_back_usage_is_kept() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-status");
    let empty = history_status_in(&home);
    assert_eq!(empty.state, UsageHistoryState::Empty);
    assert_eq!(
        (empty.kept_since, empty.folded_through, empty.bytes),
        (None, None, 0)
    );

    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    fold(&home);
    let kept = history_status_in(&home);

    assert_eq!(kept.state, UsageHistoryState::Kept);
    assert_eq!(kept.kept_since.as_deref(), Some("2026-08-07T04:00:00.000Z"));
    assert_eq!(
        kept.folded_through.as_deref(),
        Some("2026-08-14T00:00:00.000Z")
    );
    assert_eq!(
        kept.bytes,
        std::fs::metadata(history_path_for(&home)).unwrap().len()
    );

    std::fs::write(history_path_for(&home), "{ torn").unwrap();
    assert_eq!(
        history_status_in(&home).state,
        UsageHistoryState::Unreadable
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// A transcript whose newest record is folded leaves the scan cache even when it was written
/// recently, so its mtime alone would keep it.
#[test]
fn a_transcript_whose_records_are_all_folded_leaves_the_scan_cache() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-prune-by-record");
    let path = write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    read_offline(&home, august_input(false));
    assert!(scan_cache_holds(&home, &path));

    fold(&home);

    assert!(!scan_cache_holds(&home, &path));
    assert_eq!(output_tokens(&read_offline(&home, august_input(false))), 20);
    let _ = std::fs::remove_dir_all(&home);
}

/// Just after the watermark, a transcript still counts from itself: one written six hours after
/// it, and one whose record is stamped an hour after it by a clock five hours ahead of the
/// filesystem's. Both are inside the 36-hour allowance.
#[test]
fn transcripts_written_around_the_watermark_are_still_read() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-around-watermark");
    write_record(&home, "after.jsonl", "2026-08-14T06:00:00.000Z", 20);
    let skewed = write_single_claude_record(&home, "skewed.jsonl", "2026-08-14T01:00:00.000Z", 300);
    set_mtime(&skewed, "2026-08-13T19:00:00Z");

    fold(&home);
    let summary = read_offline(&home, august_input(false));

    assert_eq!(watermark(&home).ms(), Some(at("2026-08-14T00:00:00Z")));
    assert!(folded_outputs(&home).is_empty());
    assert_eq!(output_tokens(&summary), 320);
    let _ = std::fs::remove_dir_all(&home);
}

/// The watermark belongs to the transcripts: a record stamped exactly at it is not folded, and
/// is counted once, from its transcript.
#[test]
fn a_record_exactly_at_the_watermark_counts_once() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-at-watermark");
    write_record(&home, "a.jsonl", "2026-08-14T00:00:00.000Z", 20);

    fold(&home);
    let summary = read_offline(&home, august_input(false));

    assert!(folded_outputs(&home).is_empty());
    assert_eq!(output_tokens(&summary), 20);
    assert_eq!(record_count(&summary), 1);
    let _ = std::fs::remove_dir_all(&home);
}

/// A message's copies can sit either side of the watermark in different transcripts (a partial
/// line in one, the finished message in a resumed session). Copies collapse to the richest before
/// the watermark splits them, so the message counts once, whole.
#[test]
fn a_message_whose_copies_straddle_the_watermark_counts_its_richest_copy_once() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-straddle");
    let partial = claude_line("msg_1", "2026-08-13T23:59:59.999Z", 1);
    let finished = claude_line("msg_1", "2026-08-14T00:00:00.001Z", 50);
    write_lines(&home, "first.jsonl", &[partial], "2026-08-14T00:00:00Z");
    write_lines(&home, "resumed.jsonl", &[finished], "2026-08-14T00:00:01Z");

    fold(&home);
    let summary = read_offline(&home, august_input(false));

    assert!(folded_outputs(&home).is_empty());
    assert_eq!(output_tokens(&summary), 50);
    assert_eq!(record_count(&summary), 1);
    let _ = std::fs::remove_dir_all(&home);
}

/// Until a fold is due, the background check reads the history file and nothing else.
#[test]
fn the_background_check_reads_nothing_else_until_a_fold_is_due() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-idle-check");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    fold(&home);
    std::fs::remove_file(source_index_path_for(&home)).unwrap();
    reset_transcript_parse_count();

    fold(&home);

    assert!(!source_index_path_for(&home).exists());
    assert_eq!(transcript_parse_count(), 0);
    let _ = std::fs::remove_dir_all(&home);
}

/// A history file that exists but cannot be opened (another process holds it, its permissions
/// changed) is not the same as no history: it is neither folded over nor backed up, and reads
/// count the transcripts around it.
#[cfg(unix)]
#[test]
fn a_history_file_that_cannot_be_opened_is_left_alone() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-locked-history");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    write_record(&home, "b.jsonl", "2026-08-16T04:05:13.944Z", 300);
    fold(&home);
    let history = history_path_for(&home);
    let before = std::fs::read_to_string(&history).unwrap();
    std::fs::set_permissions(&history, std::fs::Permissions::from_mode(0o000)).unwrap();

    fold_history_in(
        &home,
        at("2026-08-30T12:00:00Z"),
        &mut FoldChecks::default(),
    )
    .unwrap();
    let status = history_status_in(&home);
    let summary = read_offline(&home, august_input(false));

    let mode = std::fs::metadata(&history).unwrap().permissions().mode() & 0o777;
    std::fs::set_permissions(&history, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(mode, 0o000);
    assert_eq!(status.state, UsageHistoryState::Unreadable);
    assert_eq!(std::fs::read_to_string(&history).unwrap(), before);
    assert!(!home.join(".on-n-off/usage-history.json.bak").exists());
    assert_eq!(output_tokens(&summary), 320);
    let _ = std::fs::remove_dir_all(&home);
}

/// A read made while the history file cannot be opened counts transcripts alone. Its summary is
/// not stored, so the file reading again brings its usage back rather than a stale undercount.
#[cfg(unix)]
#[test]
fn a_summary_counted_without_the_history_is_not_stored() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-summary-while-locked");
    let gone = write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    write_record(&home, "b.jsonl", "2026-08-16T04:05:13.944Z", 300);
    fold(&home);
    std::fs::remove_file(&gone).unwrap();
    let history = history_path_for(&home);
    std::fs::set_permissions(&history, std::fs::Permissions::from_mode(0o000)).unwrap();

    let without = read_offline(&home, august_input(false));
    std::fs::set_permissions(&history, std::fs::Permissions::from_mode(0o644)).unwrap();
    let with = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&without), 300);
    assert!(!with.cache_hit);
    assert_eq!(output_tokens(&with), 320);
    let _ = std::fs::remove_dir_all(&home);
}

/// Something at the history's path that does not read as a file (here a directory, which fails
/// to read on every platform, as a file another process holds does on Windows) is unreadable:
/// never folded over or written, and reads count the transcripts around it.
#[test]
fn a_history_path_that_cannot_be_read_is_left_alone_on_every_platform() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-not-a-file");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    write_record(&home, "b.jsonl", "2026-08-16T04:05:13.944Z", 300);
    let history = history_path_for(&home);
    std::fs::create_dir_all(&history).unwrap();

    fold(&home);
    let status = history_status_in(&home);
    let summary = read_offline(&home, august_input(false));

    assert_eq!(status.state, UsageHistoryState::Unreadable);
    assert!(history.is_dir());
    assert!(!home.join(".on-n-off/usage-history.json.bak").exists());
    assert_eq!(output_tokens(&summary), 320);
    let _ = std::fs::remove_dir_all(&home);
}
