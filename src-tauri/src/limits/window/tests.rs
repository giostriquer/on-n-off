use super::*;
use crate::dto::LimitsWorkspaceCreditsDto;

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn reset(text: &str) -> ResetInstant {
    ResetInstant::parse(text).unwrap()
}

fn minutes(count: u64) -> Duration {
    Duration::from_secs(count * 60)
}

fn fact(scope: Scope, length: Option<Duration>, used_percent: f64) -> QuotaWindow {
    QuotaWindow {
        id: "w".into(),
        scope,
        length,
        used_percent,
        reset: None,
    }
}

fn model(name: &str) -> Scope {
    Scope::Model(name.into())
}

#[test]
fn every_fact_becomes_one_window_kind_label_meter_and_length() {
    let all = || Scope::AllModels;
    let rows: [(QuotaWindow, LimitWindowKind, &str, f64, Option<u64>); 16] = [
        (
            fact(all(), Some(FIVE_HOURS), 41.0),
            LimitWindowKind::Session,
            "5 hour · all models",
            41.0,
            Some(18_000),
        ),
        (
            fact(all(), Some(minutes(120)), 7.0),
            LimitWindowKind::Session,
            "2 hour · all models",
            7.0,
            Some(7_200),
        ),
        (
            fact(all(), Some(minutes(30)), 7.0),
            LimitWindowKind::Session,
            "30 minute · all models",
            7.0,
            Some(1_800),
        ),
        (
            fact(all(), Some(minutes(90)), 7.0),
            LimitWindowKind::Session,
            "90 minute · all models",
            7.0,
            Some(5_400),
        ),
        (
            fact(all(), Some(minutes(24 * 60 - 1)), 7.0),
            LimitWindowKind::Session,
            "1439 minute · all models",
            7.0,
            Some(86_340),
        ),
        (
            fact(all(), Some(minutes(24 * 60)), 7.0),
            LimitWindowKind::Weekly,
            "Weekly · all models",
            7.0,
            Some(86_400),
        ),
        (
            fact(all(), Some(ONE_WEEK), 12.0),
            LimitWindowKind::Weekly,
            "Weekly · all models",
            12.0,
            Some(604_800),
        ),
        (
            fact(all(), None, 12.0),
            LimitWindowKind::Weekly,
            "Weekly · all models",
            12.0,
            None,
        ),
        (
            fact(model("Fable"), Some(ONE_WEEK), 13.0),
            LimitWindowKind::Model,
            "Weekly · Fable",
            13.0,
            Some(604_800),
        ),
        (
            fact(model("GPT-5.6-Luna"), Some(FIVE_HOURS), 1.0),
            LimitWindowKind::Model,
            "5 hour · GPT-5.6-Luna",
            1.0,
            Some(18_000),
        ),
        (
            fact(model("extra limit"), None, 2.0),
            LimitWindowKind::Model,
            "Weekly · extra limit",
            2.0,
            None,
        ),
        (
            fact(all(), Some(ONE_WEEK), 140.0),
            LimitWindowKind::Weekly,
            "Weekly · all models",
            100.0,
            Some(604_800),
        ),
        (
            fact(all(), Some(ONE_WEEK), -3.0),
            LimitWindowKind::Weekly,
            "Weekly · all models",
            0.0,
            Some(604_800),
        ),
        (
            fact(all(), Some(ONE_WEEK), f64::NAN),
            LimitWindowKind::Weekly,
            "Weekly · all models",
            0.0,
            Some(604_800),
        ),
        (
            fact(all(), Some(ONE_WEEK), f64::INFINITY),
            LimitWindowKind::Weekly,
            "Weekly · all models",
            0.0,
            Some(604_800),
        ),
        (
            QuotaWindow::idle("session", FIVE_HOURS),
            LimitWindowKind::Session,
            "5 hour · all models",
            0.0,
            Some(18_000),
        ),
    ];
    for (fact, kind, label, used_percent, window_seconds) in rows {
        let described = format!("{fact:?}");
        let id = fact.id.clone();
        let dto = fact.into_dto();
        assert_eq!(dto.id, id, "{described}");
        assert_eq!(dto.kind, kind, "{described}");
        assert_eq!(dto.label, label, "{described}");
        assert_eq!(dto.used_percent, used_percent, "{described}");
        assert_eq!(dto.window_seconds, window_seconds, "{described}");
        assert!(dto.observed_at.is_empty(), "{described}");
    }
}

#[test]
fn an_idle_window_is_unused_has_no_reset_and_counts_for_all_models() {
    let idle = QuotaWindow::idle("session", FIVE_HOURS);
    assert_eq!(idle.scope, Scope::AllModels);
    assert_eq!(idle.used_percent, 0.0);
    assert_eq!(idle.reset, None);
    assert_eq!(idle.into_dto().resets_at, None);
}

#[test]
fn every_reset_is_spelled_once_in_utc_to_the_millisecond() {
    for (reset_at, spelled) in [
        (
            at("2026-08-18T04:59:59.692639+00:00"),
            "2026-08-18T04:59:59.692Z",
        ),
        (at("2026-08-24T13:59:59+00:00"), "2026-08-24T13:59:59.000Z"),
        (at("2026-08-24T10:59:59-03:00"), "2026-08-24T13:59:59.000Z"),
        (
            DateTime::<Utc>::from_timestamp(1_787_838_960, 0).unwrap(),
            "2026-08-27T13:56:00.000Z",
        ),
        (
            DateTime::<Utc>::from_timestamp_millis(1_760_000_000_500).unwrap(),
            "2025-10-09T08:53:20.500Z",
        ),
    ] {
        let window = QuotaWindow {
            reset: Some(reset_at.into()),
            ..fact(Scope::AllModels, Some(ONE_WEEK), 1.0)
        };
        assert_eq!(window.into_dto().resets_at.as_deref(), Some(spelled));
        assert_eq!(ResetInstant::from(reset_at).to_string(), spelled);
        assert_eq!(
            ResetInstant::parse(spelled).map(|reset| reset.to_string()),
            Some(spelled.to_string())
        );
    }
}

#[test]
fn a_reset_instant_reads_any_spelling_of_one_instant_as_that_instant() {
    let spellings = [
        "2026-10-05T12:00:00Z",
        "2026-10-05T12:00:00+00:00",
        "2026-10-05T12:00:00.000Z",
        "2026-10-05T09:00:00-03:00",
    ];
    for one in spellings {
        for other in spellings {
            assert_eq!(reset(one), reset(other), "{one} vs {other}");
        }
    }
    for unreadable in ["next week", "", "2026-10-05", "1787838960"] {
        assert!(ResetInstant::parse(unreadable).is_none(), "{unreadable}");
    }
}

#[test]
fn a_reset_instant_equals_and_orders_only_by_its_exact_instant() {
    let noon = reset("2026-10-05T12:00:00Z");
    let a_moment_later = reset("2026-10-05T12:00:00.692639+00:00");
    assert_ne!(noon, a_moment_later);
    assert!(noon < a_moment_later);
    assert_ne!(noon, reset("2026-10-05T12:00:01Z"));
    assert_eq!(noon.max(a_moment_later), a_moment_later);
}

#[test]
fn reset_instants_two_seconds_apart_are_one_cycle_and_three_seconds_apart_are_later() {
    let noon = reset("2026-10-05T12:00:00Z");
    for (other, same, later) in [
        ("2026-10-05T12:00:00.692639+00:00", true, false),
        ("2026-10-05T12:00:01Z", true, false),
        ("2026-10-05T12:00:02Z", true, false),
        ("2026-10-05T11:59:58Z", true, false),
        ("2026-10-05T12:00:02.999Z", true, false),
        ("2026-10-05T12:00:03Z", false, true),
        ("2026-10-05T11:59:57Z", false, false),
        ("2026-10-12T12:00:00Z", false, true),
    ] {
        let other = reset(other);
        assert_eq!(other.same_cycle_as(noon), same, "{other} same as {noon}");
        assert_eq!(noon.same_cycle_as(other), same, "{noon} same as {other}");
        assert_eq!(other.later_than(noon), later, "{other} later than {noon}");
        assert!(
            !(later && noon.later_than(other)),
            "{noon} later than {other}"
        );
    }
}

#[test]
fn a_reset_instant_has_passed_from_its_own_instant_on() {
    let noon = reset("2026-10-05T12:00:00Z");
    assert!(!noon.passed_by(at("2026-10-05T11:59:59Z")));
    assert!(noon.passed_by(at("2026-10-05T12:00:00Z")));
    assert!(noon.passed_by(at("2026-10-06T00:00:00Z")));
}

#[test]
fn a_cycle_turns_over_on_a_later_reset_or_a_passed_one_once_usage_drops() {
    let seen = |used_percent, reset_at: Option<&str>| Observation {
        used_percent,
        reset: reset_at.map(reset),
    };
    let saved = Some("2026-08-24T12:00:00Z");
    let before_reset = at("2026-08-19T13:00:00Z");
    let after_reset = at("2026-08-24T12:00:00Z");
    for (before, after, observed_at, turned_over, case) in [
        (
            seen(82.0, saved),
            seen(0.0, Some("2026-08-31T12:00:00Z")),
            before_reset,
            true,
            "a later reset",
        ),
        (
            seen(80.0, saved),
            seen(79.0, Some("2026-08-24T12:00:03Z")),
            before_reset,
            true,
            "three seconds later",
        ),
        (
            seen(80.0, saved),
            seen(79.0, Some("2026-08-24T12:00:01Z")),
            before_reset,
            false,
            "a second later",
        ),
        (
            seen(80.0, saved),
            seen(79.0, Some("2026-08-24T11:59:59Z")),
            before_reset,
            false,
            "a second earlier",
        ),
        (
            seen(80.0, saved),
            seen(79.0, Some("2026-08-24T12:00:00.000+00:00")),
            before_reset,
            false,
            "spelled otherwise",
        ),
        (
            seen(80.0, saved),
            seen(8.0, saved),
            before_reset,
            false,
            "a drop with the same reset",
        ),
        (
            seen(50.0, saved),
            seen(49.5, Some("2026-08-24T12:05:00Z")),
            before_reset,
            false,
            "half a point",
        ),
        (
            seen(50.0, saved),
            seen(49.4, Some("2026-08-24T12:05:00Z")),
            before_reset,
            true,
            "past half a point",
        ),
        (
            seen(100.0, saved),
            seen(0.0, None),
            after_reset,
            true,
            "an idle read once the reset passed",
        ),
        (
            seen(100.0, saved),
            seen(0.0, None),
            at("2026-08-24T11:59:59Z"),
            false,
            "an idle read before it",
        ),
        (
            seen(100.0, saved),
            seen(99.5, None),
            after_reset,
            false,
            "the reset passed without a drop",
        ),
        (
            seen(100.0, None),
            seen(0.0, Some("2026-08-31T12:00:00Z")),
            after_reset,
            false,
            "nothing saved to pass",
        ),
    ] {
        assert_eq!(
            cycle_turned_over(before, after, observed_at),
            turned_over,
            "{case}"
        );
    }
}

#[test]
fn every_window_and_share_kept_in_range() {
    let mut reading = Reading {
        windows: [140.0, -3.0, f64::NAN, f64::NEG_INFINITY, 42.5]
            .into_iter()
            .map(|used| fixture("w", "W", LimitWindowKind::Weekly, used, None))
            .collect(),
        workspace_credits: Some(LimitsWorkspaceCreditsDto {
            limit: "100".into(),
            used: "250".into(),
            used_percent: 250.0,
            resets_at: None,
            reached: true,
        }),
        ..Reading::default()
    };

    keep_in_range(&mut reading);

    let used: Vec<f64> = reading
        .windows
        .iter()
        .map(|window| window.used_percent)
        .collect();
    assert_eq!(used, [100.0, 0.0, 0.0, 0.0, 42.5]);
    assert_eq!(
        reading.workspace_credits.map(|share| share.used_percent),
        Some(100.0)
    );
}

#[test]
fn a_label_reads_back_the_scope_it_was_written_for() {
    for scope in [
        Scope::AllModels,
        model("Fable"),
        model("GPT-Reserve"),
        model("Team · GPT-Reserve"),
        model("  gpt-5.3-codex-SPARK "),
    ] {
        for length in [Some(FIVE_HOURS), Some(minutes(30)), Some(ONE_WEEK), None] {
            let label = fact(scope.clone(), length, 1.0).into_dto().label;
            assert_eq!(scope_of_label(&label), Some(scope.clone()), "{label}");
        }
    }
    assert_eq!(scope_of_label("Workspace credits"), None);
}
