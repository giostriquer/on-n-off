//! Reading the usage of saved accounts beside the CLI's own.
use super::super::usage::FetchResult;
use super::fixture::{claude, identity, Harness};
use crate::dto::{AgentId, ProviderLimitsDto};
use std::sync::Mutex;

fn reading(key: &str) -> ProviderLimitsDto {
    ProviderLimitsDto::for_test(AgentId::Claude, key).with_reading(crate::dto::Reading {
        windows: vec![crate::dto::LimitWindowDto {
            id: "weekly".into(),
            label: "Weekly".into(),
            kind: crate::dto::LimitWindowKind::Weekly,
            used_percent: 42.0,
            window_seconds: Some(604800),
            resets_at: None,
            observed_at: "2026-09-19T00:00:00Z".into(),
        }],
        ..Default::default()
    })
}

/// A usage refresh resolves the provider's store through the context and reads every saved
/// account but the one the CLI is signed in with, whose reading is the CLI's own.
#[test]
fn a_usage_refresh_reads_every_saved_account_but_the_signed_in_one() {
    let harness = Harness::new();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.signed_in(Some(claude("a", "a2")));
    let fetched = Mutex::new(Vec::new());
    let mut entries = Vec::new();

    harness
        .accounts()
        .refresh_usage(AgentId::Claude, false, &mut entries, &|profile, _| {
            fetched.lock().unwrap().push(profile.id.clone());
            FetchResult {
                login: profile.login.clone(),
                result: Ok(reading(&profile.identity.observation_key())),
            }
        });

    assert_eq!(*fetched.lock().unwrap(), [b]);
    let key = identity(AgentId::Claude, "b", "team").observation_key();
    let cards: Vec<_> = entries
        .iter()
        .filter_map(|entry| Some(entry.account.as_ref()?.id.as_str()))
        .collect();
    assert_eq!(cards, [key.as_str()]);
    assert_eq!(*harness.native.resolved.borrow(), [AgentId::Claude]);
}

/// A device that never saved an account reads nothing and resolves nothing.
#[test]
fn a_usage_refresh_without_a_vault_reads_nothing() {
    let harness = Harness::new();
    let mut entries = Vec::new();
    harness
        .accounts()
        .refresh_usage(AgentId::Claude, false, &mut entries, &|_, _| {
            panic!("fetched without a vault")
        });
    assert!(entries.is_empty());
    assert!(harness.native.resolved.borrow().is_empty());
}
