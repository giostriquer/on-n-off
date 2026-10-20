use super::fixture::{codex, identity, Harness};
use crate::accounts::{forget_account_at, Forgot};
use crate::dto::AgentId;

fn named(harness: &Harness, provider: AgentId) -> Vec<(String, String)> {
    harness
        .vault()
        .categories()
        .get(provider.key())
        .into_iter()
        .flatten()
        .map(|(id, category)| (id.clone(), category.clone()))
        .collect()
}

#[test]
fn forgetting_a_saved_card_removes_its_profile_and_the_categories_of_every_id_it_holds_in_one_change(
) {
    let harness = Harness::new();
    let a = identity(AgentId::Codex, "a", "team");
    let b = identity(AgentId::Codex, "b", "team");
    let kept = harness.saved(b.clone(), codex("b", "b1"));
    harness.saved(a.clone(), codex("a", "a1"));
    harness.seed(|db| {
        for (id, category) in [
            (a.observation_key(), "Client A"),
            ("team".to_string(), "Old team"),
            (b.observation_key(), "Client B"),
        ] {
            db.set_account_category(AgentId::Codex, &id, category)
                .unwrap();
        }
    });
    let sign_in = harness.sign_in();

    let forgot = forget_account_at(
        harness.path(),
        AgentId::Codex,
        &a.observation_key(),
        &["team".to_string(), a.observation_key()],
    );

    assert_eq!(
        forgot,
        Ok(Forgot {
            profile: true,
            changed: true
        })
    );
    let vault = harness.vault();
    let left: Vec<_> = vault.profiles.iter().map(|p| p.id.clone()).collect();
    assert_eq!(left, [kept]);
    assert_eq!(vault.ignored_accounts, [a], "kept out of remembering");
    assert_eq!(
        named(&harness, AgentId::Codex),
        [(b.observation_key(), "Client B".to_string())]
    );
    assert!(
        !harness.vouches(&sign_in),
        "removing a saved login is an account change"
    );
    assert!(harness.heard().is_empty(), "its caller announces it");
}

#[test]
fn forgetting_a_history_card_drops_only_its_categories_and_leaves_sign_ins_running() {
    let harness = Harness::new();
    let b = identity(AgentId::Codex, "b", "team");
    harness.saved(b.clone(), codex("b", "b1"));
    harness.seed(|db| {
        db.set_account_category(AgentId::Codex, "team", "Old team")
            .unwrap();
    });
    let sign_in = harness.sign_in();
    let forget = || forget_account_at(harness.path(), AgentId::Codex, "team", &["team".into()]);

    assert_eq!(
        forget(),
        Ok(Forgot {
            profile: false,
            changed: true
        })
    );
    assert_eq!(harness.vault().profiles.len(), 1);
    assert!(named(&harness, AgentId::Codex).is_empty());
    assert!(
        harness.vouches(&sign_in),
        "a category is metadata, so a sign-in in flight still publishes"
    );
    let sealed = harness.sealed();
    assert_eq!(forget(), Ok(Forgot::default()));
    assert_eq!(
        harness.sealed(),
        sealed,
        "nothing left to drop writes nothing"
    );
}

#[test]
fn a_pending_recovery_refuses_to_remove_a_saved_login_and_writes_nothing() {
    let harness = Harness::new();
    let a = identity(AgentId::Codex, "a", "team");
    let id = harness.saved(a.clone(), codex("a", "a1"));
    harness.interrupted(&id, None);
    let sealed = harness.sealed();

    let refused = forget_account_at(
        harness.path(),
        AgentId::Codex,
        &a.observation_key(),
        &[a.observation_key()],
    );

    assert_eq!(
        refused,
        Err("Recover the interrupted account change first.".to_string())
    );
    assert_eq!(harness.sealed(), sealed);
    assert_eq!(harness.vault().profiles.len(), 1);
}

#[test]
fn forgetting_with_no_vault_creates_none() {
    let harness = Harness::new();

    assert_eq!(
        forget_account_at(
            harness.path(),
            AgentId::Claude,
            "user-a",
            &["user-a".into()]
        ),
        Ok(Forgot::default())
    );
    assert!(harness.sealed().is_none());
}

#[test]
fn a_saved_login_of_another_provider_is_left_alone() {
    let harness = Harness::new();
    let a = identity(AgentId::Codex, "a", "team");
    harness.saved(a.clone(), codex("a", "a1"));

    let forgot = forget_account_at(
        harness.path(),
        AgentId::Claude,
        &a.observation_key(),
        &[a.observation_key()],
    );

    assert_eq!(forgot, Ok(Forgot::default()));
    assert_eq!(harness.vault().profiles.len(), 1);
}
