//! Automatic remembering of the CLI's own login.
use super::fixture::{claude, codex, generation, identity, Harness, Heard};
use crate::dto::AgentId;

fn remembering(harness: &Harness, on: bool) {
    std::fs::write(
        harness.path().join(".on-n-off/accounts/remembering.json"),
        format!("{{\"enabled\":{on}}}"),
    )
    .unwrap();
}

/// With remembering on, a poll saves the CLI's verified login once, through the provider's store
/// as the context resolves it, and announces it once saved and its leases released.
#[test]
fn a_remembered_login_is_saved_once_and_announced() {
    for (provider, login) in [
        (AgentId::Claude, claude("a", "a1")),
        (AgentId::Codex, codex("a", "a1")),
    ] {
        let harness = Harness::new();
        remembering(&harness, true);
        harness.signed_in(Some(login));

        harness.accounts().poll_remembering(provider);
        let vault = harness.vault();
        assert_eq!(vault.profiles.len(), 1);
        assert_eq!(vault.profiles[0].identity, identity(provider, "a", "team"));
        assert_eq!(
            generation(vault.profiles[0].login.as_ref()),
            Some("a1".into())
        );
        assert!(!vault.profiles[0].pending_activation);
        assert_eq!(*harness.native.resolved.borrow(), [provider]);
        assert_eq!(harness.heard(), [(Heard::Accounts, true)]);

        assert_eq!(
            harness.accounts().remember(provider),
            Ok(false),
            "already saved"
        );
        harness.accounts().poll_remembering(provider);
        assert!(harness.heard().is_empty());
        assert_eq!(harness.vault().profiles.len(), 1);
    }
}

/// With remembering off, the native store is not even resolved.
#[test]
fn nothing_is_remembered_while_remembering_is_off() {
    let harness = Harness::new();
    remembering(&harness, false);
    harness.signed_in(Some(claude("a", "a1")));

    assert_eq!(harness.accounts().remember(AgentId::Claude), Ok(false));
    assert!(harness.native.resolved.borrow().is_empty());
    assert_eq!(harness.sealed(), None);
    assert!(harness.heard().is_empty());
}

/// Turning remembering on or off is announced as an account-list change.
#[test]
fn turning_remembering_on_or_off_is_announced() {
    let harness = Harness::new();
    harness.accounts().set_remembering(true).unwrap();
    assert!(super::super::discovery::enabled(harness.path()).unwrap());
    harness.accounts().set_remembering(false).unwrap();
    assert!(!super::super::discovery::enabled(harness.path()).unwrap());
    assert_eq!(
        harness.heard(),
        [(Heard::Accounts, true), (Heard::Accounts, true)]
    );
}

/// A sign-out whose logout failed leaves the CLI signed in with the generation it forgot; that
/// generation, as its own provider fingerprints it, is not remembered again.
#[test]
fn a_codex_generation_signed_out_of_is_not_remembered_again() {
    let harness = Harness::new();
    harness.saved(identity(AgentId::Codex, "a", "team"), codex("a", "c1"));
    harness.signed_in(Some(codex("a", "c2")));
    *harness.native.logout_error.borrow_mut() = Some("logout failed".into());
    assert!(harness.accounts().sign_out(AgentId::Codex).is_err());
    assert_eq!(harness.live(), Some("c2".into()));
    remembering(&harness, true);
    harness.heard();

    assert_eq!(harness.accounts().remember(AgentId::Codex), Ok(false));
    assert!(harness.vault().profiles[0].login.is_none());
    assert!(harness.heard().is_empty());
}

/// What the account list says about `provider`'s automatic remembering.
fn notice(harness: &Harness, provider: AgentId) -> Option<String> {
    harness.accounts().list(provider).unwrap().notice
}

/// A poll that cannot remember the CLI's login says why on the account list, and is heard once;
/// the same failure again changes nothing and is not heard again.
#[test]
fn a_failed_remembering_poll_leaves_its_notice_and_is_heard_once() {
    let harness = Harness::new();
    remembering(&harness, true);
    harness.signed_in(Some(claude("a", "a1")));
    *harness.native.verify_error.borrow_mut() = Some("fixture verification refused".into());

    harness.accounts().poll_remembering(AgentId::Claude);
    assert_eq!(harness.heard(), [(Heard::Accounts, true)]);
    assert_eq!(
        notice(&harness, AgentId::Claude).as_deref(),
        Some("fixture verification refused")
    );
    assert_eq!(
        super::super::discovery::notice(AgentId::Codex),
        None,
        "only its own provider's"
    );

    harness.accounts().poll_remembering(AgentId::Claude);
    assert!(harness.heard().is_empty(), "nothing changed");

    *harness.native.verify_error.borrow_mut() = None;
    harness.accounts().poll_remembering(AgentId::Claude);
}

/// A poll that remembers the login clears the notice a failed one left, and the saved login and
/// the cleared notice are heard as one change.
#[test]
fn a_successful_remembering_poll_clears_the_notice_and_is_heard_once() {
    let harness = Harness::new();
    remembering(&harness, true);
    harness.signed_in(Some(claude("a", "a1")));
    *harness.native.verify_error.borrow_mut() = Some("fixture verification refused".into());
    harness.accounts().poll_remembering(AgentId::Claude);
    harness.heard();
    *harness.native.verify_error.borrow_mut() = None;

    harness.accounts().poll_remembering(AgentId::Claude);

    assert_eq!(harness.heard(), [(Heard::Accounts, true)]);
    assert_eq!(notice(&harness, AgentId::Claude), None);
    assert_eq!(harness.vault().profiles.len(), 1);
}

/// Turning remembering on or off clears every provider's notice, and is heard once.
#[test]
fn turning_remembering_on_clears_the_notices_and_is_heard_once() {
    let harness = Harness::new();
    remembering(&harness, true);
    harness.signed_in(Some(claude("a", "a1")));
    *harness.native.verify_error.borrow_mut() = Some("fixture verification refused".into());
    harness.accounts().poll_remembering(AgentId::Claude);
    harness.heard();

    harness.accounts().set_remembering(true).unwrap();

    assert_eq!(harness.heard(), [(Heard::Accounts, true)]);
    assert_eq!(notice(&harness, AgentId::Claude), None);
}
