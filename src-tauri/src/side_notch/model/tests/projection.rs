//! The notch projection: what each provider cell lists, leads with and shows inside, decided once
//! for the macOS helper and the Windows painter alike.

use super::*;
use crate::dto::Reading;

fn window(id: &str, label: &str, kind: LimitWindowKind) -> LimitWindowDto {
    LimitWindowDto {
        id: id.into(),
        label: label.into(),
        kind,
        used_percent: 10.0,
        resets_at: None,
        window_seconds: None,
        observed_at: "2026-09-01T10:00:00Z".into(),
    }
}

fn weekly(id: &str) -> LimitWindowDto {
    window(id, "Weekly · all models", LimitWindowKind::Weekly)
}

fn session(id: &str) -> LimitWindowDto {
    window(id, "5 hour · all models", LimitWindowKind::Session)
}

fn model(id: &str, label: &str) -> LimitWindowDto {
    window(id, label, LimitWindowKind::Model)
}

/// The signed-in account's card for `provider`, reporting `windows` in the order a card lists them.
fn signed_in(provider: AgentId, windows: Vec<LimitWindowDto>) -> ProviderLimitsDto {
    ProviderLimitsDto::for_test(provider, "acct").with_reading(Reading {
        windows,
        ..Reading::default()
    })
}

fn project(card: ProviderLimitsDto) -> NotchProvider {
    NotchProvider::current(vec![card]).expect("a signed-in account")
}

fn headline(card: ProviderLimitsDto) -> Option<String> {
    project(card).headline_window_id
}

#[test]
fn claudes_ring_leads_with_its_weekly_and_its_fable_window_fills_the_inner_ring() {
    for fable in ["weekly_fable", "weekly_scoped:Fable"] {
        let cell = project(signed_in(
            AgentId::Claude,
            vec![
                weekly("weekly_all"),
                session("session"),
                model(fable, "Weekly · Fable"),
            ],
        ));

        assert_eq!(cell.headline_window_id.as_deref(), Some("weekly_all"));
        assert_eq!(
            cell.inner_ring,
            Some(InnerRing::Fable {
                window_id: fable.into()
            })
        );
    }
}

/// Fable is Claude's window and is known by its label: another model's window, or a window of the
/// same name from another provider, leaves the inner ring empty.
#[test]
fn only_claudes_window_labelled_weekly_fable_fills_the_inner_ring() {
    let opus = project(signed_in(
        AgentId::Claude,
        vec![weekly("weekly_all"), model("weekly_opus", "Weekly · Opus")],
    ));
    assert_eq!(opus.inner_ring, None);
    let codex = project(signed_in(
        AgentId::Codex,
        vec![weekly("primary"), model("extra:fable", "Weekly · Fable")],
    ));
    assert_eq!(codex.inner_ring, None);
    let spaced = project(signed_in(
        AgentId::Claude,
        vec![weekly("weekly_all"), model("fable", " weekly · FABLE ")],
    ));
    assert_eq!(
        spaced.inner_ring,
        Some(InnerRing::Fable {
            window_id: "fable".into()
        })
    );
}

#[test]
fn claude_without_a_weekly_window_leads_with_nothing() {
    let card = signed_in(
        AgentId::Claude,
        vec![session("session"), model("weekly_opus", "Weekly · Opus")],
    );
    assert_eq!(headline(card), None);
}

#[test]
fn codex_leads_with_its_session_when_it_reports_one_else_its_weekly() {
    let both = signed_in(
        AgentId::Codex,
        vec![weekly("secondary"), session("primary")],
    );
    assert_eq!(headline(both).as_deref(), Some("primary"));
    let weekly_only = signed_in(AgentId::Codex, vec![weekly("primary")]);
    assert_eq!(headline(weekly_only).as_deref(), Some("primary"));
    let models_only = signed_in(
        AgentId::Codex,
        vec![model("extra:luna", "Weekly · GPT-5.6-Luna")],
    );
    assert_eq!(headline(models_only), None);
}

/// A business member's credit share takes the inner ring; the weekly window stays the headline, and
/// the share travels with the cell for the popover.
#[test]
fn a_workspace_share_fills_the_inner_ring_while_the_weekly_stays_the_headline() {
    let mut card = signed_in(AgentId::Codex, vec![weekly("primary")]);
    card.reading.workspace_credits = Some(share("25000", "8000", 32.0, false));

    let cell = project(card);
    assert_eq!(cell.headline_window_id.as_deref(), Some("primary"));
    assert_eq!(cell.inner_ring, Some(InnerRing::WorkspaceShare));
    assert_eq!(
        cell.workspace_credits
            .map(|share| (share.limit, share.used_percent)),
        Some(("25000".to_string(), 32.0))
    );
}

/// A ring shows only what a read just answered. An account that could not be read leads with
/// nothing and fills no inner ring, while the popover still lists what it remembers.
#[test]
fn an_account_that_cannot_be_read_leads_with_nothing() {
    for status in [
        LimitsStatus::Failed,
        LimitsStatus::Unauthenticated,
        LimitsStatus::SignedOut,
        LimitsStatus::Unsupported,
    ] {
        let mut card = signed_in(
            AgentId::Claude,
            vec![
                weekly("weekly_all"),
                model("weekly_fable", "Weekly · Fable"),
            ],
        );
        card.status = status;
        card.message = Some("Paused".into());
        card.reading.workspace_credits = Some(share("25000", "8000", 32.0, false));

        let cell = project(card);
        assert_eq!(cell.headline_window_id, None, "{status:?}");
        assert_eq!(cell.inner_ring, None, "{status:?}");
        assert_eq!(cell.windows.len(), 2, "{status:?}");
        assert!(cell.workspace_credits.is_some(), "{status:?}");
        assert_eq!(cell.message.as_deref(), Some("Paused"));
    }
}

#[test]
fn only_the_signed_in_account_reaches_the_notch() {
    let remembered = ProviderLimitsDto {
        current_account: false,
        ..signed_in(AgentId::Codex, vec![weekly("primary")])
    };
    let mut current = signed_in(AgentId::Codex, vec![session("primary")]);
    current.reading.plan = Some("pro".into());

    let cell = NotchProvider::current(vec![remembered.clone(), current]).expect("the current one");
    assert_eq!(cell.plan.as_deref(), Some("pro"));
    assert_eq!(cell.windows[0].kind, LimitWindowKind::Session);
    assert_eq!(NotchProvider::current(vec![remembered]), None);
    assert_eq!(NotchProvider::current(Vec::new()), None);
}

#[test]
fn the_popover_lists_the_session_then_weekly_then_model() {
    let cell = project(signed_in(
        AgentId::Claude,
        vec![
            weekly("weekly_all"),
            session("session"),
            model("weekly_fable", "Weekly · Fable"),
        ],
    ));
    let ids: Vec<&str> = cell
        .windows
        .iter()
        .map(|window| window.id.as_str())
        .collect();
    assert_eq!(ids, ["session", "weekly_all", "weekly_fable"]);
}
