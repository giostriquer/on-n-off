use super::fixture::{claude, identity, Harness};
use crate::dto::AgentId;

#[test]
fn listing_shows_this_providers_profiles_and_which_one_the_cli_uses() {
    let harness = Harness::new();
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.saved(identity(AgentId::Codex, "c", "team"), claude("c", "c1"));
    harness.seed(|db| db.profiles[1].login = None);
    harness.signed_in(Some(claude("a", "a2")));
    let sealed = harness.sealed();

    let listed = harness.accounts().list(AgentId::Claude).unwrap();

    let shown: Vec<_> = listed
        .profiles
        .iter()
        .map(|p| (p.id.as_str(), p.active, p.needs_login))
        .collect();
    assert_eq!(
        shown,
        [(a.as_str(), true, false), (b.as_str(), false, true)]
    );
    assert_eq!(listed.profiles[0].email.as_deref(), Some("a@example.com"));
    assert_eq!(
        listed.native_account,
        Some(identity(AgentId::Claude, "a", "team"))
    );
    assert!(!listed.recovery_required);
    assert_eq!(harness.sealed(), sealed, "listing never writes the vault");
    assert!(harness.heard().is_empty());
}

#[test]
fn listing_a_device_without_a_vault_neither_creates_nor_unlocks_one() {
    let harness = Harness::new();
    harness.signed_in(Some(claude("a", "a1")));

    let listed = harness.accounts().list(AgentId::Claude).unwrap();

    assert!(listed.profiles.is_empty());
    assert_eq!(
        listed.native_account,
        Some(identity(AgentId::Claude, "a", "team"))
    );
    assert_eq!(harness.sealed(), None);
}

#[test]
fn a_pending_recovery_is_reported_only_to_the_provider_it_belongs_to() {
    let harness = Harness::new();
    let target = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.interrupted(&target, Some(claude("a", "a1")));

    assert!(
        harness
            .accounts()
            .list(AgentId::Claude)
            .unwrap()
            .recovery_required
    );
    assert!(
        !harness
            .accounts()
            .list(AgentId::Codex)
            .unwrap()
            .recovery_required
    );
}
