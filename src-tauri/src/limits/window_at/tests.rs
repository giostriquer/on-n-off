use super::*;
use crate::dto::LimitWindowKind;
use serde::Deserialize;

const CASES: &str = include_str!("fixtures/window-cases.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cases {
    now: DateTime<Utc>,
    time_zone: String,
    windows: Vec<Case<WindowInput>>,
    shares: Vec<Case<ShareInput>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case<Input> {
    name: String,
    #[serde(alias = "window", alias = "share")]
    input: Input,
    used_now: Option<f64>,
    renewed: bool,
    reached: bool,
    #[serde(default)]
    idle: bool,
    used_text: String,
    meter: MeterCase,
    note: NoteCase,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowInput {
    used_percent: Option<f64>,
    resets_at: Option<String>,
    #[serde(default)]
    starts_on_first_message: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShareInput {
    used_percent: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Deserialize)]
struct MeterCase {
    level: String,
    fraction: Option<f64>,
}

#[derive(Deserialize)]
struct NoteCase {
    kind: String,
    at: Option<String>,
    notch: String,
}

fn cases() -> Cases {
    serde_json::from_str(CASES).expect("window-cases.json reads")
}

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

fn kind_name(kind: NoteKind) -> &'static str {
    match kind {
        NoteKind::Pending => "pending",
        NoteKind::Elapsed => "elapsed",
        NoteKind::Idle => "idle",
        NoteKind::Unknown => "unknown",
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
    assert_eq!(at.renewed(), case.renewed, "{name}: renewed");
    assert_eq!(
        at.used_now().is_some_and(reached),
        case.reached,
        "{name}: reached"
    );
    assert_eq!(at.note_kind() == NoteKind::Idle, case.idle, "{name}: idle");
    assert_eq!(at.used_text(), case.used_text, "{name}: used text");
    assert!(
        meter_matches(at.meter(), &case.meter),
        "{name}: meter {:?} is not {} {:?}",
        at.meter(),
        case.meter.level,
        case.meter.fraction
    );
    assert_eq!(
        kind_name(at.note_kind()),
        case.note.kind,
        "{name}: note kind"
    );
    assert_eq!(at.reset_at(zone), case.note.at, "{name}: reset instant");
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

#[test]
fn the_monitor_reads_a_raw_figure_by_the_same_rules() {
    let cases = cases();
    for case in &cases.windows {
        let Some(used_percent) = case.input.used_percent.filter(|_| !case.renewed) else {
            continue;
        };
        assert_eq!(
            used_text(used_percent),
            case.used_text,
            "{}: used text",
            case.name
        );
        assert_eq!(
            reached(used_percent),
            case.reached,
            "{}: reached",
            case.name
        );
    }
    assert_eq!(used_text(f64::NAN), "—");
    assert!(!reached(f64::NAN));
}
