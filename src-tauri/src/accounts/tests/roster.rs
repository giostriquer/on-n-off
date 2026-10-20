use super::super::usage::FetchResult;
use super::fixture::{claude, codex, identity, weekly_reading, Harness};
use crate::accounts::Roster;
use crate::dto::{AgentId, ProviderLimitsDto};

fn answered(harness: &Harness, provider: AgentId) -> Option<Roster> {
    let mut entries: Vec<ProviderLimitsDto> = Vec::new();
    harness
        .accounts()
        .refresh_usage(provider, false, &mut entries, &|profile, _| FetchResult {
            login: profile.login.clone(),
            result: Ok(crate::limits::Accepted::for_test(weekly_reading(
                &profile.identity.observation_key(),
            ))),
        })
}

fn without_sign_in_end(profile: &impl serde::Serialize) -> serde_json::Value {
    let mut json = serde_json::to_value(profile).unwrap();
    json.as_object_mut().unwrap().remove("signInEndsAt");
    json
}

#[test]
fn the_limits_read_lists_each_saved_profile_as_the_account_list_does_but_its_sign_in_end() {
    let harness = Harness::new().with_homes();
    let a = identity(AgentId::Claude, "a", "team");
    let b = identity(AgentId::Claude, "b", "team");
    harness.saved(a.clone(), claude("a", "a1"));
    harness.saved(b.clone(), claude("b", "b1"));
    harness.saved(identity(AgentId::Codex, "c", "team"), codex("c", "c1"));
    harness.signed_in(Some(claude("a", "a2")));
    harness
        .accounts()
        .set_account_category(AgentId::Claude, &b.observation_key(), "Client A")
        .unwrap();
    harness
        .accounts()
        .set_account_category(AgentId::Claude, "history", "Old team")
        .unwrap();
    crate::limits::set_archived_at(
        harness.path(),
        AgentId::Claude,
        &[b.observation_key()],
        true,
    )
    .unwrap();
    *harness.native.deadline.borrow_mut() = Some(super::super::model::SignInDeadline {
        identity: a.clone(),
        ends_at_ms: 1_792_081_900_000,
    });

    let roster = answered(&harness, AgentId::Claude).unwrap();
    let listed = harness.accounts().list(AgentId::Claude).unwrap();

    let read: Vec<_> = roster.profiles().iter().map(without_sign_in_end).collect();
    let shown: Vec<_> = serde_json::to_value(&listed).unwrap()["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(without_sign_in_end)
        .collect();
    assert_eq!(read, shown);
    assert_eq!(
        roster
            .profiles()
            .iter()
            .map(|p| (p.active(), p.archived()))
            .collect::<Vec<_>>(),
        [(true, false), (false, true)]
    );
    assert_eq!(roster.category(&b.observation_key()), Some("Client A"));
    assert_eq!(roster.category("history"), Some("Old team"));
    assert_eq!(
        listed.sign_in_deadlines(),
        crate::accounts::SignInDeadlines::from([(
            a.observation_key(),
            "2026-10-15T16:31:40Z".to_string()
        )]),
        "the account list hands Limits the sign-in ends it read"
    );
}

#[test]
fn the_limits_read_reads_no_sign_in_end_and_the_native_login_only_once() {
    let harness = Harness::new().with_homes();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.signed_in(Some(claude("a", "a2")));
    answered(&harness, AgentId::Claude);
    harness.native.reads.set(0);

    let roster = answered(&harness, AgentId::Claude).unwrap();

    assert_eq!(roster.profiles().len(), 2);
    assert!(
        harness.homes().logins.lock().unwrap().len() == 1,
        "b's login waits in its home"
    );
    assert_eq!(harness.native.deadline_reads.get(), 0);
    assert!(harness.homes().deadline_reads.lock().unwrap().is_empty());
    assert_eq!(
        harness.native.reads.get(),
        1,
        "the read the saved poll needs"
    );
}

#[test]
fn the_limits_read_opens_the_vault_only_with_a_key_this_session_unlocked() {
    let home = tempfile::tempdir().unwrap();
    let vault = home.path().join(".on-n-off/accounts/vault.enc");
    std::fs::create_dir_all(vault.parent().unwrap()).unwrap();
    std::fs::write(&vault, b"sealed elsewhere").unwrap();

    assert_eq!(Roster::read(home.path(), AgentId::Claude, None), None);
}
