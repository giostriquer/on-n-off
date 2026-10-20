use super::*;
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Cases {
    pub(super) now: DateTime<Utc>,
    pub(super) time_zone: String,
    pub(super) windows: Vec<Case<WindowInput>>,
    pub(super) shares: Vec<Case<ShareInput>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Case<Input> {
    pub(super) name: String,
    #[serde(alias = "window", alias = "share")]
    pub(super) input: Input,
    pub(super) used_now: Option<f64>,
    pub(super) used_figure: Option<u8>,
    pub(super) reached: bool,
    pub(super) used_text: String,
    pub(super) meter: MeterCase,
    pub(super) note: NoteCase,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WindowInput {
    pub(super) used_percent: Option<f64>,
    pub(super) resets_at: Option<String>,
    #[serde(default)]
    pub(super) starts_on_first_message: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ShareInput {
    pub(super) used_percent: Option<f64>,
    pub(super) resets_at: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct MeterCase {
    pub(super) level: String,
    pub(super) fraction: Option<f64>,
}

#[derive(Deserialize)]
pub(super) struct NoteCase {
    pub(super) kind: String,
    pub(super) when: Option<String>,
    pub(super) notch: String,
}

pub(super) fn cases() -> Cases {
    serde_json::from_str(include_str!("fixtures/window-cases.json"))
        .expect("window-cases.json reads")
}

#[test]
fn the_figure_behind_every_percent_text_is_the_one_each_case_names() {
    let cases = cases();
    for case in &cases.windows {
        assert_eq!(
            case.used_now.map(figure_of),
            case.used_figure,
            "{}: figure",
            case.name
        );
        assert_eq!(text_of(case.used_now), case.used_text, "{}", case.name);
    }
}

#[test]
fn no_percent_short_of_the_limit_reads_as_the_limit() {
    let sweep = (0..=200).map(|step| f64::from(step) / 2.0);
    for used in sweep.chain([99.4, 99.5, 99.9]) {
        let figure = figure_of(used);
        assert!(
            figure <= 99 || used >= 100.0,
            "{used}% reads as figure {figure}"
        );
        if used >= 1.0 {
            assert_eq!(text_of(Some(used)), format!("{figure}%"), "{used}%");
        }
    }
}

#[test]
fn the_monitor_reads_a_raw_percent_by_the_rules_every_surface_draws() {
    let cases = cases();
    for case in &cases.windows {
        let Some(used_percent) = case
            .input
            .used_percent
            .filter(|_| case.note.kind != "renewed")
        else {
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
    assert_eq!(used_text(f64::NAN), UNREADABLE);
    assert!(!reached(f64::NAN));
}
