use super::*;
use crate::dto::{AgentId, LimitWindowKind, LimitsSubscriptionDto};
use crate::limits::json::window;

fn observed(
    id: &str,
    label: &str,
    kind: LimitWindowKind,
    used_percent: f64,
    resets_at: Option<&str>,
    observed_at: &str,
) -> LimitWindowDto {
    LimitWindowDto {
        observed_at: observed_at.to_string(),
        ..window(id, label, kind, used_percent, resets_at.map(str::to_string))
    }
}

/// What a paused read shows of `remembered` when it read `own`.
fn paused(own: Reading, remembered: Reading) -> Reading {
    own.keeping(remembered, Outcome::Failed)
}

#[test]
fn newer_windows_merge_independently_and_do_not_inherit_an_old_reset() {
    let own = Reading {
        windows: vec![
            observed(
                "weekly_all",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                63.0,
                None,
                "2026-08-18T03:07:53.000Z",
            ),
            observed(
                "session",
                "5 hour · all models",
                LimitWindowKind::Session,
                17.0,
                None,
                "2026-08-18T03:07:53.000Z",
            ),
        ],
        ..Reading::default()
    };
    let remembered = Reading {
        plan: Some("max".to_string()),
        windows: vec![
            observed(
                "weekly_all",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                39.0,
                Some("2026-08-24T12:00:00Z"),
                "2026-08-17T15:00:00Z",
            ),
            observed(
                "weekly_opus",
                "Weekly · Opus",
                LimitWindowKind::Model,
                91.0,
                Some("2026-08-24T12:00:00Z"),
                "2026-08-17T15:00:00Z",
            ),
        ],
        ..Reading::default()
    };

    let merged = paused(own, remembered);
    let summary: Vec<(&str, f64, Option<&str>, &str)> = merged
        .windows
        .iter()
        .map(|window| {
            (
                window.id.as_str(),
                window.used_percent,
                window.resets_at.as_deref(),
                window.observed_at.as_str(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("weekly_all", 63.0, None, "2026-08-18T03:07:53.000Z"),
            ("session", 17.0, None, "2026-08-18T03:07:53.000Z"),
            (
                "weekly_opus",
                91.0,
                Some("2026-08-24T12:00:00Z"),
                "2026-08-17T15:00:00Z"
            ),
        ]
    );
    assert_eq!(merged.plan.as_deref(), Some("max"));
}

#[test]
fn a_paused_refresh_keeps_the_remembered_reset_credit_count() {
    let reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: None,
    });
    let remembered = Reading {
        plan: Some("pro".to_string()),
        windows: vec![observed(
            "primary",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            40.0,
            None,
            "2026-08-17T15:00:00Z",
        )],
        reset_credits: reset_credits.clone(),
        // A remembered account can carry no offer, and merging must not invent one either.
        reset_offer: Some(crate::dto::LimitsResetOfferDto {
            price: Some(crate::dto::LimitsPriceDto {
                amount_minor_units: 800,
                currency: "USD".to_string(),
            }),
        }),
        ..Reading::default()
    };

    assert_eq!(
        paused(Reading::default(), remembered).reset_credits,
        reset_credits
    );
}

#[test]
fn a_paused_refresh_keeps_banked_resets_remembered_without_any_windows() {
    let reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 2,
        next_expires_at: None,
    });
    let remembered = Reading {
        plan: Some("pro".to_string()),
        reset_credits: reset_credits.clone(),
        ..Reading::default()
    };

    let merged = paused(Reading::default(), remembered);

    assert_eq!(merged.reset_credits, reset_credits);
    // The banked count is remembered; a price the provider may already have withdrawn is not.
    assert_eq!(merged.reset_offer, None);
    assert_eq!(merged.plan.as_deref(), Some("pro"));
    assert!(merged.windows.is_empty());
}

/// A read that paused keeps the workspace-credit share the account last reported, as it keeps a
/// credit balance; a read that reports one replaces it.
#[test]
fn a_paused_refresh_keeps_the_remembered_workspace_credit_share() {
    let share = |used: &str| {
        Some(crate::dto::LimitsWorkspaceCreditsDto {
            limit: "25000".to_string(),
            used: used.to_string(),
            used_percent: 32.0,
            resets_at: None,
            reached: false,
        })
    };
    let read = |workspace_credits| Reading {
        workspace_credits,
        ..Reading::default()
    };

    assert_eq!(
        paused(read(None), read(share("8000"))).workspace_credits,
        share("8000")
    );
    assert_eq!(
        paused(read(share("9000")), read(share("8000"))).workspace_credits,
        share("9000")
    );
}

/// Spending is kept through a paused refresh like the other figures, and a read that answered wins.
#[test]
fn a_paused_refresh_keeps_the_remembered_credits_spent() {
    let spent = |last_7_days: f64| {
        Some(crate::dto::LimitsCreditsSpentDto {
            last_7_days,
            last_30_days: last_7_days,
            updated_at: None,
        })
    };
    let read = |credits_spent| Reading {
        credits_spent,
        ..Reading::default()
    };

    assert_eq!(
        paused(read(None), read(spent(100.0))).credits_spent,
        spent(100.0)
    );
    assert_eq!(
        paused(read(spent(250.0)), read(spent(100.0))).credits_spent,
        spent(250.0)
    );
}

/// The subscription status is account metadata, like the plan: a read that could not say keeps the
/// remembered one, and a read that did say wins.
#[test]
fn a_paused_refresh_keeps_the_remembered_subscription_status() {
    let read = |subscription_status: Option<&str>| Reading {
        subscription_status: subscription_status.map(str::to_string),
        ..Reading::default()
    };
    let remembered = || Reading {
        windows: vec![observed(
            "seven_day",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            40.0,
            None,
            "2026-08-17T10:00:00.000Z",
        )],
        ..read(Some("past_due"))
    };

    assert_eq!(
        paused(read(None), remembered())
            .subscription_status
            .as_deref(),
        Some("past_due")
    );
    assert_eq!(
        paused(read(Some("active")), remembered())
            .subscription_status
            .as_deref(),
        Some("active")
    );
}

/// A Codex card is asked about its term, so a successful read that could not tell it keeps the
/// remembered one and one that answered replaces it; a Claude card is never asked and keeps none.
#[test]
fn a_card_keeps_the_term_it_remembers_when_a_read_could_not_tell() {
    let term = |will_renew| {
        Some(LimitsSubscriptionDto {
            active_until: "2026-09-28T16:22:34Z".to_string(),
            will_renew,
            note: None,
            checked_at: "2026-09-25T12:00:00Z".to_string(),
        })
    };
    let card = |provider, subscription| {
        ProviderLimitsDto::for_test(provider, "acct-1").with_reading(Reading {
            plan: Some("team".into()),
            subscription,
            ..Reading::default()
        })
    };
    let remembered = || card(AgentId::Codex, term(false)).reading;

    let mut silent = card(AgentId::Codex, None);
    keep_remembered(&mut silent, remembered());
    assert_eq!(silent.reading.subscription, term(false));

    let mut answered = card(AgentId::Codex, term(true));
    keep_remembered(&mut answered, remembered());
    assert_eq!(answered.reading.subscription, term(true));

    let mut claude = card(AgentId::Claude, None);
    keep_remembered(&mut claude, remembered());
    assert_eq!(claude.reading.subscription, None);
}
