//! Usage kept after a transcript is deleted: folding at a Usage read and in the background, the
//! watermark between folded rows and live transcripts, and a history that does not read.

use super::super::test_support::*;
use super::super::*;
use crate::paths::scratch_dir;
use crate::usage::history::{history_path_for, load_history, FoldedRow, LoadedHistory};
use crate::usage::pricing;
use crate::usage::source_index::{
    reset_transcript_parse_count, source_index_path_for, transcript_parse_count,
};
use crate::usage::summary_cache::summary_cache_path_for;

/// Two weeks after the fixtures' 2026-08-07: the fold cutoff is 2026-08-14, so August's first week
/// is folded and a record from 2026-08-15 on stays in its transcript.
const AFTER_AUGUST_FIRST_WEEK: &str = "2026-08-21T12:00:00Z";

fn at(iso: &str) -> i64 {
    DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

fn read_at(home: &Path, input: UsageSummaryInput, now: &str) -> UsageSummaryDto {
    pricing::with_test_fetch(None, || {
        read_summary_from(input, || Ok(home.to_path_buf()), at(now))
    })
    .unwrap()
}

fn folded_rows(home: &Path) -> Vec<FoldedRow> {
    match load_history(&history_path_for(home)) {
        LoadedHistory::Ready { history, .. } => history.rows().to_vec(),
        other => panic!("expected a readable history, got {other:?}"),
    }
}

fn folded_through(home: &Path) -> Option<i64> {
    match load_history(&history_path_for(home)) {
        LoadedHistory::Ready { history, .. } => history.folded_through_ms(),
        LoadedHistory::Missing => None,
        LoadedHistory::Unreadable => panic!("history does not read"),
    }
}

fn set_mtime(path: &Path, iso: &str) {
    let modified = std::time::SystemTime::from(DateTime::parse_from_rfc3339(iso).unwrap());
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

#[test]
fn a_deleted_transcript_still_counts_once_its_usage_is_folded() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-deleted");
    let path = write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);

    let before = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);
    assert_eq!(output_tokens(&before), 20);
    std::fs::remove_file(&path).unwrap();
    let after = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

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
fn nothing_is_folded_before_it_is_a_week_old() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-too-recent");
    let path = write_single_claude_record(&home, "a.jsonl", "2026-08-15T04:05:13.944Z", 20);

    read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);
    std::fs::remove_file(&path).unwrap();
    let after = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    assert_eq!(output_tokens(&after), 0);
    assert!(folded_rows(&home).is_empty());
    assert_eq!(folded_through(&home), Some(at("2026-08-14T00:00:00Z")));
    let _ = std::fs::remove_dir_all(&home);
}

/// A resumed session copies earlier messages into its new transcript under their original
/// timestamps. Once the original is folded, the copy is below the watermark and not counted again.
#[test]
fn a_live_copy_of_a_folded_record_is_not_counted_twice() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-copy");
    let original = claude_usage_line(
        "msg_1",
        "sess-a",
        "2026-08-07T04:05:13.944Z",
        serde_json::json!({ "input_tokens": 1, "output_tokens": 20 }),
    );
    let later = claude_usage_line(
        "msg_2",
        "sess-b",
        "2026-08-20T04:05:13.944Z",
        serde_json::json!({ "input_tokens": 1, "output_tokens": 300 }),
    );
    write_claude_lines(&home, "original.jsonl", std::slice::from_ref(&original));
    read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    write_claude_lines(&home, "resumed.jsonl", &[original, later]);
    let with_both = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);
    std::fs::remove_file(home.join(".claude/projects/proj/original.jsonl")).unwrap();
    let with_copy_only = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    assert_eq!(output_tokens(&with_both), 320);
    assert_eq!(output_tokens(&with_copy_only), 320);
    assert_eq!(record_count(&with_copy_only), 2);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn records_after_the_watermark_are_read_from_their_transcript() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-live-side");
    let first = claude_usage_line(
        "msg_1",
        "sess-a",
        "2026-08-07T04:05:13.944Z",
        serde_json::json!({ "input_tokens": 1, "output_tokens": 20 }),
    );
    let recent = claude_usage_line(
        "msg_2",
        "sess-a",
        "2026-08-18T04:05:13.944Z",
        serde_json::json!({ "input_tokens": 1, "output_tokens": 300 }),
    );
    write_claude_lines(&home, "a.jsonl", &[first.clone(), recent.clone()]);
    let folded = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    let appended = claude_usage_line(
        "msg_3",
        "sess-a",
        "2026-08-20T04:05:13.944Z",
        serde_json::json!({ "input_tokens": 1, "output_tokens": 4000 }),
    );
    write_claude_lines(&home, "a.jsonl", &[first, recent, appended]);
    let grown = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    assert_eq!(output_tokens(&folded), 320);
    assert_eq!(output_tokens(&grown), 4320);
    let folded_outputs: Vec<u64> = folded_rows(&home)
        .iter()
        .map(|row| row.totals.output_tokens)
        .collect();
    assert_eq!(folded_outputs, [20]);
    let _ = std::fs::remove_dir_all(&home);
}

/// An agent is almost always writing some transcript. Its records older than the cutoff were
/// written days ago, so what it holds is enough to fold them.
#[test]
fn a_transcript_still_being_written_does_not_hold_the_fold_back() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-live-writer");
    let old = claude_usage_line(
        "msg_1",
        "sess-a",
        "2026-08-07T04:05:13.944Z",
        serde_json::json!({ "input_tokens": 1, "output_tokens": 20 }),
    );
    let still_writing = claude_usage_line(
        "msg_2",
        "sess-a",
        "2026-08-21T11:00:00.000Z",
        serde_json::json!({ "input_tokens": 1, "output_tokens": 1 }),
    );
    write_claude_lines(&home, "a.jsonl", &[old]);
    let path = home.join(".claude/projects/proj/a.jsonl");

    crate::usage::source_index::with_live_transcript(&path, &still_writing, || {
        read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK)
    });

    let folded_outputs: Vec<u64> = folded_rows(&home)
        .iter()
        .map(|row| row.totals.output_tokens)
        .collect();
    assert_eq!(folded_outputs, [20]);
    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn folding_waits_until_every_directory_can_be_walked() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-locked-dir");
    write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = home.join(".claude/projects/locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let summary = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(output_tokens(&summary), 20);
    assert_eq!(folded_through(&home), None);
    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn folding_waits_until_every_transcript_it_needs_can_be_read() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-locked-file");
    write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = write_single_claude_record(&home, "b.jsonl", "2026-08-08T04:05:13.944Z", 300);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(folded_through(&home), None);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_history_that_does_not_read_is_counted_around_and_never_written_over() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-torn");
    write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let history = history_path_for(&home);
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    std::fs::write(&history, "{ torn").unwrap();

    let summary = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

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
    let path = write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    set_mtime(&path, "2026-08-07T04:05:14Z");
    read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    let scan_cache = std::fs::read_to_string(home.join(".on-n-off/usage-scan-cache.json")).unwrap();
    assert!(!scan_cache.contains("a.jsonl"), "{scan_cache}");
    std::fs::remove_file(source_index_path_for(&home)).unwrap();
    std::fs::remove_file(summary_cache_path_for(&home)).unwrap();
    reset_transcript_parse_count();
    let again = read_at(&home, full_time_input(false), AFTER_AUGUST_FIRST_WEEK);

    assert_eq!(transcript_parse_count(), 0);
    assert_eq!(output_tokens(&again), 20);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn the_background_fold_keeps_usage_the_screen_never_read() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-background");
    let path = write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);

    fold_history_in(&home, at(AFTER_AUGUST_FIRST_WEEK)).unwrap();
    std::fs::remove_file(&path).unwrap();
    let after = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    assert_eq!(output_tokens(&after), 20);
    let _ = std::fs::remove_dir_all(&home);
}

/// Clearing forgets what only the history held; usage whose transcript is still on disk is
/// counted again from it.
#[test]
fn clearing_the_history_recounts_what_is_still_on_disk() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-clear");
    let gone = write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    write_single_claude_record(&home, "b.jsonl", "2026-08-08T04:05:13.944Z", 300);
    read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);
    std::fs::remove_file(&gone).unwrap();

    clear_history_in(&home).unwrap();
    let after = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    assert_eq!(output_tokens(&after), 300);
    let folded_outputs: Vec<u64> = folded_rows(&home)
        .iter()
        .map(|row| row.totals.output_tokens)
        .collect();
    assert_eq!(folded_outputs, [300]);
    let _ = std::fs::remove_dir_all(&home);
}

/// Claude Code sets a replaced transcript aside instead of overwriting it; a turn the rewrite
/// dropped still counts, and a turn both copies hold counts once.
#[test]
fn a_superseded_transcript_keeps_the_turns_its_rewrite_dropped() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-history-superseded");
    let kept = claude_usage_line(
        "msg_1",
        "sess-a",
        "2026-08-18T04:05:13.944Z",
        serde_json::json!({ "input_tokens": 1, "output_tokens": 20 }),
    );
    let dropped = claude_usage_line(
        "msg_0",
        "sess-a",
        "2026-08-18T04:00:00.000Z",
        serde_json::json!({ "input_tokens": 1, "output_tokens": 300 }),
    );
    write_claude_lines(&home, "s.jsonl", std::slice::from_ref(&kept));
    write_claude_lines(&home, "s.jsonl.superseded-1755489600000", &[dropped, kept]);

    let summary = read_at(&home, august_input(false), AFTER_AUGUST_FIRST_WEEK);

    assert_eq!(output_tokens(&summary), 320);
    assert_eq!(record_count(&summary), 2);
    let _ = std::fs::remove_dir_all(&home);
}
