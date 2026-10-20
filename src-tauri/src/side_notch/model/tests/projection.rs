use super::*;
use crate::dto::{Reading, WindowRole};

fn window(id: &str, label: &str, kind: LimitWindowKind) -> LimitWindowDto {
    LimitWindowDto {
        id: id.into(),
        label: label.into(),
        kind,
        used_percent: 10.0,
        resets_at: None,
        window_seconds: None,
        observed_at: "2026-09-01T10:00:00Z".into(),
        role: None,
        starts_on_first_message: false,
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

fn ring(id: &str, label: &str) -> LimitWindowDto {
    LimitWindowDto {
        role: Some(WindowRole::InnerRing),
        ..model(id, label)
    }
}

fn signed_in(provider: AgentId, windows: Vec<LimitWindowDto>) -> ProviderLimitsDto {
    crate::limits::signed_in_card(
        provider,
        "acct",
        Reading {
            windows,
            ..Reading::default()
        },
    )
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
                ring(fable, "Weekly · Fable"),
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

#[test]
fn the_window_its_reader_gave_the_inner_ring_role_fills_it_whatever_its_label() {
    let renamed = project(signed_in(
        AgentId::Claude,
        vec![
            weekly("weekly_all"),
            ring("weekly_scoped:Fable 5", "Weekly · Fable 5"),
        ],
    ));
    assert_eq!(
        renamed.inner_ring,
        Some(InnerRing::Fable {
            window_id: "weekly_scoped:Fable 5".into()
        })
    );
    let unmarked = project(signed_in(
        AgentId::Claude,
        vec![
            weekly("weekly_all"),
            model("weekly_fable", "Weekly · Fable"),
        ],
    ));
    assert_eq!(unmarked.inner_ring, None);
    let opus = project(signed_in(
        AgentId::Claude,
        vec![weekly("weekly_all"), model("weekly_opus", "Weekly · Opus")],
    ));
    assert_eq!(opus.inner_ring, None);
}

#[test]
fn a_marked_window_wins_the_inner_ring_over_a_workspace_share() {
    for provider in crate::provider_catalogue::WITH_LIMITS {
        let mut card = signed_in(
            provider,
            vec![weekly("weekly"), ring("ringed", "Weekly · Ringed")],
        );
        card.reading.workspace_credits = Some(share("25000", "8000", 32.0, false));
        assert_eq!(
            project(card).inner_ring,
            Some(InnerRing::Fable {
                window_id: "ringed".into()
            }),
            "{provider:?}"
        );
    }
}

#[test]
fn every_ring_leads_with_its_weekly_window_or_with_nothing() {
    let codex_both = signed_in(
        AgentId::Codex,
        vec![weekly("secondary"), session("primary")],
    );
    assert_eq!(headline(codex_both).as_deref(), Some("secondary"));
    let codex_weekly = signed_in(AgentId::Codex, vec![weekly("primary")]);
    assert_eq!(headline(codex_weekly).as_deref(), Some("primary"));
    let codex_session = signed_in(
        AgentId::Codex,
        vec![
            session("primary"),
            model("extra:luna", "Weekly · GPT-5.6-Luna"),
        ],
    );
    assert_eq!(headline(codex_session), None);
    let codex_models = signed_in(
        AgentId::Codex,
        vec![model("extra:luna", "Weekly · GPT-5.6-Luna")],
    );
    assert_eq!(headline(codex_models), None);
    let claude_without_weekly = signed_in(
        AgentId::Claude,
        vec![session("session"), model("weekly_opus", "Weekly · Opus")],
    );
    assert_eq!(headline(claude_without_weekly), None);
    assert_eq!(headline(signed_in(AgentId::Claude, Vec::new())), None);
}

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

#[test]
fn an_account_whose_refresh_is_paused_keeps_its_last_reading() {
    for status in [
        LimitsStatus::Failed,
        LimitsStatus::Unauthenticated,
        LimitsStatus::SignedOut,
        LimitsStatus::Unsupported,
    ] {
        let mut card = signed_in(
            AgentId::Claude,
            vec![weekly("weekly_all"), ring("weekly_fable", "Weekly · Fable")],
        );
        card.status = status;
        card.message = Some("Paused".into());
        card.reading.workspace_credits = Some(share("25000", "8000", 32.0, false));

        let cell = project(card);
        assert_eq!(
            cell.headline_window_id.as_deref(),
            Some("weekly_all"),
            "{status:?}"
        );
        assert_eq!(
            cell.inner_ring,
            Some(InnerRing::Fable {
                window_id: "weekly_fable".into()
            }),
            "{status:?}"
        );
        assert_eq!(cell.windows.len(), 2, "{status:?}");
        assert!(cell.workspace_credits.is_some(), "{status:?}");
        assert_eq!(cell.message.as_deref(), Some("Paused"));
    }

    let mut member = signed_in(AgentId::Codex, vec![weekly("primary")]);
    member.status = LimitsStatus::Failed;
    member.reading.workspace_credits = Some(share("25000", "8000", 32.0, false));
    assert_eq!(project(member).inner_ring, Some(InnerRing::WorkspaceShare));

    let mut session_only = signed_in(AgentId::Codex, vec![session("primary")]);
    session_only.status = LimitsStatus::Failed;
    assert_eq!(project(session_only).headline_window_id, None);
}

#[test]
fn only_the_signed_in_account_reaches_the_notch() {
    let remembered = ProviderLimitsDto {
        current_account: false,
        ..signed_in(AgentId::Codex, vec![weekly("primary")])
    };
    let current = ProviderLimitsDto {
        status: LimitsStatus::SignedOut,
        ..signed_in(AgentId::Codex, vec![session("primary")])
    };

    let cell = NotchProvider::current(vec![remembered.clone(), current]).expect("the current one");
    assert_eq!(cell.windows[0].kind, LimitWindowKind::Session);
    assert_eq!(cell.status, LimitsStatus::SignedOut);
    assert_eq!(NotchProvider::current(vec![remembered]), None);
    assert_eq!(NotchProvider::current(Vec::new()), None);
}

#[test]
fn the_popover_lists_the_windows_weekly_first_as_the_card_orders_them() {
    let cell = project(signed_in(
        AgentId::Claude,
        vec![
            weekly("weekly_all"),
            session("session"),
            model("weekly_fable", "Weekly · Fable"),
            model("weekly_opus", "Weekly · Opus"),
        ],
    ));
    let ids: Vec<&str> = cell
        .windows
        .iter()
        .map(|window| window.id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["weekly_all", "session", "weekly_fable", "weekly_opus"]
    );
}

#[test]
fn a_codex_read_reaches_the_notch_weekly_first_without_its_hidden_windows() {
    let main = serde_json::json!({"limitId": "codex",
        "primary": {"usedPercent": 30, "windowDurationMins": 300, "resetsAt": 1787273137},
        "secondary": {"usedPercent": 60, "windowDurationMins": 10080, "resetsAt": 1787838960}});
    let card = crate::limits::codex_card(
        serde_json::json!({
            "rateLimits": main,
            "rateLimitsByLimitId": {
                "codex": main,
                "codex_bengalfox": {"limitId": "codex_bengalfox",
                    "limitName": "GPT-5.3-Codex-Spark",
                    "primary": {"usedPercent": 99, "windowDurationMins": 300}}
            }
        }),
        "acct",
        "2026-09-01T10:00:00Z",
    );

    let cell = project(card);
    let ids: Vec<&str> = cell
        .windows
        .iter()
        .map(|window| window.id.as_str())
        .collect();
    assert_eq!(ids, ["secondary", "primary"]);
    assert_eq!(cell.headline_window_id.as_deref(), Some("secondary"));
    assert_eq!(cell.inner_ring, None);
}

#[test]
fn the_named_windows_resolve_for_the_painter() {
    let claude = project(signed_in(
        AgentId::Claude,
        vec![
            weekly("weekly_all"),
            session("session"),
            model("weekly_opus", "Weekly · Opus"),
            ring("weekly_fable", "Weekly · Fable"),
        ],
    ));
    assert_eq!(
        claude.headline().map(|window| window.id.as_str()),
        Some("weekly_all")
    );
    let now = "2026-09-01T10:00:00Z".parse().unwrap();
    let (inner, at) = claude.inner_at(now).expect("the Fable ring");
    assert!(matches!(inner, InnerRing::Fable { .. }));
    assert_eq!(
        at,
        WindowAt::of(&ring("weekly_fable", "Weekly · Fable"), now)
    );

    let mut member = signed_in(AgentId::Codex, vec![weekly("primary")]);
    let credits = share("25000", "8000", 32.0, false);
    member.reading.workspace_credits = Some(credits.clone());
    let member = project(member);
    let (inner, at) = member.inner_at(now).expect("the share ring");
    assert_eq!(inner, &InnerRing::WorkspaceShare);
    assert_eq!(at, WindowAt::of_share(&credits, now));
    assert_eq!(at.used_now(), Some(32.0));
}
