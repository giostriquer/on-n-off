use std::io::Write;

use super::super::test_support::*;
use super::super::*;
use crate::paths::scratch_dir;
use crate::usage::folding::{fold_history_in, FoldChecks};
use crate::usage::pricing;

fn in_zone(time_zone: &str, input: UsageSummaryInput) -> UsageSummaryInput {
    UsageSummaryInput {
        time_zone: time_zone.into(),
        ..input
    }
}

fn append_record(path: &Path, message_id: &str, timestamp: &str, output_tokens: u64) {
    let line = claude_usage_line(
        message_id,
        "sess-claude",
        timestamp,
        serde_json::json!({ "input_tokens": 1, "output_tokens": output_tokens }),
    );
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{line}").unwrap();
}

fn days(summary: &UsageSummaryDto) -> Vec<&str> {
    summary
        .buckets
        .iter()
        .map(|bucket| bucket.day.as_str())
        .collect()
}

#[test]
fn a_day_window_reads_transcripts_written_up_to_36_hours_before_its_opening_utc_midnight_in_any_zone(
) {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-window-day-allowance");
    let edge = write_single_claude_record(&home, "edge.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let earlier =
        write_single_claude_record(&home, "earlier.jsonl", "2026-08-08T04:05:13.944Z", 30);
    set_mtime(&edge, "2026-07-30T12:00:00.000Z");
    set_mtime(&earlier, "2026-07-30T11:59:59.999Z");

    for zone in ["UTC", "America/Los_Angeles", "Pacific/Kiritimati"] {
        let summary = read_offline(&home, in_zone(zone, august_input(true)));
        assert_eq!(output_tokens(&summary), 20, "{zone}");
        assert_eq!(claude_scanned_files(&summary), 1, "{zone}");
    }
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn an_hourly_window_reads_transcripts_written_up_to_36_hours_before_its_opening_instant() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-window-hour-allowance");
    let edge = write_single_claude_record(&home, "edge.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let earlier =
        write_single_claude_record(&home, "earlier.jsonl", "2026-08-07T05:05:13.944Z", 30);
    set_mtime(&edge, "2026-08-05T12:37:00.000Z");
    set_mtime(&earlier, "2026-08-05T12:36:59.999Z");

    let summary = read_offline(
        &home,
        hourly_input("2026-08-07T00:37:00Z", "2026-08-08T00:37:00Z"),
    );

    assert_eq!(output_tokens(&summary), 20);
    assert_eq!(claude_scanned_files(&summary), 1);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_change_on_a_local_day_of_the_window_outside_its_utc_days_is_not_served_from_the_cache() {
    let _serial = pricing::lock_rates_state();
    for (label, zone, first, second) in [
        (
            "east",
            "Asia/Tokyo",
            "2026-07-31T20:00:00.000Z",
            "2026-07-31T21:00:00.000Z",
        ),
        (
            "west",
            "America/Los_Angeles",
            "2026-09-01T03:00:00.000Z",
            "2026-09-01T04:00:00.000Z",
        ),
    ] {
        let home = scratch_dir(&format!("usage-window-signature-{label}"));
        let path = write_single_claude_record(&home, "edge.jsonl", first, 20);
        let initial = read_offline(&home, in_zone(zone, august_input(false)));
        assert_eq!(output_tokens(&initial), 20, "{zone}");

        append_record(&path, "msg_second", second, 25);
        let after = read_offline(&home, in_zone(zone, august_input(false)));

        assert!(!after.cache_hit, "{zone}");
        assert_eq!(output_tokens(&after), 45, "{zone}");
        let _ = std::fs::remove_dir_all(home);
    }
}

#[test]
fn folded_usage_on_a_local_day_east_of_utc_counts_in_that_days_window() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-window-folded-east");
    let path = write_single_claude_record(&home, "tokyo.jsonl", "2026-07-31T20:00:00.000Z", 20);
    set_mtime(&path, "2026-07-31T20:00:01Z");
    fold_history_in(
        &home,
        at("2026-08-21T12:00:00Z"),
        &mut FoldChecks::default(),
    )
    .unwrap();
    std::fs::remove_file(&path).unwrap();

    let tokyo = read_offline(&home, in_zone("Asia/Tokyo", august_input(false)));
    let utc = read_offline(&home, august_input(false));

    assert_eq!(output_tokens(&tokyo), 20);
    assert_eq!(days(&tokyo), ["2026-08-01"]);
    assert_eq!(output_tokens(&utc), 0);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_day_that_is_not_a_calendar_day_is_refused_before_any_home() {
    for (since, until, field, text) in [
        ("acme", "acme", "sinceDay", "acme"),
        ("2026-02-30", "2026-03-01", "sinceDay", "2026-02-30"),
        ("2026-08-01", "2026-8-31", "untilDay", "2026-8-31"),
    ] {
        let err = super::refused_before_any_home(day_input(since, until, false));
        assert_eq!(
            err.message,
            format!("{field} '{text}' is not a day (YYYY-MM-DD)")
        );
    }
}

#[test]
fn a_day_window_counts_folded_usage_across_its_days_whatever_instants_it_carries() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-window-day-with-instants");
    let folded = write_single_claude_record(&home, "folded.jsonl", "2026-08-07T04:05:13.944Z", 20);
    set_mtime(&folded, "2026-08-07T04:05:14Z");
    fold_history_in(
        &home,
        at("2026-08-21T12:00:00Z"),
        &mut FoldChecks::default(),
    )
    .unwrap();
    std::fs::remove_file(&folded).unwrap();
    write_single_claude_record(&home, "live.jsonl", "2026-08-20T04:05:13.944Z", 25);

    let summary = read_offline(
        &home,
        UsageSummaryInput {
            since_time: Some("2026-08-10T00:00:00Z".into()),
            until_time: Some("2026-08-11T00:00:00Z".into()),
            ..august_input(false)
        },
    );

    assert_eq!(days(&summary), ["2026-08-07", "2026-08-20"]);
    assert_eq!(output_tokens(&summary), 45);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_change_outside_a_window_that_opens_or_closes_at_a_dst_midnight_is_served_from_the_cache() {
    let _serial = pricing::lock_rates_state();
    for (label, zone, since, until, inside, outside, later) in [
        (
            "skipped",
            "America/Sao_Paulo",
            "2018-11-04",
            "2018-11-10",
            "2018-11-05T12:00:00.000Z",
            "2018-10-01T12:00:00.000Z",
            "2018-10-02T12:00:00.000Z",
        ),
        (
            "repeated",
            "America/Havana",
            "2018-10-28",
            "2018-11-03",
            "2018-11-01T12:00:00.000Z",
            "2018-11-04T04:30:00.000Z",
            "2018-11-04T04:40:00.000Z",
        ),
    ] {
        let home = scratch_dir(&format!("usage-window-dst-{label}"));
        write_single_claude_record(&home, "inside.jsonl", inside, 20);
        let outside_path = write_single_claude_record(&home, "outside.jsonl", outside, 30);
        let input = || in_zone(zone, day_input(since, until, false));
        let initial = read_offline(&home, input());
        assert_eq!(output_tokens(&initial), 20, "{zone}");

        append_record(&outside_path, "msg_later", later, 5);
        let after = read_offline(&home, input());

        assert!(after.cache_hit, "{zone}");
        assert_eq!(output_tokens(&after), 20, "{zone}");
        let _ = std::fs::remove_dir_all(home);
    }
}

#[test]
fn hourly_reads_keep_one_summary_cache_entry_and_leave_day_windows_in_it() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-window-hourly-cache");
    write_claude_transcript(&home);
    read_offline(&home, august_input(false));
    let hour = |minute: u32| {
        hourly_input(
            &format!("2026-08-07T00:{minute:02}:00Z"),
            &format!("2026-08-08T00:{minute:02}:00Z"),
        )
    };
    for minute in 0..8 {
        read_offline(&home, hour(minute));
    }

    assert!(read_offline(&home, august_input(false)).cache_hit);
    assert!(read_offline(&home, hour(7)).cache_hit);
    assert!(!read_offline(&home, hour(6)).cache_hit);
    let _ = std::fs::remove_dir_all(home);
}
