mod archived;
mod credits_spent;
mod memory;
mod remembered_reading;
mod renewal;

use super::*;
use crate::dto::{
    LimitWindowDto, LimitWindowKind, LimitsCreditsDto, LimitsPriceDto, LimitsResetCreditsDto,
    LimitsResetOfferDto, LimitsWorkspaceCreditsDto,
};
use crate::http::HttpError;
use crate::paths::scratch_dir;
use credentials::CredentialLookup;
use json::window;
use serde_json::json;
use std::cell::Cell;
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

fn parsed(windows: Vec<LimitWindowDto>) -> Parsed {
    Parsed {
        account: None,
        reading: Reading {
            plan: Some("max".to_string()),
            windows,
            ..Reading::default()
        },
    }
}

/// An account known as `id`, labelled `label`, for every test under `limits`.
pub(super) fn account(id: &str, label: &str) -> LimitsAccountDto {
    LimitsAccountDto {
        legacy_id: None,
        id: id.to_string(),
        label: Some(label.to_string()),
    }
}

#[test]
fn missing_login_is_signed_out_and_names_the_cli() {
    let dto = resolve::<()>(AgentId::Claude, None, CredentialLookup::Missing, |_| {
        unreachable!("no fetch without a login")
    });
    assert_eq!(dto.provider, AgentId::Claude);
    assert_eq!(dto.status, LimitsStatus::SignedOut);
    assert!(dto.message.as_deref().unwrap().contains("`claude`"));
    assert!(dto.reading.windows.is_empty());
}

#[test]
fn expired_and_unreadable_logins_map_to_their_statuses_without_loading() {
    let loaded = Cell::new(false);
    // Captures only `&loaded`, so the closure is `Copy` and can be handed to each call.
    let load = |_: &&str| {
        loaded.set(true);
        Ok(Parsed::default())
    };

    let expired = resolve(
        AgentId::Codex,
        None,
        CredentialLookup::Expired { renewable: false },
        load,
    );
    assert_eq!(expired.status, LimitsStatus::Unauthenticated);
    assert!(expired.message.as_deref().unwrap().contains("`codex`"));

    let unreadable = resolve(
        AgentId::Claude,
        Some(account("uuid-1", "me@example.com")),
        CredentialLookup::Unreadable("Keychain denied".to_string()),
        load,
    );
    assert_eq!(unreadable.status, LimitsStatus::Failed);
    assert!(unreadable
        .message
        .as_deref()
        .unwrap()
        .contains("Keychain denied"));
    assert_eq!(
        unreadable.account,
        Some(account("uuid-1", "me@example.com")),
        "a failure still says which account it is about"
    );

    assert!(!loaded.get());
}

/// The state on-n-off can put a user into by renewing a login it then could not store. The message
/// has to own that, name the reason, and send them to the only remedy that works — telling them to
/// run `claude` to renew it would point at a refresh token that is already spent.
#[test]
fn a_stranded_login_says_on_n_off_spent_it_and_asks_for_a_new_sign_in() {
    let dto = resolve::<()>(
        AgentId::Claude,
        Some(account("uuid-1", "me@example.com")),
        CredentialLookup::Stranded("Keychain write failed (denied)".to_string()),
        |_| unreachable!("nothing is loaded with a login that was not stored"),
    );
    assert_eq!(dto.status, LimitsStatus::Unauthenticated);
    let message = dto.message.as_deref().unwrap();
    assert!(message.starts_with("on-n-off renewed"), "{message}");
    assert!(
        message.contains("Keychain write failed (denied)"),
        "{message}"
    );
    assert!(message.contains("sign in again"), "{message}");
    assert!(
        !message.contains("send a prompt"),
        "the spent token cannot be renewed by running `claude`: {message}"
    );
    assert_eq!(dto.account, Some(account("uuid-1", "me@example.com")));
}

#[test]
fn a_rejected_token_is_unauthenticated_and_other_http_failures_are_failed() {
    let rejected = resolve(
        AgentId::Claude,
        None,
        CredentialLookup::Found("token"),
        |_| Err(HttpError::Unauthorized),
    );
    assert_eq!(rejected.status, LimitsStatus::Unauthenticated);

    let offline = resolve(
        AgentId::Claude,
        None,
        CredentialLookup::Found("token"),
        |_| Err(HttpError::Network("dns".to_string())),
    );
    assert_eq!(offline.status, LimitsStatus::Failed);
    assert!(offline.message.as_deref().unwrap().contains("dns"));

    let server = resolve(
        AgentId::Claude,
        None,
        CredentialLookup::Found("token"),
        |_| Err(HttpError::Status(503)),
    );
    assert_eq!(server.status, LimitsStatus::Failed);
    assert!(server.message.as_deref().unwrap().contains("503"));
}

#[test]
fn a_successful_load_is_ok_with_windows_ordered_weekly_session_model() {
    let dto = resolve(
        AgentId::Claude,
        None,
        CredentialLookup::Found("token"),
        |token| {
            assert_eq!(*token, "token");
            Ok(parsed(vec![
                window("m", "Weekly · Opus", LimitWindowKind::Model, 3.0, None),
                window(
                    "s",
                    "5 hour · all models",
                    LimitWindowKind::Session,
                    7.0,
                    None,
                ),
                window(
                    "w",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    12.0,
                    None,
                ),
                window("m2", "Weekly · Sonnet", LimitWindowKind::Model, 1.0, None),
            ]))
        },
    );
    assert_eq!(dto.status, LimitsStatus::Ok);
    assert_eq!(dto.message, None);
    assert_eq!(dto.reading.plan.as_deref(), Some("max"));
    let ids: Vec<&str> = dto.reading.windows.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["w", "s", "m", "m2"]);
    assert!(dto
        .reading
        .windows
        .iter()
        .all(|window| { chrono::DateTime::parse_from_rfc3339(&window.observed_at).is_ok() }));
}

/// A test build has no user home, so `read_limits` reads nothing and says why.
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
            ..Reading::default()
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

/// A saved profile's card says so over IPC as `savedProfile`; no other card carries the key, and a
/// card without it reads as not one.
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

/// An archived card says so over IPC as `archived`; no other card carries the key, and a card
/// without it, as every version before archiving sent, reads as not archived.
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

/// Every note crosses under the name the UI matches on, and comes back from a snapshot the same way.
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

/// Live probe against the real home, read-only: one read per provider, printed. A test build has
/// no user home, so the probe reads the one named in `ON_N_OFF_PROBE_HOME` (`paths::probe_home`).
///
/// Claude reads only the real home's `~/.claude/.credentials.json`: a test build's sealed
/// environment (`paths::process_env`) treats every home as disposable, so `CLAUDE_CONFIG_DIR`,
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR` and the Claude Code Keychain entry are out of its reach, and
/// a login kept in the Keychain reads as signed out here. Codex runs its own `codex app-server`,
/// and its native store reads a keyring login through the real `security` when its config selects
/// one, which is what `with_real_keychain` allows.
///
/// `ON_N_OFF_PROBE_HOME="$HOME" cargo test --manifest-path src-tauri/Cargo.toml probe_real_home_limits -- --ignored --nocapture`
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
