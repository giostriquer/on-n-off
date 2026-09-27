//! What unarchives an account among the account operations: the explicit re-adds (Save account,
//! Add account, Sign in again), never automatic remembering.
use super::fixture::{claude, codex, identity, Harness};
use crate::accounts::model::Identity;
use crate::dto::AgentId;
use std::collections::BTreeSet;

fn archive(harness: &Harness, provider: AgentId, ids: &[String]) {
    crate::limits::set_archived_at(harness.path(), provider, ids, true).unwrap();
}

fn archived(harness: &Harness, provider: AgentId) -> BTreeSet<String> {
    crate::limits::archived(harness.path(), provider)
}

/// `identity`'s card, archived beside another account's.
fn archived_beside_another(harness: &Harness, identity: &Identity) {
    archive(
        harness,
        identity.provider,
        &[identity.observation_key(), "profile:other".into()],
    );
}

#[test]
fn saving_the_current_login_unarchives_its_account() {
    let harness = Harness::new();
    let a = identity(AgentId::Claude, "a", "team");
    archived_beside_another(&harness, &a);
    harness.signed_in(Some(claude("a", "a1")));

    harness.accounts().save_current(AgentId::Claude).unwrap();

    assert_eq!(
        archived(&harness, AgentId::Claude),
        BTreeSet::from(["profile:other".to_string()])
    );
}

/// Add account and Sign in again unarchive the account they signed in to once the sign-in is
/// published; one that did not finish leaves it archived.
#[test]
fn a_finished_sign_in_unarchives_its_account() {
    for (provider, login) in [
        (AgentId::Claude, claude("b", "b1")),
        (AgentId::Codex, codex("b", "b1")),
    ] {
        let harness = Harness::new();
        let b = identity(provider, "b", "team");
        archived_beside_another(&harness, &b);
        *harness.native.signed_in.borrow_mut() = Some(login);

        harness.native.sign_in_exit.set(1);
        let operation = uuid::Uuid::new_v4().to_string();
        assert!(harness.accounts().add(provider, operation, None).is_err());
        assert!(
            archived(&harness, provider).contains(&b.observation_key()),
            "{provider:?}: an unfinished sign-in unarchives nothing"
        );

        harness.native.sign_in_exit.set(0);
        let operation = uuid::Uuid::new_v4().to_string();
        harness.accounts().add(provider, operation, None).unwrap();
        assert_eq!(
            archived(&harness, provider),
            BTreeSet::from(["profile:other".to_string()]),
            "{provider:?}"
        );

        archive(&harness, provider, &[b.observation_key()]);
        let again = harness.vault().profiles[0].id.clone();
        let operation = uuid::Uuid::new_v4().to_string();
        harness
            .accounts()
            .add(provider, operation, Some(again))
            .unwrap();
        assert!(
            !archived(&harness, provider).contains(&b.observation_key()),
            "{provider:?}: sign in again"
        );
    }
}

/// Automatic remembering is not the user's action, so it never unarchives the login it saves.
#[test]
fn automatic_remembering_never_unarchives() {
    let harness = Harness::new();
    std::fs::write(
        harness.path().join(".on-n-off/accounts/remembering.json"),
        "{\"enabled\":true}",
    )
    .unwrap();
    let a = identity(AgentId::Claude, "a", "team");
    archived_beside_another(&harness, &a);
    harness.signed_in(Some(claude("a", "a1")));

    assert_eq!(harness.accounts().remember(AgentId::Claude), Ok(true));

    assert!(archived(&harness, AgentId::Claude).contains(&a.observation_key()));
}
