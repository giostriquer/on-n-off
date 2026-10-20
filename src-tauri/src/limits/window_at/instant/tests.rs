use super::super::reached;
use super::super::tests::{cases, Case, MeterCase, ShareInput, WindowInput};
use super::*;
use crate::dto::LimitWindowKind;

fn window(input: &WindowInput) -> LimitWindowDto {
    LimitWindowDto {
        id: "w".into(),
        label: "Weekly · all models".into(),
        kind: LimitWindowKind::Weekly,
        used_percent: input.used_percent.unwrap_or(f64::NAN),
        resets_at: input.resets_at.clone(),
        window_seconds: None,
        observed_at: String::new(),
        role: None,
        starts_on_first_message: input.starts_on_first_message,
    }
}

fn share(input: &ShareInput) -> LimitsWorkspaceCreditsDto {
    LimitsWorkspaceCreditsDto {
        limit: "25000".into(),
        used: "8000".into(),
        used_percent: input.used_percent.unwrap_or(f64::NAN),
        resets_at: input.resets_at.clone(),
        reached: false,
    }
}

fn kind(cycle: Cycle) -> &'static str {
    match cycle {
        Cycle::Pending(_) => "pending",
        Cycle::Renewed(_) => "renewed",
        Cycle::Idle => "idle",
        Cycle::Unknown => "unknown",
    }
}

fn when(at: &WindowAt, zone: &chrono_tz::Tz) -> Option<String> {
    match at.cycle {
        Cycle::Pending(reset) | Cycle::Renewed(reset) => Some(at.subject.when(reset, zone)),
        Cycle::Idle | Cycle::Unknown => None,
    }
}

fn meter_matches(meter: Meter, case: &MeterCase) -> bool {
    match (meter, case.level.as_str(), case.fraction) {
        (Meter::Unreadable, "unreadable", None)
        | (Meter::Base, "base", None)
        | (Meter::Trip, "trip", None) => true,
        (Meter::Blend(fraction), "blend", Some(expected)) => (fraction - expected).abs() < 1e-9,
        _ => false,
    }
}

fn assert_case<Input>(case: &Case<Input>, at: WindowAt, zone: &chrono_tz::Tz) {
    let name = &case.name;
    assert_eq!(at.used_now(), case.used_now, "{name}: used now");
    assert_eq!(
        at.used_now().is_some_and(reached),
        case.reached,
        "{name}: reached"
    );
    assert_eq!(at.used_text(), case.used_text, "{name}: used text");
    assert!(
        meter_matches(at.meter(), &case.meter),
        "{name}: meter {:?} is not {} {:?}",
        at.meter(),
        case.meter.level,
        case.meter.fraction
    );
    assert_eq!(kind(at.cycle), case.note.kind, "{name}: cycle");
    assert_eq!(at.renewed(), case.note.kind == "renewed", "{name}: renewed");
    assert_eq!(when(&at, zone), case.note.when, "{name}: reset instant");
    assert_eq!(at.notch_note(zone), case.note.notch, "{name}: notch note");
}

#[test]
fn every_window_case_reads_the_same_at_the_pinned_instant() {
    let cases = cases();
    let zone: chrono_tz::Tz = cases.time_zone.parse().expect("an IANA zone");
    assert!(!cases.windows.is_empty());
    for case in &cases.windows {
        assert_case(case, WindowAt::of(&window(&case.input), cases.now), &zone);
    }
}

#[test]
fn every_share_case_reads_the_same_at_the_pinned_instant() {
    let cases = cases();
    let zone: chrono_tz::Tz = cases.time_zone.parse().expect("an IANA zone");
    assert!(!cases.shares.is_empty());
    for case in &cases.shares {
        assert_case(
            case,
            WindowAt::of_share(&share(&case.input), cases.now),
            &zone,
        );
    }
}
