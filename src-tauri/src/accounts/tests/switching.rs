//! Using a saved profile, recovering an interrupted switch, and signing out.
use super::super::Activation;
use super::fixture::{claude, claude_in, codex, fingerprint, generation, identity, Harness, Heard};
use crate::dto::AgentId;

/// Profile a is the CLI's login and has rotated to a2 since it was saved; b is saved.
fn two_profiles(harness: &Harness) -> (String, String) {
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.signed_in(Some(claude("a", "a2")));
    (a, b)
}

#[test]
fn using_a_profile_publishes_its_login_and_keeps_the_latest_outgoing_one() {
    let harness = Harness::new();
    let (a, b) = two_profiles(&harness);
    let sign_in = harness.sign_in();

    harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap();

    assert_eq!(harness.live(), Some("b1".into()));
    let vault = harness.vault();
    let login = |id: &str| {
        vault
            .profiles
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .login
            .as_ref()
            .and_then(|login| generation(Some(login)))
    };
    assert_eq!(login(&a), Some("a2".into()));
    assert_eq!(login(&b), Some("b1".into()));
    assert!(vault.recovery().is_none());
    assert!(!harness.vouches(&sign_in));
    assert_eq!(*harness.clients.asked.borrow(), ["activation safe"]);
    assert_eq!(*harness.native.resolved.borrow(), [AgentId::Claude]);
    assert_eq!(harness.heard(), [(Heard::Changed(AgentId::Claude), true)]);
}

#[test]
fn running_clients_refuse_an_ordinary_switch_but_not_one_made_beside_them() {
    let harness = Harness::new();
    let (_, b) = two_profiles(&harness);
    let elsewhere = harness.saved(
        identity(AgentId::Claude, "e", "elsewhere"),
        claude_in("e", "elsewhere", "e1"),
    );
    harness.clients.running.set(true);
    let sealed = harness.sealed();

    assert!(harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .is_err());
    assert_eq!(harness.live(), Some("a2".into()));
    assert_eq!(harness.sealed(), sealed);
    assert!(harness.heard().is_empty());

    harness
        .accounts()
        .activate(AgentId::Claude, &elsewhere, Activation::AlongsideClients)
        .unwrap();
    assert_eq!(harness.live(), Some("e1".into()));
    assert_eq!(*harness.clients.asked.borrow(), ["activation safe"]);
}

#[test]
fn another_providers_profile_is_not_used() {
    let harness = Harness::new();
    two_profiles(&harness);
    let codex = harness.saved(identity(AgentId::Codex, "c", "team"), codex("c", "c1"));
    let sealed = harness.sealed();

    let error = harness
        .accounts()
        .activate(AgentId::Claude, &codex, Activation::Ordinary)
        .unwrap_err();

    assert_eq!(error, "Profile does not belong to this provider.");
    assert_eq!(harness.live(), Some("a2".into()));
    assert_eq!(harness.sealed(), sealed);
    assert!(harness.heard().is_empty());
}

#[test]
fn a_login_with_an_unfinished_private_renewal_is_not_used() {
    let harness = Harness::new();
    let (_, b) = two_profiles(&harness);
    let profile = harness.seed(|db| {
        db.profiles[1].usage_renewal_owned = true;
        db.profiles[1].clone()
    });
    let open = || super::super::store::Store::open_existing(harness.path());
    assert!(
        super::super::usage_renew::renew_owned(&profile, &open, &|_| {
            Err("connection lost".into())
        })
        .is_err()
    );
    let sealed = harness.sealed();

    let error = harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap_err();

    assert!(error.contains("unfinished usage renewal"), "{error}");
    assert_eq!(harness.live(), Some("a2".into()));
    assert_eq!(harness.sealed(), sealed);
}

/// A rollback may have touched the native login, so a switch that failed is announced too.
#[test]
fn a_failed_switch_restores_the_outgoing_login_and_is_still_announced() {
    let harness = Harness::new();
    let (_, b) = two_profiles(&harness);
    *harness.native.verify_error.borrow_mut() = Some("verification refused".into());

    assert!(harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .is_err());

    assert_eq!(harness.live(), Some("a2".into()));
    assert!(harness.vault().recovery().is_none());
    assert_eq!(harness.heard(), [(Heard::Changed(AgentId::Claude), true)]);
}

/// A pending recovery refuses a use before it writes anything, so it rejects no sign-in in
/// flight, and leaves nothing to announce.
#[test]
fn a_pending_recovery_refuses_a_use_before_anything_is_written() {
    let harness = Harness::new();
    let (_, b) = two_profiles(&harness);
    harness.interrupted(&b, Some(claude("a", "a2")));
    let sealed = harness.sealed();

    for activation in [Activation::Ordinary, Activation::AlongsideClients] {
        let error = harness
            .accounts()
            .activate(AgentId::Claude, &b, activation)
            .unwrap_err();

        assert_eq!(harness.sealed(), sealed, "{activation:?} wrote the vault");
        assert!(harness.heard().is_empty(), "{activation:?} announced");
        assert!(
            error.starts_with("Recover the interrupted account change"),
            "{error}"
        );
        assert_eq!(harness.live(), Some("a2".into()));
    }
    assert!(harness.vault().recovery().is_some());
}

#[test]
fn recovery_restores_the_outgoing_login_with_clients_closed() {
    let harness = Harness::new();
    let (_, b) = two_profiles(&harness);
    let sign_in = harness.sign_in();
    harness.interrupted(&b, Some(claude("a", "a2")));
    harness.signed_in(Some(claude("b", "b1")));

    harness
        .accounts()
        .activate(AgentId::Claude, "", Activation::Recover)
        .unwrap();

    assert_eq!(harness.live(), Some("a2".into()));
    assert!(harness.vault().recovery().is_none());
    assert!(!harness.vouches(&sign_in));
    assert_eq!(*harness.clients.asked.borrow(), ["closed"]);
    assert_eq!(harness.heard(), [(Heard::Changed(AgentId::Claude), true)]);
}

/// Recovery rewrites the native login, so running clients refuse it: one could overwrite the
/// restored outgoing login after the journal is cleared.
#[test]
fn recovery_requires_closed_clients() {
    let harness = Harness::new();
    let (_, b) = two_profiles(&harness);
    harness.interrupted(&b, Some(claude("a", "a2")));
    harness.signed_in(Some(claude("b", "b1")));
    harness.clients.running.set(true);
    let sealed = harness.sealed();

    assert!(harness
        .accounts()
        .activate(AgentId::Claude, "", Activation::Recover)
        .is_err());

    assert_eq!(harness.sealed(), sealed);
    assert_eq!(harness.live(), Some("b1".into()));
    assert_eq!(
        harness.vault().recovery().map(|j| j.target_id.clone()),
        Some(b)
    );
    assert!(harness.heard().is_empty());
}

#[test]
fn a_recovery_pending_for_the_other_provider_is_refused() {
    let harness = Harness::new();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let codex = harness.saved(identity(AgentId::Codex, "c", "team"), codex("c", "c1"));
    harness.interrupted(&codex, Some(claude("c", "c0")));
    harness.signed_in(Some(claude("a", "a2")));
    let sealed = harness.sealed();

    let error = harness
        .accounts()
        .activate(AgentId::Claude, "", Activation::Recover)
        .unwrap_err();

    assert_eq!(error, "Recovery belongs to the other provider.");
    assert_eq!(harness.sealed(), sealed);
    assert_eq!(harness.live(), Some("a2".into()));
    assert_eq!(
        harness.vault().recovery().map(|j| j.target_id.clone()),
        Some(codex)
    );
    assert!(harness.heard().is_empty());
}

#[test]
fn recovery_without_a_pending_journal_is_refused() {
    let harness = Harness::new();
    two_profiles(&harness);
    let sealed = harness.sealed();

    let error = harness
        .accounts()
        .activate(AgentId::Claude, "", Activation::Recover)
        .unwrap_err();

    assert_eq!(error, "No recovery is pending.");
    assert_eq!(harness.sealed(), sealed);
    assert!(harness.heard().is_empty());
}

/// a and a-other are the same user in two workspaces, b another user, c the same user on Codex.
#[test]
fn signing_out_forgets_the_users_saved_logins_before_logging_out() {
    let harness = Harness::new();
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let other = harness.saved(identity(AgentId::Claude, "a", "other"), claude("a", "o1"));
    let b = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    let c = harness.saved(identity(AgentId::Codex, "a", "team"), codex("a", "c1"));
    harness.signed_in(Some(claude("a", "a2")));
    let sign_in = harness.sign_in();

    harness.accounts().sign_out(AgentId::Claude).unwrap();

    assert_eq!(harness.live(), None);
    let vault = harness.vault();
    let kept = |id: &str| {
        vault
            .profiles
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .login
            .is_some()
    };
    assert!(!kept(&a) && !kept(&other));
    assert!(kept(&b) && kept(&c));
    assert_eq!(
        vault.ignored_credentials,
        [fingerprint(AgentId::Claude, &claude("a", "a2"))]
    );
    assert!(!harness.vouches(&sign_in));
    assert_eq!(*harness.clients.asked.borrow(), ["closed"]);
    assert_eq!(harness.heard(), [(Heard::Changed(AgentId::Claude), true)]);
}

/// Signing Codex out resolves Codex's native store, and forgets only the user's Codex logins.
#[test]
fn signing_out_one_provider_leaves_the_other_providers_logins() {
    let harness = Harness::new();
    let claude_a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let codex_a = harness.saved(identity(AgentId::Codex, "a", "team"), codex("a", "c1"));
    harness.signed_in(Some(codex("a", "c2")));

    harness.accounts().sign_out(AgentId::Codex).unwrap();

    assert_eq!(*harness.native.resolved.borrow(), [AgentId::Codex]);
    let vault = harness.vault();
    let kept = |id: &str| {
        vault
            .profiles
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .login
            .is_some()
    };
    assert!(kept(&claude_a) && !kept(&codex_a));
    assert_eq!(harness.heard(), [(Heard::Changed(AgentId::Codex), true)]);
}

#[test]
fn a_failed_logout_keeps_the_forgotten_logins_and_is_still_announced() {
    let harness = Harness::new();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    harness.signed_in(Some(claude("a", "a2")));
    *harness.native.logout_error.borrow_mut() = Some("logout failed".into());

    assert_eq!(
        harness.accounts().sign_out(AgentId::Claude),
        Err("logout failed".into())
    );

    let vault = harness.vault();
    assert!(vault.profiles[0].login.is_none());
    assert_eq!(
        vault.ignored_credentials,
        [fingerprint(AgentId::Claude, &claude("a", "a2"))]
    );
    assert_eq!(harness.heard(), [(Heard::Changed(AgentId::Claude), true)]);
}

#[test]
fn signing_out_refuses_during_a_pending_recovery_without_writing_or_logging_out() {
    let harness = Harness::new();
    let (_, b) = two_profiles(&harness);
    harness.interrupted(&b, Some(claude("a", "a2")));
    let sealed = harness.sealed();

    let error = harness.accounts().sign_out(AgentId::Claude).unwrap_err();

    assert!(
        error.starts_with("Recover the interrupted account change"),
        "{error}"
    );
    assert_eq!(harness.native.logouts.get(), 0);
    assert_eq!(harness.sealed(), sealed);
    assert!(harness.heard().is_empty());
}

/// A pending recovery refuses a sign-out before it reads the native login (the Keychain, on
/// macOS).
#[test]
fn a_sign_out_refused_by_a_pending_recovery_reads_no_native_login() {
    let harness = Harness::new();
    let (_, b) = two_profiles(&harness);
    harness.interrupted(&b, Some(claude("a", "a2")));

    assert!(harness.accounts().sign_out(AgentId::Claude).is_err());

    assert_eq!(harness.native.reads.get(), 0, "read the native login");
    assert_eq!(harness.native.locks.get(), 0, "took the native locks");
}

#[test]
fn signing_out_requires_closed_clients() {
    let harness = Harness::new();
    two_profiles(&harness);
    harness.clients.running.set(true);
    let sealed = harness.sealed();

    assert!(harness.accounts().sign_out(AgentId::Claude).is_err());

    assert_eq!(harness.native.logouts.get(), 0);
    assert_eq!(harness.sealed(), sealed);
    assert!(harness.heard().is_empty());
}
