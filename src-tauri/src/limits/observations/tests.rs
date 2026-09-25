use super::*;
use crate::dto::{AgentId, LimitWindowKind, LimitsStatus, ProviderLimitsDto, Reading};
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

/// `provider`'s signed-in `acct-1`, on a read that paused with `reading`.
fn paused(provider: AgentId, reading: Reading) -> ProviderLimitsDto {
    ProviderLimitsDto {
        status: LimitsStatus::Failed,
        message: Some("Refresh paused".to_string()),
        ..ProviderLimitsDto::for_test(provider, "acct-1")
            .labelled("me@example.com")
            .with_reading(reading)
    }
}

/// The same account remembered with `reading`.
fn remembered(provider: AgentId, reading: Reading) -> Option<ObservedWindowSet> {
    ObservedWindowSet::from_account(ProviderLimitsDto {
        current_account: false,
        ..ProviderLimitsDto::for_test(provider, "acct-1")
            .labelled("me@example.com")
            .with_reading(reading)
    })
}

#[test]
fn newer_windows_merge_independently_and_do_not_inherit_an_old_reset() {
    let current = ProviderLimitsDto {
        status: LimitsStatus::Unauthenticated,
        ..paused(
            AgentId::Claude,
            Reading {
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
            },
        )
    };
    let remembered = remembered(
        AgentId::Claude,
        Reading {
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
        },
    );

    let merged = merge_windows(current, remembered).reading;
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
    let remembered = remembered(
        AgentId::Codex,
        Reading {
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
        },
    );

    assert_eq!(
        merge_windows(paused(AgentId::Codex, Reading::default()), remembered)
            .reading
            .reset_credits,
        reset_credits
    );
}

#[test]
fn a_paused_refresh_keeps_banked_resets_remembered_without_any_windows() {
    let reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 2,
        next_expires_at: None,
    });
    let remembered = remembered(
        AgentId::Codex,
        Reading {
            plan: Some("pro".to_string()),
            reset_credits: reset_credits.clone(),
            ..Reading::default()
        },
    );

    let merged = merge_windows(paused(AgentId::Codex, Reading::default()), remembered).reading;

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
    let read = |workspace_credits| {
        paused(
            AgentId::Codex,
            Reading {
                workspace_credits,
                ..Reading::default()
            },
        )
    };
    let remembered = || {
        remembered(
            AgentId::Codex,
            Reading {
                workspace_credits: share("8000"),
                ..Reading::default()
            },
        )
    };

    let paused = merge_windows(read(None), remembered()).reading;
    let answered = merge_windows(read(share("9000")), remembered()).reading;

    assert_eq!(paused.workspace_credits, share("8000"));
    assert_eq!(answered.workspace_credits, share("9000"));
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
    let read = |credits_spent| {
        paused(
            AgentId::Codex,
            Reading {
                credits_spent,
                ..Reading::default()
            },
        )
    };
    let remembered = || {
        remembered(
            AgentId::Codex,
            Reading {
                credits_spent: spent(100.0),
                ..Reading::default()
            },
        )
    };

    let paused = merge_windows(read(None), remembered()).reading;
    let answered = merge_windows(read(spent(250.0)), remembered()).reading;

    assert_eq!(paused.credits_spent, spent(100.0));
    assert_eq!(answered.credits_spent, spent(250.0));
}

/// The subscription status is account metadata, like the plan: a read that could not say keeps the
/// remembered one, and a read that did say wins.
#[test]
fn a_paused_refresh_keeps_the_remembered_subscription_status() {
    let read = |subscription_status: Option<&str>| {
        paused(
            AgentId::Claude,
            Reading {
                subscription_status: subscription_status.map(str::to_string),
                ..Reading::default()
            },
        )
    };
    let remembered = || {
        remembered(
            AgentId::Claude,
            Reading {
                subscription_status: Some("past_due".to_string()),
                windows: vec![observed(
                    "seven_day",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    40.0,
                    None,
                    "2026-08-17T10:00:00.000Z",
                )],
                ..Reading::default()
            },
        )
    };

    let paused = merge_windows(read(None), remembered()).reading;
    let answered = merge_windows(read(Some("active")), remembered()).reading;

    assert_eq!(paused.subscription_status.as_deref(), Some("past_due"));
    assert_eq!(answered.subscription_status.as_deref(), Some("active"));
}
