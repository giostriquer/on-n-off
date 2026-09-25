//! When a read is served from the summary cache, and when what a read counted is stored there: only
//! a read that accounted for every transcript, read each to a final parse, counted with a readable
//! history, and found nothing changed by the time it stored.

use super::super::test_support::*;
use super::super::*;
use crate::paths::scratch_dir;
use crate::usage::pricing;
use crate::usage::sources::{reset_transcript_parse_count, transcript_parse_count};
use crate::usage::summary_cache::summary_cache_path_for;

#[test]
fn a_forced_read_is_never_served_from_the_cache_but_stores_what_it_counted() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-summary-forced");
    write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);

    let forced = read_offline(&home, august_input(true));
    let served = read_offline(&home, august_input(false));
    let forced_again = read_offline(&home, august_input(true));

    assert!(!forced.cache_hit);
    assert!(served.cache_hit);
    assert_eq!(output_tokens(&served), 20);
    assert!(!forced_again.cache_hit);
    let _ = std::fs::remove_dir_all(&home);
}

/// A walk that could not list a directory may have missed transcripts, so its count is neither
/// served from the cache nor stored.
#[cfg(unix)]
#[test]
fn a_read_that_could_not_walk_every_directory_is_neither_served_nor_stored() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-summary-locked-dir");
    write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let locked = home.join(".claude/projects/locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let first = read_offline(&home, august_input(false));
    let stored = summary_cache_path_for(&home).exists();
    let second = read_offline(&home, august_input(false));

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(output_tokens(&first), 20);
    assert!(!stored);
    assert!(!second.cache_hit);
    let _ = std::fs::remove_dir_all(&home);
}

/// A transcript that did not read (here one no longer readable, with no cached parse to stand in)
/// leaves the count short, so it is not stored, though nothing about the transcripts changed.
#[cfg(unix)]
#[test]
fn a_read_that_could_not_read_a_transcript_is_not_stored() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-summary-unreadable");
    let path = write_single_claude_record(&home, "a.jsonl", "2026-08-07T04:05:13.944Z", 20);
    read_offline(&home, august_input(false));
    let paths = crate::usage::sources::UsagePaths::for_home(&home);
    std::fs::remove_file(&paths.summary).unwrap();
    std::fs::remove_file(&paths.scan_cache).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

    let unread = read_offline(&home, august_input(false));

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(output_tokens(&unread), 0);
    assert!(!paths.summary.exists());
    let _ = std::fs::remove_dir_all(&home);
}

/// A transcript written to between the read and the store makes the count stale before it is
/// stored, so it is not.
#[test]
fn a_read_whose_transcripts_changed_before_it_stored_is_not_stored() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-summary-changed-before-store");
    write_claude_transcript(&home);

    let reached = Arc::new(std::sync::Barrier::new(2));
    let resume = Arc::new(std::sync::Barrier::new(2));
    let (read_reached, read_resume) = (Arc::clone(&reached), Arc::clone(&resume));
    let read_home = home.clone();
    let read = std::thread::spawn(move || {
        with_before_publish_pause(read_reached, read_resume, || {
            read_offline(&read_home, august_input(false))
        })
    });
    reached.wait();
    append_claude_record(&home, "msg_2", 25);
    resume.wait();

    assert_eq!(output_tokens(&read.join().unwrap()), 20);
    assert!(!summary_cache_path_for(&home).exists());
    let _ = std::fs::remove_dir_all(&home);
}

/// A read served from the cache after bringing the source index up to date keeps what it parsed
/// doing so. Here the transcript created after August was stored is July's, outside August, so
/// August is served; the July read that follows counts it without parsing it again.
#[test]
fn a_read_served_after_indexing_a_new_transcript_keeps_its_parse() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-summary-served-keeps-parses");
    write_single_claude_record(&home, "august.jsonl", "2026-08-07T04:05:13.944Z", 20);
    read_offline(&home, august_input(false));
    write_single_claude_record(&home, "july.jsonl", "2026-07-07T04:05:13.944Z", 10);
    assert!(read_offline(&home, august_input(false)).cache_hit);
    reset_transcript_parse_count();

    let july = read_offline(&home, day_input("2026-07-01", "2026-07-31", false));

    assert_eq!(output_tokens(&july), 10);
    assert_eq!(transcript_parse_count(), 0, "july.jsonl was parsed again");
    let _ = std::fs::remove_dir_all(&home);
}
