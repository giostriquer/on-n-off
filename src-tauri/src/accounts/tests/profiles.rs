//! Saving the current login, editing a category and removing a profile.
use super::fixture::{claude, generation, identity, Harness, Heard};
use crate::dto::AgentId;

#[test]
fn saving_the_current_login_keeps_it_as_a_profile_and_rejects_sign_ins_in_flight() {
    let harness = Harness::new();
    harness.signed_in(Some(claude("a", "a1")));
    harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    let sign_in = harness.sign_in();

    harness.accounts().save_current(AgentId::Claude).unwrap();

    let vault = harness.vault();
    let saved = vault
        .profiles
        .iter()
        .find(|p| p.identity == identity(AgentId::Claude, "a", "team"))
        .expect("the current login is saved");
    assert_eq!(generation(saved.login.as_ref()), Some("a1".into()));
    assert_eq!(saved.label, "a@example.com");
    assert!(!saved.pending_activation && !saved.usage_renewal_owned);
    assert_eq!(vault.profiles.len(), 2);
    assert!(!harness.vouches(&sign_in));
    assert_eq!(harness.live(), Some("a1".into()), "the CLI keeps its login");
}

#[test]
fn saving_reenrolls_an_account_that_remove_excluded() {
    let harness = Harness::new();
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    harness.accounts().remove(&a).unwrap();
    assert_eq!(harness.vault().ignored_accounts.len(), 1);
    harness.signed_in(Some(claude("a", "a2")));

    harness.accounts().save_current(AgentId::Claude).unwrap();

    let vault = harness.vault();
    assert!(vault.ignored_accounts.is_empty());
    assert_eq!(
        generation(vault.profiles[0].login.as_ref()),
        Some("a2".into())
    );
}

/// Only the current account's own pending sign-in refuses a save; another account's is kept.
#[test]
fn saving_beside_another_accounts_pending_sign_in_updates_only_the_current_profile() {
    let harness = Harness::new();
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = harness.saved(
        identity(AgentId::Claude, "b", "team"),
        claude("b", "isolated"),
    );
    harness.seed(|db| db.profiles[1].pending_activation = true);
    harness.signed_in(Some(claude("a", "a2")));

    harness.accounts().save_current(AgentId::Claude).unwrap();

    let vault = harness.vault();
    assert_eq!(vault.profiles.len(), 2);
    let profile = |id: &str| vault.profiles.iter().find(|p| p.id == id).unwrap();
    assert_eq!(generation(profile(&a).login.as_ref()), Some("a2".into()));
    assert!(profile(&b).pending_activation);
    assert_eq!(
        generation(profile(&b).login.as_ref()),
        Some("isolated".into())
    );
}

#[test]
fn saving_refuses_to_overwrite_a_sign_in_awaiting_activation() {
    let harness = Harness::new();
    harness.saved(
        identity(AgentId::Claude, "a", "team"),
        claude("a", "isolated"),
    );
    harness.seed(|db| db.profiles[0].pending_activation = true);
    harness.signed_in(Some(claude("a", "native")));
    let sealed = harness.sealed();

    let error = harness
        .accounts()
        .save_current(AgentId::Claude)
        .unwrap_err();

    assert!(error.contains("awaiting activation"), "{error}");
    assert_eq!(harness.sealed(), sealed);
}

#[test]
fn saving_refuses_during_a_pending_recovery_without_writing() {
    let harness = Harness::new();
    let target = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.interrupted(&target, Some(claude("a", "a1")));
    harness.signed_in(Some(claude("a", "a1")));
    let sealed = harness.sealed();

    let error = harness
        .accounts()
        .save_current(AgentId::Claude)
        .unwrap_err();

    assert!(
        error.starts_with("Recover the interrupted account change"),
        "{error}"
    );
    assert_eq!(harness.sealed(), sealed);
    assert!(harness.heard().is_empty());
}

/// A pending recovery refuses a save before it takes the native locks (Claude Code's) or reads
/// the native login (the Keychain, on macOS).
#[test]
fn a_save_refused_by_a_pending_recovery_touches_no_native_lock_or_login() {
    let harness = Harness::new();
    let target = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.interrupted(&target, Some(claude("a", "a1")));
    harness.signed_in(Some(claude("a", "a1")));

    assert!(harness.accounts().save_current(AgentId::Claude).is_err());

    assert_eq!(harness.native.locks.get(), 0, "took the native locks");
    assert_eq!(harness.native.reads.get(), 0, "read the native login");
}

#[test]
fn a_category_edit_persists_without_rejecting_sign_ins_in_flight() {
    let harness = Harness::new();
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let sign_in = harness.sign_in();

    harness
        .accounts()
        .set_category(&a, "  Client A / research ")
        .unwrap();

    assert_eq!(
        harness.vault().profiles[0].category.as_deref(),
        Some("Client A / research")
    );
    assert!(harness.vouches(&sign_in));
    assert_eq!(harness.heard(), [(Heard::Accounts, true)]);
}

#[test]
fn a_category_edit_is_allowed_during_a_pending_recovery_and_keeps_the_journal() {
    let harness = Harness::new();
    let target = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.interrupted(&target, Some(claude("a", "a1")));

    harness
        .accounts()
        .set_category(&target, "Client B")
        .unwrap();

    let vault = harness.vault();
    assert_eq!(vault.profiles[0].category.as_deref(), Some("Client B"));
    assert_eq!(
        vault.recovery().map(|journal| journal.target_id.as_str()),
        Some(target.as_str())
    );
}

#[test]
fn removing_a_profile_excludes_its_account_from_remembering_and_rejects_sign_ins_in_flight() {
    let harness = Harness::new();
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    let sign_in = harness.sign_in();

    harness.accounts().remove(&a).unwrap();

    let vault = harness.vault();
    let left: Vec<_> = vault.profiles.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(left, [b.as_str()]);
    assert_eq!(
        vault.ignored_accounts,
        [identity(AgentId::Claude, "a", "team")]
    );
    assert!(!harness.vouches(&sign_in));
}

#[test]
fn removing_refuses_during_a_pending_recovery_without_writing() {
    let harness = Harness::new();
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let target = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.interrupted(&target, Some(claude("a", "a1")));
    let sealed = harness.sealed();

    let error = harness.accounts().remove(&a).unwrap_err();

    assert!(
        error.starts_with("Recover the interrupted account change"),
        "{error}"
    );
    assert_eq!(harness.sealed(), sealed);
    assert!(harness.heard().is_empty());
}

/// A saved card changes Limits, which serves a cached reading until its next poll unless the
/// change asks it to read again.
#[test]
fn saving_the_current_login_refreshes_limits_after_release() {
    let harness = Harness::new();
    harness.signed_in(Some(claude("a", "a1")));

    harness.accounts().save_current(AgentId::Claude).unwrap();

    assert_eq!(harness.heard(), [(Heard::Changed(AgentId::Claude), true)]);
}

/// A removed card would linger in Limits until the next poll: the removal refreshes the removed
/// profile's provider.
#[test]
fn removing_a_profile_refreshes_its_providers_limits_after_release() {
    let harness = Harness::new();
    let codex = harness.saved(identity(AgentId::Codex, "c", "team"), claude("c", "c1"));

    harness.accounts().remove(&codex).unwrap();

    assert_eq!(harness.heard(), [(Heard::Changed(AgentId::Codex), true)]);
}
