use super::*;
use crate::dto::{LimitWindowDto, Reading};
use crate::limits::window::fixture;

fn card(windows: Vec<LimitWindowDto>) -> ProviderLimitsDto {
    finish(
        AgentId::Codex,
        LimitsStatus::Ok,
        None,
        Parsed {
            account: None,
            reading: Reading {
                windows,
                ..Reading::default()
            },
        },
    )
}

fn ids(card: &ProviderLimitsDto) -> Vec<&str> {
    card.reading
        .windows
        .iter()
        .map(|window| window.id.as_str())
        .collect()
}

#[test]
fn a_card_lists_its_windows_weekly_then_session_then_model_whatever_order_they_came_in() {
    let weekly = fixture(
        "weekly",
        "Weekly · all models",
        LimitWindowKind::Weekly,
        10.0,
        None,
    );
    let session = fixture(
        "session",
        "5 hour · all models",
        LimitWindowKind::Session,
        20.0,
        None,
    );
    let model = fixture(
        "fable",
        "Weekly · Fable",
        LimitWindowKind::Model,
        30.0,
        None,
    );
    for order in [
        [&weekly, &session, &model],
        [&weekly, &model, &session],
        [&session, &weekly, &model],
        [&session, &model, &weekly],
        [&model, &weekly, &session],
        [&model, &session, &weekly],
    ] {
        let answered: Vec<LimitWindowDto> = order.into_iter().cloned().collect();
        let from: Vec<String> = answered.iter().map(|window| window.id.clone()).collect();
        assert_eq!(
            ids(&card(answered)),
            ["weekly", "session", "fable"],
            "answered as {from:?}"
        );
    }
}

#[test]
fn windows_of_one_kind_keep_the_providers_order() {
    let card = card(vec![
        fixture("opus", "Weekly · Opus", LimitWindowKind::Model, 1.0, None),
        fixture(
            "weekly",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            2.0,
            None,
        ),
        fixture("fable", "Weekly · Fable", LimitWindowKind::Model, 3.0, None),
    ]);
    assert_eq!(ids(&card), ["weekly", "opus", "fable"]);
}

#[test]
fn every_window_and_share_leaves_in_range() {
    let mut card = card(vec![
        fixture("over", "Over", LimitWindowKind::Weekly, 140.0, None),
        fixture("under", "Under", LimitWindowKind::Session, -3.0, None),
        fixture("lost", "Lost", LimitWindowKind::Model, f64::NAN, None),
    ]);
    let used: Vec<f64> = card
        .reading
        .windows
        .iter()
        .map(|window| window.used_percent)
        .collect();
    assert_eq!(used, [100.0, 0.0, 0.0]);

    card = finish(
        AgentId::Codex,
        LimitsStatus::Ok,
        None,
        Parsed {
            account: None,
            reading: Reading {
                workspace_credits: Some(crate::dto::LimitsWorkspaceCreditsDto {
                    limit: "100".into(),
                    used: "250".into(),
                    used_percent: 250.0,
                    resets_at: None,
                    reached: true,
                }),
                ..Reading::default()
            },
        },
    );
    assert_eq!(
        card.reading
            .workspace_credits
            .map(|share| share.used_percent),
        Some(100.0)
    );
}

#[test]
fn every_window_leaves_with_an_observation_time() {
    let mut dated = fixture("dated", "Dated", LimitWindowKind::Weekly, 1.0, None);
    dated.observed_at = "2026-08-17T10:00:00.000Z".into();
    let undated = fixture("undated", "Undated", LimitWindowKind::Session, 2.0, None);
    assert!(undated.observed_at.is_empty());

    let card = card(vec![dated, undated]);

    assert_eq!(
        card.reading.windows[0].observed_at,
        "2026-08-17T10:00:00.000Z"
    );
    assert!(
        chrono::DateTime::parse_from_rfc3339(&card.reading.windows[1].observed_at).is_ok(),
        "{}",
        card.reading.windows[1].observed_at
    );
}
