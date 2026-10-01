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

    assert_eq!(*fetched.lock().unwrap(), std::slice::from_ref(&b));
    let key = identity(AgentId::Claude, "b", "team").observation_key();
    let cards: Vec<_> = entries
        .iter()
        .filter_map(|entry| Some(entry.account.as_ref()?.id.as_str()))
        .collect();
    assert_eq!(cards, [key.as_str()]);
    assert_eq!(*harness.native.resolved.borrow(), [AgentId::Claude]);
    assert_eq!(harness.in_vault(&b), Some("b1".into()));
}

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

#[test]
fn a_usage_refresh_leaves_archived_accounts_alone() {
    let harness = Harness::new();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = identity(AgentId::Claude, "b", "team");
    harness.saved(b.clone(), claude("b", "b1"));
    let c = harness.saved(identity(AgentId::Claude, "c", "team"), claude("c", "c1"));
    harness.seed(|db| db.profiles[1].usage_renewal_owned = true);
    harness.signed_in(Some(claude("a", "a2")));
    crate::limits::set_archived_at(
        harness.path(),
        AgentId::Claude,
        &[b.observation_key()],
        true,
    )
    .unwrap();
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

    assert_eq!(*fetched.lock().unwrap(), [c]);
    assert!(entries
        .iter()
        .all(|entry| entry.account.as_ref().unwrap().id != b.observation_key()));
}
