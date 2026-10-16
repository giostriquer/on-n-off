use super::fixture::*;
use super::*;

#[test]
fn an_account_without_a_saved_login_keeps_its_category_by_account_id() {
    let harness = Harness::new();

    harness
        .accounts()
        .set_account_category(
            AgentId::Muse,
            "email:you@example.com",
            "  Client A / research ",
        )
        .unwrap();

    assert_eq!(
        harness.accounts().categories().unwrap()["muse"]["email:you@example.com"],
        "Client A / research"
    );
    assert_eq!(harness.heard(), [(Heard::Accounts, true)]);
}

#[test]
fn a_category_is_kept_apart_per_provider_and_cleared_by_a_blank() {
    let harness = Harness::new();
    let accounts = harness.accounts();
    accounts
        .set_account_category(AgentId::Codex, "acct-1", "Work")
        .unwrap();
    accounts
        .set_account_category(AgentId::Muse, "acct-1", "Personal")
        .unwrap();

    accounts
        .set_account_category(AgentId::Codex, "acct-1", "   ")
        .unwrap();

    let categories = accounts.categories().unwrap();
    assert!(
        !categories.contains_key("codex"),
        "an empty provider leaves no entry"
    );
    assert_eq!(categories["muse"]["acct-1"], "Personal");
}

#[test]
fn a_category_longer_than_a_hundred_characters_is_refused_and_nothing_changes() {
    let harness = Harness::new();
    let accounts = harness.accounts();
    accounts
        .set_account_category(AgentId::Codex, "acct-1", "Work")
        .unwrap();
    harness.heard();

    assert!(accounts
        .set_account_category(AgentId::Codex, "acct-1", &"x".repeat(101))
        .is_err());

    assert_eq!(accounts.categories().unwrap()["codex"]["acct-1"], "Work");
    assert!(harness.heard().is_empty());
}

#[test]
fn reading_categories_without_a_vault_creates_none() {
    let harness = Harness::new();

    assert!(harness.accounts().categories().unwrap().is_empty());
    assert!(!store::Store::vault_exists(harness.path()));
}

#[test]
fn a_saved_login_shows_the_category_set_on_its_card_and_keeps_a_copy_for_older_app_versions() {
    let harness = Harness::new();
    let identity = identity(AgentId::Codex, "a", "team");
    let id = harness.saved(identity.clone(), codex("a", "a1"));

    harness
        .accounts()
        .set_account_category(AgentId::Codex, &identity.observation_key(), "Client B")
        .unwrap();

    let listed = harness.accounts().list(AgentId::Codex).unwrap();
    assert_eq!(listed.profiles[0].category.as_deref(), Some("Client B"));
    let vault = harness.vault();
    assert_eq!(
        vault
            .profiles
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .category
            .as_deref(),
        Some("Client B")
    );
}

#[test]
fn a_saved_login_category_from_an_older_version_moves_to_its_account_id() {
    let harness = Harness::new();
    let identity = identity(AgentId::Claude, "a", "team");
    harness.seed(|db| {
        let id = db.save(identity.clone(), claude("a", "a1"), None).unwrap();
        db.profiles
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .category = Some("Legacy".into());
    });

    assert_eq!(
        harness.accounts().categories().unwrap()["claude"][&identity.observation_key()],
        "Legacy"
    );
}

#[test]
fn the_profile_category_edit_writes_the_same_store_as_the_card() {
    let harness = Harness::new();
    let identity = identity(AgentId::Claude, "a", "team");
    let id = harness.saved(identity.clone(), claude("a", "a1"));

    harness.accounts().set_category(&id, "Client C").unwrap();

    assert_eq!(
        harness.accounts().categories().unwrap()["claude"][&identity.observation_key()],
        "Client C"
    );
}

#[test]
fn forgetting_accounts_drops_their_categories_only() {
    let harness = Harness::new();
    let accounts = harness.accounts();
    accounts
        .set_account_category(AgentId::Codex, "old", "Gone")
        .unwrap();
    accounts
        .set_account_category(AgentId::Codex, "kept", "Kept")
        .unwrap();
    harness.heard();

    accounts
        .forget_categories(AgentId::Codex, &["old".into(), "never-set".into()])
        .unwrap();

    let categories = accounts.categories().unwrap();
    assert_eq!(categories["codex"].len(), 1);
    assert_eq!(categories["codex"]["kept"], "Kept");
    assert_eq!(harness.heard(), [(Heard::Accounts, true)]);
}

#[test]
fn forgetting_accounts_with_no_category_announces_nothing_and_creates_no_vault() {
    let harness = Harness::new();

    harness
        .accounts()
        .forget_categories(AgentId::Muse, &["email:you@example.com".into()])
        .unwrap();

    assert!(harness.heard().is_empty());
    assert!(!store::Store::vault_exists(harness.path()));
}
