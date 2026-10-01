mod archived;
mod memory;
mod remembered_reading;

use super::*;
use crate::dto::{
    LimitWindowDto, LimitWindowKind, LimitsCreditsDto, LimitsPriceDto, LimitsResetCreditsDto,
    LimitsResetOfferDto, LimitsWorkspaceCreditsDto,
};
use crate::paths::scratch_dir;
use json::window;
use serde_json::json;
use std::fs;

#[test]
fn provider_read_guards_serialize_only_the_same_provider() {
    let codex = provider_read_guard(AgentId::Codex);
    assert!(provider_read_lock(AgentId::Codex).try_lock().is_err());
    assert!(!std::ptr::eq(
        provider_read_lock(AgentId::Codex),
        provider_read_lock(AgentId::Claude)
    ));
    drop(codex);
    assert!(provider_read_lock(AgentId::Codex).try_lock().is_ok());
}

pub(super) fn account(id: &str, label: &str) -> LimitsAccountDto {
    LimitsAccountDto {
        legacy_id: None,
        id: id.to_string(),
        label: Some(label.to_string()),
    }
}

#[test]
fn without_a_user_home_a_read_says_it_cannot_reach_the_login() {
    let dtos = read_limits(AgentId::Cursor, false);
    assert_eq!(dtos.len(), 1);
    assert_eq!(dtos[0].provider, AgentId::Cursor);
    assert_eq!(dtos[0].status, LimitsStatus::Failed);
    assert_eq!(
        dtos[0].message.as_deref(),
        Some("Could not read the stored login: a test build has no user home")
    );
}

#[test]
fn providers_without_a_subscription_are_unsupported() {
    let home = scratch_dir("limits-unsupported");
    for provider in [AgentId::Cursor, AgentId::Antigravity] {
        let dtos = read_limits_at(provider, false, &home);
        assert_eq!(dtos.len(), 1);
        assert_eq!(dtos[0].provider, provider);
        assert_eq!(dtos[0].status, LimitsStatus::Unsupported);
        assert!(dtos[0].message.is_some());
    }
}

#[test]
fn dto_serializes_with_the_camel_case_wire_shape_the_ui_expects() {
    let ok = ProviderLimitsDto::for_test(AgentId::Codex, "acct-1")
        .labelled("me@example.com")
        .with_reading(Reading {
            plan: Some("pro".to_string()),
            windows: vec![LimitWindowDto {
                observed_at: "2026-08-17T20:00:00.000Z".to_string(),
                ..window(
                    "primary",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    2.5,
                    None,
                )
            }],
            credits: Some(LimitsCreditsDto {
                balance: "3".to_string(),
                unlimited: false,
            }),
            workspace_credits: Some(LimitsWorkspaceCreditsDto {
                limit: "25000".to_string(),
                used: "8000".to_string(),
                used_percent: 32.0,
                resets_at: Some("2026-10-01T12:00:00+00:00".to_string()),
                reached: true,
            }),
            credits_spent: Some(crate::dto::LimitsCreditsSpentDto {
                last_7_days: 18303.4,
                last_30_days: 20299.7,
                updated_at: Some("2026-09-24T19:00:00Z".to_string()),
            }),
            subscription: Some(crate::dto::LimitsSubscriptionDto {
                active_until: "2026-09-28T16:22:34Z".to_string(),
                will_renew: false,
                note: Some(crate::dto::SubscriptionNote::Cancelled),
                checked_at: "2026-09-25T12:00:00Z".to_string(),
            }),
            reset_credits: Some(LimitsResetCreditsDto {
                available_count: 1,
                next_expires_at: Some("2026-09-01T12:00:00+00:00".to_string()),
                resets: vec![crate::dto::LimitsBankedResetDto {
                    title: Some("Full reset".to_string()),
                    expires_at: Some("2026-09-01T12:00:00+00:00".to_string()),
                }],
            }),
            reset_offer: Some(LimitsResetOfferDto {
                price: Some(LimitsPriceDto {
                    amount_minor_units: 800,
                    currency: "USD".to_string(),
                }),
            }),
        });
    assert_eq!(
        serde_json::to_value(&ok).unwrap(),
        json!({
            "provider": "codex",
            "status": "ok",
            "account": {"id": "acct-1", "label": "me@example.com"},
            "currentAccount": true,
            "plan": "pro",
            "windows": [{"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 2.5, "observedAt": "2026-08-17T20:00:00.000Z"}],
            "credits": {"balance": "3", "unlimited": false},
            "workspaceCredits": {"limit": "25000", "used": "8000", "usedPercent": 32.0, "resetsAt": "2026-10-01T12:00:00+00:00", "reached": true},
            "creditsSpent": {"last7Days": 18303.4, "last30Days": 20299.7, "updatedAt": "2026-09-24T19:00:00Z"},
            "subscription": {"activeUntil": "2026-09-28T16:22:34Z", "willRenew": false, "note": "cancelled", "checkedAt": "2026-09-25T12:00:00Z"},
            "resetCredits": {"availableCount": 1, "nextExpiresAt": "2026-09-01T12:00:00+00:00", "resets": [{"title": "Full reset", "expiresAt": "2026-09-01T12:00:00+00:00"}]},
            "resetOffer": {"price": {"amountMinorUnits": 800, "currency": "USD"}}
        })
    );
    let signed_out = finish(
        AgentId::Claude,
        LimitsStatus::SignedOut,
        Some("Sign in".to_string()),
        Parsed::default(),
    );
    let value = serde_json::to_value(&signed_out).unwrap();
    assert_eq!(value["status"], "signedOut");
    assert_eq!(value["message"], "Sign in");
    assert_eq!(value["windows"], json!([]));
    assert!(value.get("plan").is_none());
    assert!(value.get("credits").is_none());
    assert!(value.get("workspaceCredits").is_none());
    assert!(value.get("creditsSpent").is_none());
    assert!(value.get("subscription").is_none());
    assert!(value.get("resetCredits").is_none());
    assert!(value.get("resetOffer").is_none());
    assert!(value.get("account").is_none());
    assert_eq!(value["currentAccount"], true);
}

#[test]
fn only_a_saved_profiles_card_carries_the_saved_profile_key() {
    let remembered = ProviderLimitsDto {
        current_account: false,
        ..ProviderLimitsDto::for_test(AgentId::Codex, "profile:acct-1")
    };
    let saved = ProviderLimitsDto {
        saved_profile: true,
        ..remembered.clone()
    };

    let value = serde_json::to_value(&saved).unwrap();
    assert_eq!(value["savedProfile"], json!(true));
    assert_eq!(
        serde_json::from_value::<ProviderLimitsDto>(value).unwrap(),
        saved
    );
    let value = serde_json::to_value(&remembered).unwrap();
    assert!(value.get("savedProfile").is_none(), "{value}");
    assert!(
        !serde_json::from_value::<ProviderLimitsDto>(value)
            .unwrap()
            .saved_profile
    );
}

#[test]
fn only_an_archived_card_carries_the_archived_key() {
    let remembered = ProviderLimitsDto {
        current_account: false,
        ..ProviderLimitsDto::for_test(AgentId::Codex, "profile:acct-1")
    };
    let archived = ProviderLimitsDto {
        archived: true,
        ..remembered.clone()
    };

    let value = serde_json::to_value(&archived).unwrap();
    assert_eq!(value["archived"], json!(true));
    assert_eq!(
        serde_json::from_value::<ProviderLimitsDto>(value).unwrap(),
        archived
    );
    let value = serde_json::to_value(&remembered).unwrap();
    assert!(value.get("archived").is_none(), "{value}");
    assert!(
        !serde_json::from_value::<ProviderLimitsDto>(value)
            .unwrap()
            .archived
    );
}

#[test]
fn every_subscription_note_keeps_its_wire_name() {
    use crate::dto::SubscriptionNote;
    for (note, wire) in [
        (SubscriptionNote::Cancelled, "cancelled"),
        (SubscriptionNote::PlanChange, "planChange"),
        (SubscriptionNote::PastDue, "pastDue"),
    ] {
        assert_eq!(serde_json::to_value(note).unwrap(), serde_json::json!(wire));
        assert_eq!(
            serde_json::from_value::<SubscriptionNote>(serde_json::json!(wire)).unwrap(),
            note
        );
    }
}

#[test]
#[ignore = "real-home network probe; not part of CI"]
fn probe_real_home_limits() {
    #[cfg(target_os = "macos")]
    crate::accounts::with_real_keychain(print_real_home_limits);
    #[cfg(not(target_os = "macos"))]
    print_real_home_limits();
}

fn print_real_home_limits() {
    let home = crate::paths::probe_home();
    for provider in [AgentId::Claude, AgentId::Codex] {
        for dto in read_limits_at(provider, false, &home) {
            println!(
                    "{:?}: current_account={} account={:?} status={:?} plan={:?} message={:?} credits={:?}",
                    provider,
                    dto.current_account,
                    dto.account,
                    dto.status,
                    dto.reading.plan,
                    dto.message,
                    dto.reading.credits
                );
            for window in &dto.reading.windows {
                println!(
                    "  [{:?}] {} ({}) used={}% resets_at={:?} observed_at={}",
                    window.kind,
                    window.label,
                    window.id,
                    window.used_percent,
                    window.resets_at,
                    window.observed_at
                );
            }
        }
    }
}
