use super::*;
use crate::dto::{AgentId, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto};
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

#[test]
fn newer_windows_merge_independently_and_do_not_inherit_an_old_reset() {
    let current = ProviderLimitsDto {
        provider: AgentId::Claude,
        status: LimitsStatus::Unauthenticated,
        message: Some("Refresh paused".to_string()),
        account: Some(LimitsAccountDto {
            legacy_id: None,
            id: "uuid-1".to_string(),
            label: Some("me@example.com".to_string()),
        }),
        current_account: true,
        plan: None,
        subscription_status: None,
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
        credits: None,
        workspace_credits: None,
        credits_spent: None,
        subscription: None,
        reset_credits: None,
        reset_offer: None,
    };
    let remembered = ObservedWindowSet::from_account(ProviderLimitsDto {
        provider: AgentId::Claude,
        status: LimitsStatus::Ok,
        message: None,
        account: current.account.clone(),
        current_account: false,
        plan: Some("max".to_string()),
        subscription_status: None,
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
        credits: None,
        workspace_credits: None,
        credits_spent: None,
        subscription: None,
        reset_credits: None,
        reset_offer: None,
    });

    let merged = merge_windows(current, remembered);
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
    let account = Some(LimitsAccountDto {
        legacy_id: None,
        id: "acct-1".to_string(),
        label: Some("me@example.com".to_string()),
    });
    let current = ProviderLimitsDto {
        provider: AgentId::Codex,
        status: LimitsStatus::Failed,
        message: Some("Refresh paused".to_string()),
        account: account.clone(),
        current_account: true,
        plan: None,
        subscription_status: None,
        windows: Vec::new(),
        credits: None,
        workspace_credits: None,
        credits_spent: None,
        subscription: None,
        reset_credits: None,
        reset_offer: None,
    };
    let reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: None,
    });
    let remembered = ObservedWindowSet::from_account(ProviderLimitsDto {
        provider: AgentId::Codex,
        status: LimitsStatus::Ok,
        message: None,
        account,
        current_account: false,
        plan: Some("pro".to_string()),
        subscription_status: None,
        windows: vec![observed(
            "primary",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            40.0,
            None,
            "2026-08-17T15:00:00Z",
        )],
        credits: None,
        workspace_credits: None,
        credits_spent: None,
        subscription: None,
        reset_credits: reset_credits.clone(),
        // A remembered account can carry no offer, and merging must not invent one either.
        reset_offer: Some(crate::dto::LimitsResetOfferDto {
            price: Some(crate::dto::LimitsPriceDto {
                amount_minor_units: 800,
                currency: "USD".to_string(),
            }),
        }),
    });

    assert_eq!(
        merge_windows(current, remembered).reset_credits,
        reset_credits
    );
}

#[test]
fn a_paused_refresh_keeps_banked_resets_remembered_without_any_windows() {
    let account = Some(LimitsAccountDto {
        legacy_id: None,
        id: "acct-1".to_string(),
        label: Some("me@example.com".to_string()),
    });
    let current = ProviderLimitsDto {
        provider: AgentId::Codex,
        status: LimitsStatus::Failed,
        message: Some("Refresh paused".to_string()),
        account: account.clone(),
        current_account: true,
        plan: None,
        subscription_status: None,
        windows: Vec::new(),
        credits: None,
        workspace_credits: None,
        credits_spent: None,
        subscription: None,
        reset_credits: None,
        reset_offer: None,
    };
    let reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 2,
        next_expires_at: None,
    });
    let remembered = ObservedWindowSet::from_account(ProviderLimitsDto {
        provider: AgentId::Codex,
        status: LimitsStatus::Ok,
        message: None,
        account,
        current_account: false,
        plan: Some("pro".to_string()),
        subscription_status: None,
        windows: Vec::new(),
        credits: None,
        workspace_credits: None,
        credits_spent: None,
        subscription: None,
        reset_credits: reset_credits.clone(),
        reset_offer: None,
    });

    let merged = merge_windows(current, remembered);

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
    let account = Some(LimitsAccountDto {
        legacy_id: None,
        id: "acct-1".to_string(),
        label: Some("me@example.com".to_string()),
    });
    let share = |used: &str| {
        Some(crate::dto::LimitsWorkspaceCreditsDto {
            limit: "25000".to_string(),
            used: used.to_string(),
            used_percent: 32.0,
            resets_at: None,
            reached: false,
        })
    };
    let read = |workspace_credits| ProviderLimitsDto {
        provider: AgentId::Codex,
        status: LimitsStatus::Failed,
        message: Some("Refresh paused".to_string()),
        account: account.clone(),
        current_account: true,
        plan: None,
        subscription_status: None,
        windows: Vec::new(),
        credits: None,
        workspace_credits,
        credits_spent: None,
        subscription: None,
        reset_credits: None,
        reset_offer: None,
    };
    let remembered = || {
        ObservedWindowSet::from_account(ProviderLimitsDto {
            status: LimitsStatus::Ok,
            message: None,
            current_account: false,
            ..read(share("8000"))
        })
    };

    let paused = merge_windows(read(None), remembered());
    let answered = merge_windows(read(share("9000")), remembered());

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
    let read = |credits_spent| ProviderLimitsDto {
        provider: AgentId::Codex,
        status: LimitsStatus::Failed,
        message: Some("Refresh paused".to_string()),
        account: Some(LimitsAccountDto {
            id: "acct-1".to_string(),
            legacy_id: None,
            label: None,
        }),
        current_account: true,
        plan: None,
        subscription_status: None,
        windows: Vec::new(),
        credits: None,
        workspace_credits: None,
        credits_spent,
        subscription: None,
        reset_credits: None,
        reset_offer: None,
    };
    let remembered = || {
        ObservedWindowSet::from_account(ProviderLimitsDto {
            status: LimitsStatus::Ok,
            message: None,
            current_account: false,
            ..read(spent(100.0))
        })
    };

    let paused = merge_windows(read(None), remembered());
    let answered = merge_windows(read(spent(250.0)), remembered());

    assert_eq!(paused.credits_spent, spent(100.0));
    assert_eq!(answered.credits_spent, spent(250.0));
}

/// The subscription status is account metadata, like the plan: a read that could not say keeps the
/// remembered one, and a read that did say wins.
#[test]
fn a_paused_refresh_keeps_the_remembered_subscription_status() {
    let read = |subscription_status: Option<&str>| ProviderLimitsDto {
        provider: AgentId::Claude,
        status: LimitsStatus::Failed,
        message: Some("Refresh paused".to_string()),
        account: Some(LimitsAccountDto {
            legacy_id: None,
            id: "acct-1".to_string(),
            label: Some("me@example.com".to_string()),
        }),
        current_account: true,
        plan: None,
        subscription_status: subscription_status.map(str::to_string),
        windows: Vec::new(),
        credits: None,
        workspace_credits: None,
        credits_spent: None,
        subscription: None,
        reset_credits: None,
        reset_offer: None,
    };
    let remembered = || {
        ObservedWindowSet::from_account(ProviderLimitsDto {
            status: LimitsStatus::Ok,
            message: None,
            current_account: false,
            windows: vec![observed(
                "seven_day",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                40.0,
                None,
                "2026-08-17T10:00:00.000Z",
            )],
            ..read(Some("past_due"))
        })
    };

    let paused = merge_windows(read(None), remembered());
    let answered = merge_windows(read(Some("active")), remembered());

    assert_eq!(paused.subscription_status.as_deref(), Some("past_due"));
    assert_eq!(answered.subscription_status.as_deref(), Some("active"));
}
