use chrono::Offset;
use chrono_tz::TZ_VARIANTS;

use super::*;

fn input(since_day: &str, until_day: &str, time_zone: &str) -> UsageSummaryInput {
    UsageSummaryInput {
        since_day: since_day.into(),
        until_day: until_day.into(),
        time_zone: time_zone.into(),
        resolution: Some("day".into()),
        since_time: None,
        until_time: None,
        force: false,
    }
}

fn hourly(since: &str, until: &str, time_zone: &str) -> UsageSummaryInput {
    UsageSummaryInput {
        resolution: Some("hour".into()),
        since_time: Some(since.into()),
        until_time: Some(until.into()),
        ..input("2026-08-07", "2026-08-08", time_zone)
    }
}

fn window(input: UsageSummaryInput) -> UsageWindow {
    UsageWindow::of(&input).unwrap()
}

fn ms(iso: &str) -> i64 {
    DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

fn span(start: &str, end: &str) -> Span {
    Span {
        start_ms: ms(start),
        end_ms: ms(end),
    }
}

fn period(window: &UsageWindow, iso: &str) -> Option<(String, Option<String>)> {
    window
        .period_of(ms(iso))
        .map(|period| (period.day(), period.hour_start()))
}

fn day_of(day: &str) -> Option<(String, Option<String>)> {
    Some((day.into(), None))
}

fn hour_of(day: &str, hour_start: &str) -> Option<(String, Option<String>)> {
    Some((day.into(), Some(hour_start.into())))
}

#[test]
fn a_day_window_spans_its_local_days_and_reads_from_36_hours_before_its_opening_utc_midnight() {
    for (zone, start, end) in [
        ("UTC", "2026-08-01T00:00:00Z", "2026-09-01T00:00:00Z"),
        (
            "America/Los_Angeles",
            "2026-08-01T07:00:00Z",
            "2026-09-01T07:00:00Z",
        ),
        (
            "Pacific/Tongatapu",
            "2026-07-31T11:00:00Z",
            "2026-08-31T11:00:00Z",
        ),
        (
            "Pacific/Kiritimati",
            "2026-07-31T10:00:00Z",
            "2026-08-31T10:00:00Z",
        ),
    ] {
        let august = window(input("2026-08-01", "2026-08-31", zone));
        assert_eq!(august.span(), span(start, end), "{zone}");
        assert_eq!(
            august.earliest_mtime_ms(),
            ms("2026-07-30T12:00:00Z"),
            "{zone}"
        );
    }
}

#[test]
fn a_day_window_counts_an_instant_in_the_local_day_it_falls_on() {
    let kiritimati = window(input("2026-08-01", "2026-08-31", "Pacific/Kiritimati"));
    assert_eq!(period(&kiritimati, "2026-07-31T09:59:59.999Z"), None);
    assert_eq!(
        period(&kiritimati, "2026-07-31T10:00:00Z"),
        day_of("2026-08-01")
    );
    assert_eq!(
        period(&kiritimati, "2026-08-31T09:59:59.999Z"),
        day_of("2026-08-31")
    );
    assert_eq!(period(&kiritimati, "2026-08-31T10:00:00Z"), None);

    let tongatapu = window(input("2026-08-01", "2026-08-31", "Pacific/Tongatapu"));
    assert_eq!(period(&tongatapu, "2026-07-31T10:59:59.999Z"), None);
    assert_eq!(
        period(&tongatapu, "2026-07-31T11:00:00Z"),
        day_of("2026-08-01")
    );
}

#[test]
fn an_unknown_zone_reads_as_utc() {
    let unknown = window(input("2026-08-01", "2026-08-31", "Not/AZone"));
    let utc = window(input("2026-08-01", "2026-08-31", "UTC"));
    assert_eq!(unknown.span(), utc.span());
    assert_eq!(unknown.earliest_mtime_ms(), utc.earliest_mtime_ms());
    assert_eq!(
        period(&unknown, "2026-08-07T23:30:00Z"),
        day_of("2026-08-07")
    );
    assert_ne!(unknown.cache_identity(), utc.cache_identity());
}

#[test]
fn a_window_opening_at_a_midnight_the_clocks_skip_starts_when_its_day_does() {
    let sao_paulo = window(input("2018-11-04", "2018-11-10", "America/Sao_Paulo"));
    assert_eq!(
        sao_paulo.span(),
        span("2018-11-04T03:00:00Z", "2018-11-11T02:00:00Z")
    );
    assert_eq!(period(&sao_paulo, "2018-11-04T02:59:59.999Z"), None);
    assert_eq!(
        period(&sao_paulo, "2018-11-04T03:00:00Z"),
        day_of("2018-11-04")
    );
}

#[test]
fn a_midnight_the_clocks_repeat_starts_its_day_at_the_first_pass() {
    let before = window(input("2018-10-28", "2018-11-03", "America/Havana"));
    assert_eq!(before.span().end_ms, ms("2018-11-04T04:00:00Z"));
    assert_eq!(
        period(&before, "2018-11-04T03:59:59.999Z"),
        day_of("2018-11-03")
    );
    assert_eq!(period(&before, "2018-11-04T04:00:00Z"), None);

    let after = window(input("2018-11-04", "2018-11-04", "America/Havana"));
    assert_eq!(
        after.span(),
        span("2018-11-04T04:00:00Z", "2018-11-05T05:00:00Z")
    );
}

#[test]
fn a_day_the_zone_skipped_spans_no_instant() {
    let skipped = window(input("2011-12-30", "2011-12-30", "Pacific/Apia"));
    assert_eq!(
        skipped.span(),
        span("2011-12-30T10:00:00Z", "2011-12-30T10:00:00Z")
    );
}

#[test]
fn an_hourly_window_counts_hours_from_its_opening_instant_on_their_local_days() {
    let hours = window(hourly(
        "2026-08-06T06:37:00Z",
        "2026-08-07T06:37:00Z",
        "America/Los_Angeles",
    ));
    assert_eq!(
        hours.span(),
        span("2026-08-06T06:37:00Z", "2026-08-07T06:37:00Z")
    );
    assert_eq!(hours.earliest_mtime_ms(), ms("2026-08-04T18:37:00Z"));
    assert_eq!(period(&hours, "2026-08-06T06:36:59.999Z"), None);
    assert_eq!(
        period(&hours, "2026-08-06T06:37:00Z"),
        hour_of("2026-08-05", "2026-08-06T06:37:00.000Z")
    );
    assert_eq!(
        period(&hours, "2026-08-06T07:10:00Z"),
        hour_of("2026-08-06", "2026-08-06T06:37:00.000Z")
    );
    assert_eq!(
        period(&hours, "2026-08-06T07:37:00Z"),
        hour_of("2026-08-06", "2026-08-06T07:37:00.000Z")
    );
    assert_eq!(
        period(&hours, "2026-08-07T06:36:59.999Z"),
        hour_of("2026-08-06", "2026-08-07T05:37:00.000Z")
    );
    assert_eq!(period(&hours, "2026-08-07T06:37:00Z"), None);
}

#[test]
fn any_resolution_but_hour_reads_by_the_day_whatever_instants_it_carries() {
    for resolution in [None, Some("day"), Some("week")] {
        let days = window(UsageSummaryInput {
            resolution: resolution.map(Into::into),
            since_time: Some("2026-08-10T00:00:00Z".into()),
            until_time: Some("2026-08-11T00:00:00Z".into()),
            ..input("2026-08-01", "2026-08-31", "UTC")
        });
        assert_eq!(
            days.span(),
            span("2026-08-01T00:00:00Z", "2026-09-01T00:00:00Z"),
            "{resolution:?}"
        );
        assert_eq!(
            period(&days, "2026-08-07T04:05:00Z"),
            day_of("2026-08-07"),
            "{resolution:?}"
        );
    }
}

#[test]
fn a_window_that_names_no_span_is_refused() {
    let not_a_day = |field: &str, text: &str| format!("{field} '{text}' is not a day (YYYY-MM-DD)");
    let refusals = [
        (
            input("2026-08-10", "2026-08-01", "UTC"),
            "sinceDay '2026-08-10' is after untilDay '2026-08-01'".to_string(),
        ),
        (input("acme", "acme", "UTC"), not_a_day("sinceDay", "acme")),
        (
            input("2026-02-30", "2026-03-01", "UTC"),
            not_a_day("sinceDay", "2026-02-30"),
        ),
        (
            input("2026-08-01", "2026-8-31", "UTC"),
            not_a_day("untilDay", "2026-8-31"),
        ),
        (
            input("+2026-08-01", "2026-08-31", "UTC"),
            not_a_day("sinceDay", "+2026-08-01"),
        ),
        (
            input("+026-08-31", "2026-08-31", "UTC"),
            not_a_day("sinceDay", "+026-08-31"),
        ),
        (
            input("2026-08-01", "+10000-01-01", "UTC"),
            not_a_day("untilDay", "+10000-01-01"),
        ),
        (
            UsageSummaryInput {
                since_time: None,
                ..hourly("2026-08-07T00:00:00Z", "2026-08-07T01:00:00Z", "UTC")
            },
            "Hourly usage requires valid sinceTime and untilTime instants".to_string(),
        ),
        (
            hourly("seven", "2026-08-07T01:00:00Z", "UTC"),
            "Hourly usage requires valid sinceTime and untilTime instants".to_string(),
        ),
        (
            hourly("2026-08-07T01:00:00Z", "2026-08-07T01:00:00Z", "UTC"),
            "Hourly usage window must be greater than zero and at most 24 hours".to_string(),
        ),
        (
            hourly("2026-08-07T02:00:00Z", "2026-08-07T01:00:00Z", "UTC"),
            "Hourly usage window must be greater than zero and at most 24 hours".to_string(),
        ),
        (
            hourly("2026-08-07T00:00:00Z", "2026-08-08T00:00:00.001Z", "UTC"),
            "Hourly usage window must be greater than zero and at most 24 hours".to_string(),
        ),
    ];
    for (input, message) in refusals {
        match UsageWindow::of(&input) {
            Ok(_) => panic!("accepted {input:?}"),
            Err(error) => assert_eq!(error.message, message, "{input:?}"),
        }
    }
    assert!(UsageWindow::of(&hourly(
        "2026-08-07T00:00:00Z",
        "2026-08-08T00:00:00Z",
        "UTC"
    ))
    .is_ok());
}

#[test]
fn the_cache_identity_changes_with_everything_a_summary_counts_or_echoes() {
    let hour = UsageSummaryInput {
        since_day: "2026-08-01".into(),
        until_day: "2026-08-01".into(),
        ..hourly("2026-08-01T00:00:00Z", "2026-08-01T12:00:00Z", "UTC")
    };
    let others = [
        UsageSummaryInput {
            resolution: Some("day".into()),
            ..hour.clone()
        },
        UsageSummaryInput {
            since_day: "2026-07-31".into(),
            ..hour.clone()
        },
        UsageSummaryInput {
            until_day: "2026-08-02".into(),
            ..hour.clone()
        },
        UsageSummaryInput {
            time_zone: "America/Sao_Paulo".into(),
            ..hour.clone()
        },
        UsageSummaryInput {
            since_time: Some("2026-08-01T01:00:00Z".into()),
            ..hour.clone()
        },
        UsageSummaryInput {
            until_time: Some("2026-08-01T13:00:00Z".into()),
            ..hour.clone()
        },
    ];
    let key = |input: &UsageSummaryInput| window(input.clone()).cache_identity().key;
    for other in &others {
        assert_ne!(key(&hour), key(other), "{other:?}");
    }
}

#[test]
fn a_day_window_is_its_own_cache_group_and_hourly_windows_share_one() {
    let august = window(input("2026-08-01", "2026-08-31", "UTC")).cache_identity();
    let august_with_instants = window(UsageSummaryInput {
        since_time: Some("2026-08-10T00:00:00Z".into()),
        until_time: Some("2026-08-11T00:00:00Z".into()),
        ..input("2026-08-01", "2026-08-31", "UTC")
    })
    .cache_identity();
    let july = window(input("2026-07-01", "2026-07-31", "UTC")).cache_identity();
    let earlier = window(hourly(
        "2026-08-07T00:00:00Z",
        "2026-08-08T00:00:00Z",
        "UTC",
    ))
    .cache_identity();
    let later = window(hourly(
        "2026-08-07T00:01:00Z",
        "2026-08-08T00:01:00Z",
        "UTC",
    ))
    .cache_identity();
    let respelled = window(hourly(
        "2026-08-07T00:00:00.000Z",
        "2026-08-08T00:00:00+00:00",
        "UTC",
    ))
    .cache_identity();

    assert_eq!(august, august_with_instants);
    assert_ne!(august.group, july.group);
    assert_ne!(earlier.key, later.key);
    assert_eq!(earlier.group, later.group);
    assert_ne!(earlier.group, august.group);
    assert_eq!(respelled, earlier);
}

fn offset_seconds(at_ms: i64, zone: Tz) -> i32 {
    Utc.timestamp_millis_opt(at_ms)
        .unwrap()
        .with_timezone(&zone)
        .offset()
        .fix()
        .local_minus_utc()
}

fn days_around_agree(zone: Tz, from_ms: i64, to_ms: i64) -> usize {
    let first = local_day(from_ms, zone).unwrap().pred_opt().unwrap();
    let last = local_day(to_ms, zone).unwrap().succ_opt().unwrap();
    let mut probes = 0;
    for day in first.iter_days().take_while(|day| *day <= last) {
        let text = day_text(day);
        let single = window(input(&text, &text, zone.name()));
        let around = window(input(
            &day_text(day.pred_opt().unwrap()),
            &day_text(day.succ_opt().unwrap()),
            zone.name(),
        ));
        let Span { start_ms, end_ms } = single.span();
        for at in [start_ms - 1, start_ms, end_ms - 1, end_ms] {
            let in_span = (start_ms..end_ms).contains(&at);
            let on_day = around.period_of(at).map(Period::day).as_deref() == Some(text.as_str());
            assert_eq!(
                single.period_of(at).is_some(),
                in_span,
                "{} {text} at {at}",
                zone.name()
            );
            assert_eq!(on_day, in_span, "{} {text} at {at}", zone.name());
            probes += 1;
        }
    }
    probes
}

#[test]
fn in_every_zone_a_days_span_holds_exactly_the_instants_on_that_local_day() {
    const DAY_MS: i64 = 24 * HOUR_MS;
    const WEEK_MS: i64 = 7 * DAY_MS;
    let (from, to) = (ms("2020-01-01T00:00:00Z"), ms("2030-01-01T00:00:00Z"));
    let (mut transitions, mut probes) = (0, 0);
    for zone in TZ_VARIANTS {
        let mut week = from;
        let mut week_offset = offset_seconds(week, zone);
        while week < to {
            let next_offset = offset_seconds(week + WEEK_MS, zone);
            if next_offset != week_offset {
                for day in (week..week + WEEK_MS).step_by(DAY_MS as usize) {
                    if offset_seconds(day, zone) != offset_seconds(day + DAY_MS, zone) {
                        transitions += 1;
                        probes += days_around_agree(zone, day, day + DAY_MS);
                    }
                }
            }
            week += WEEK_MS;
            week_offset = next_offset;
        }
    }
    assert!(transitions > 1000, "{transitions} transitions");
    assert!(probes > transitions * 12, "{probes} probes");
}
