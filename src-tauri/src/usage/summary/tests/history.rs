use super::super::test_support::*;
use super::super::*;
use crate::dto::{AgentId, UsageHistoryState};
use crate::paths::scratch_dir;
use crate::usage::files::testing::{
    cached_record_count, history_open_count, history_path_for, reset_history_open_count,
    reset_scan_cache_decode_count, reset_transcript_parse_count, scan_cache_decode_count,
    source_index_file, summary_cache_path_for, transcript_parse_count, HistoryStore, Watermark,
};
use crate::usage::files::{FoldChecks, FoldedRow, Step};
use crate::usage::pricing;

const AFTER_AUGUST_FIRST_WEEK: &str = "2026-08-21T12:00:00Z";

fn fold(home: &Path) {
    fold_with(home, &mut FoldChecks::default());
}

fn fold_with(home: &Path, checks: &mut FoldChecks) {
    UsageFiles::of(home)
        .fold(at(AFTER_AUGUST_FIRST_WEEK), checks)
        .unwrap();
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

fn write_record(home: &Path, name: &str, timestamp: &str, output_tokens: u64) -> PathBuf {
    let path = write_single_claude_record(home, name, timestamp, output_tokens);
    set_mtime(&path, timestamp);
    path
}

fn write_lines(home: &Path, name: &str, lines: &[String], written: &str) -> PathBuf {
    write_claude_lines(home, name, lines);
    let path = home.join(".claude/projects/proj").join(name);
    set_mtime(&path, written);
    path
}

fn scan_cache_holds(home: &Path, path: &Path) -> bool {
    cached_record_count(home, path).is_some()
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

#[test]
fn a_folded_transcript_is_never_parsed_again() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-no-reparse");
    let path = write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    fold(&home);

    assert!(!scan_cache_holds(&home, &path));
    std::fs::remove_file(source_index_file(&home)).unwrap();
    reset_transcript_parse_count();
    let again = read_offline(&home, full_time_input(false));

    assert_eq!(transcript_parse_count(), 0);
    assert_eq!(output_tokens(&again), 20);
    let _ = std::fs::remove_dir_all(&home);
}

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

    UsageFiles::of(&home).clear().unwrap();
    let after_clear = read_offline(&home, august_input(false));
    fold(&home);

    assert_eq!(output_tokens(&before_clear), 320);
    assert_eq!(output_tokens(&after_clear), 300);
    assert!(!after_clear.cache_hit);
    assert_eq!(folded_outputs(&home), [300]);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_summary_that_a_fold_overtook_before_it_locked_is_kept_for_that_fold_and_not_after_a_clear() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-fold-before-lock");
    let gone = write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    write_record(&home, "b.jsonl", "2026-08-16T04:05:13.944Z", 300);

    let read_home = home.clone();
    let overtaken = paused_before(Step::Count, move || {
        read_offline(&read_home, august_input(false))
    });
    fold(&home);
    std::fs::remove_file(&gone).unwrap();
    let overtaken = overtaken.resume();
    let served = read_offline(&home, august_input(false));
    UsageFiles::of(&home).clear().unwrap();
    let after_clear = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&overtaken), 320);
    assert!(served.cache_hit);
    assert_eq!(output_tokens(&served), 320);
    assert_eq!(output_tokens(&after_clear), 300);
    assert!(!after_clear.cache_hit);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_fold_waits_while_a_summary_holds_the_usage_files() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-fold-waits");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);

    let read_home = home.clone();
    let read = paused_before(Step::Read, move || {
        read_offline(&read_home, august_input(false))
    });
    let (folded, fold_finished) = std::sync::mpsc::channel();
    let fold_home = home.clone();
    let folding = std::thread::spawn(move || {
        fold(&fold_home);
        folded.send(()).unwrap();
    });
    let finished_while_held = fold_finished
        .recv_timeout(std::time::Duration::from_millis(300))
        .is_ok();
    let summary = read.resume();
    let finished_after = finished_while_held || fold_finished.recv_timeout(PAUSE_LIMIT).is_ok();
    folding.join().unwrap();

    assert!(
        !finished_while_held,
        "the fold ran while the read held the lock"
    );
    assert!(finished_after, "the fold never finished");
    assert_eq!(output_tokens(&summary), 20);
    assert_eq!(watermark(&home).ms(), Some(at("2026-08-14T00:00:00Z")));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_summary_counted_before_a_fold_does_not_replace_one_counted_after_it() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-fold-before-store");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    write_record(&home, "b.jsonl", "2026-08-16T04:05:13.944Z", 300);

    let read_home = home.clone();
    let before_fold = paused_before(Step::Store, move || {
        read_offline(&read_home, august_input(false))
    });
    fold(&home);
    let after_fold = read_offline(&home, august_input(false));
    let before_fold = before_fold.resume();
    let again = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&before_fold), 320);
    assert_eq!(output_tokens(&after_fold), 320);
    assert!(again.cache_hit);
    assert_eq!(output_tokens(&again), 320);
    let _ = std::fs::remove_dir_all(&home);
}

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

#[test]
fn a_summary_served_from_the_cache_reads_neither_the_history_nor_the_scan_cache() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-served-unread");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    write_record(&home, "b.jsonl", "2026-08-16T04:05:13.944Z", 300);
    fold(&home);
    read_offline(&home, august_input(false));
    reset_history_open_count();
    reset_scan_cache_decode_count();

    let served = read_offline(&home, august_input(false));

    assert!(served.cache_hit);
    assert_eq!(output_tokens(&served), 320);
    assert_eq!(history_open_count(), 0);
    assert_eq!(scan_cache_decode_count(), 0);
    let _ = std::fs::remove_dir_all(&home);
}

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

    fold_on(&home, "2026-08-30T12:00:00Z");
    let status = UsageFiles::of(&home).status();
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

#[test]
fn a_history_path_that_cannot_be_read_is_left_alone_on_every_platform() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-not-a-file");
    write_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    write_record(&home, "b.jsonl", "2026-08-16T04:05:13.944Z", 300);
    let history = history_path_for(&home);
    std::fs::create_dir_all(&history).unwrap();

    fold(&home);
    let status = UsageFiles::of(&home).status();
    let summary = read_offline(&home, august_input(false));

    assert_eq!(status.state, UsageHistoryState::Unreadable);
    assert!(history.is_dir());
    assert!(!home.join(".on-n-off/usage-history.json.bak").exists());
    assert_eq!(output_tokens(&summary), 320);
    let _ = std::fs::remove_dir_all(&home);
}
