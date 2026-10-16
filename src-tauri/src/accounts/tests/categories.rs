use super::fixture::*;
use super::*;

fn category(accounts: &Accounts, provider: AgentId, account_id: &str) -> Option<String> {
    accounts
        .categories()
        .unwrap()
        .0
        .get(provider.key())?
        .get(account_id)
        .cloned()
}

fn never_unlocked(harness: &Harness, home: &tempfile::TempDir) -> Accounts {
    Accounts {
        home: home.path().into(),
        ..harness.accounts()
    }
}

#[test]
fn an_account_without_a_saved_login_keeps_its_category_by_account_id() {
    let harness = Harness::new();

    let stored = harness
        .accounts()
        .set_account_category(
            AgentId::Muse,
            "email:you@example.com",
            "  Client A / research ",
        )
        .unwrap();

    assert_eq!(stored.as_deref(), Some("Client A / research"));
    assert_eq!(
        category(&harness.accounts(), AgentId::Muse, "email:you@example.com").as_deref(),
        Some("Client A / research")
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

    let stored = accounts
        .set_account_category(AgentId::Codex, "acct-1", "   ")
        .unwrap();

    assert_eq!(stored, None);
    assert!(
        !accounts.categories().unwrap().0.contains_key("codex"),
        "an empty provider leaves no entry"
    );
    assert_eq!(
        category(&accounts, AgentId::Muse, "acct-1").as_deref(),
        Some("Personal")
    );
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

    assert_eq!(
        category(&accounts, AgentId::Codex, "acct-1").as_deref(),
        Some("Work")
    );
    assert!(harness.heard().is_empty());
}

#[test]
fn a_blank_account_id_is_refused_and_writes_no_vault() {
    let harness = Harness::new();

    assert!(harness
        .accounts()
        .set_account_category(AgentId::Muse, "  ", "Work")
        .is_err());

    assert!(!store::Store::vault_exists(harness.path()));
    assert!(harness.heard().is_empty());
}

#[test]
fn reading_categories_without_a_vault_never_opens_the_credential_store() {
    let harness = Harness::new();
    let fresh = tempfile::tempdir().unwrap();

    assert!(never_unlocked(&harness, &fresh)
        .categories()
        .unwrap()
        .0
        .is_empty());
    assert!(!store::Store::vault_exists(fresh.path()));
}

#[test]
fn forgetting_without_a_vault_announces_nothing_and_never_opens_the_credential_store() {
    let harness = Harness::new();
    let fresh = tempfile::tempdir().unwrap();

    never_unlocked(&harness, &fresh)
        .forget_categories(AgentId::Muse, &["email:you@example.com".into()])
        .unwrap();

    assert!(harness.heard().is_empty());
    assert!(!store::Store::vault_exists(fresh.path()));
}

#[test]
fn forgetting_an_account_with_no_category_announces_nothing() {
    let harness = Harness::new();
    let accounts = harness.accounts();
    accounts
        .set_account_category(AgentId::Codex, "kept", "Kept")
        .unwrap();
    harness.heard();

    accounts
        .forget_categories(AgentId::Codex, &["never-set".into()])
        .unwrap();

    assert!(harness.heard().is_empty());
}

#[test]
fn forgetting_accounts_drops_their_categories_only() {
    let harness = Harness::new();
    let accounts = harness.accounts();
    accounts
        .set_account_category(AgentId::Codex, "old", "Gone")
        .unwrap();
    accounts
        .set_account_category(AgentId::Codex, "also-old", "Gone too")
        .unwrap();
    accounts
        .set_account_category(AgentId::Codex, "kept", "Kept")
        .unwrap();
    harness.heard();

    accounts
        .forget_categories(
            AgentId::Codex,
            &["old".into(), "also-old".into(), "never-set".into()],
        )
        .unwrap();

    let categories = accounts.categories().unwrap().0;
    assert_eq!(categories["codex"].len(), 1);
    assert_eq!(categories["codex"]["kept"], "Kept");
    assert_eq!(harness.heard(), [(Heard::Accounts, true)]);
}

#[test]
fn a_saved_login_shows_its_own_category_and_keeps_a_copy_no_other_login_receives() {
    let harness = Harness::new();
    let named = identity(AgentId::Codex, "a", "team");
    let other = identity(AgentId::Codex, "b", "team");
    let id = harness.saved(named.clone(), codex("a", "a1"));
    let other_id = harness.saved(other.clone(), codex("b", "b1"));

    harness
        .accounts()
        .set_account_category(AgentId::Codex, &named.observation_key(), "Client B")
        .unwrap();

    let listed = harness.accounts().list(AgentId::Codex).unwrap();
    let shown = |key: &str| {
        listed
            .profiles
            .iter()
            .find(|p| p.observation_id == key)
            .and_then(|p| p.category.clone())
    };
    assert_eq!(shown(&named.observation_key()).as_deref(), Some("Client B"));
    assert_eq!(shown(&other.observation_key()), None);
    let vault = harness.vault();
    let copy = |profile: &str| {
        vault
            .profiles
            .iter()
            .find(|p| p.id == profile)
            .unwrap()
            .category
            .clone()
    };
    assert_eq!(copy(&id).as_deref(), Some("Client B"));
    assert_eq!(copy(&other_id), None);
    assert_eq!(
        category(
            &harness.accounts(),
            AgentId::Codex,
            &other.observation_key()
        ),
        None
    );
}

#[test]
fn a_category_named_before_its_login_is_saved_is_kept_on_the_saved_login() {
    let harness = Harness::new();
    let identity = identity(AgentId::Codex, "a", "team");
    harness
        .accounts()
        .set_account_category(AgentId::Codex, &identity.observation_key(), "Named early")
        .unwrap();

    let id = harness.saved(identity, codex("a", "a1"));

    let vault = harness.vault();
    let profile = vault.profiles.iter().find(|p| p.id == id).unwrap();
    assert_eq!(profile.category.as_deref(), Some("Named early"));
}

#[test]
fn a_forgotten_saved_login_category_stays_forgotten_when_the_vault_loads_again() {
    let harness = Harness::new();
    let identity = identity(AgentId::Claude, "a", "team");
    let id = harness.saved(identity.clone(), claude("a", "a1"));
    let accounts = harness.accounts();
    accounts
        .set_account_category(AgentId::Claude, &identity.observation_key(), "Gone")
        .unwrap();

    accounts
        .forget_categories(AgentId::Claude, &[identity.observation_key()])
        .unwrap();

    let vault = harness.vault();
    assert_eq!(
        vault.profiles.iter().find(|p| p.id == id).unwrap().category,
        None
    );
    assert_eq!(
        category(&accounts, AgentId::Claude, &identity.observation_key()),
        None
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
        category(
            &harness.accounts(),
            AgentId::Claude,
            &identity.observation_key()
        )
        .as_deref(),
        Some("Legacy")
    );
}

#[test]
fn a_category_edit_persists_without_rejecting_sign_ins_in_flight() {
    let harness = Harness::new();
    let identity = identity(AgentId::Claude, "a", "team");
    harness.saved(identity.clone(), claude("a", "a1"));
    let sign_in = harness.sign_in();

    harness
        .accounts()
        .set_account_category(AgentId::Claude, &identity.observation_key(), "Client A")
        .unwrap();

    assert_eq!(
        harness.vault().profiles[0].category.as_deref(),
        Some("Client A")
    );
    assert!(harness.vouches(&sign_in));
    assert_eq!(harness.heard(), [(Heard::Accounts, true)]);
}

#[test]
fn a_category_edit_is_allowed_during_a_pending_recovery_and_keeps_the_journal() {
    let harness = Harness::new();
    let identity = identity(AgentId::Claude, "b", "team");
    let target = harness.saved(identity.clone(), claude("b", "b1"));
    harness.interrupted(&target, Some(claude("a", "a1")));

    harness
        .accounts()
        .set_account_category(AgentId::Claude, &identity.observation_key(), "Client B")
        .unwrap();

    let vault = harness.vault();
    assert_eq!(vault.profiles[0].category.as_deref(), Some("Client B"));
    assert_eq!(
        vault.recovery().map(|journal| journal.target_id.as_str()),
        Some(target.as_str())
    );
}
