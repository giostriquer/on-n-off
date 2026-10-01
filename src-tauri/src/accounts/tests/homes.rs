use super::super::{store::Store, usage::FetchResult, usage_renew::Renewals, Activation};
use super::fixture::{claude, claude_in, identity, weekly_reading, Harness};
use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto};
use std::sync::Mutex;

fn two_accounts(harness: &Harness) -> (String, String) {
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.signed_in(Some(claude("a", "a2")));
    (a, b)
}

fn read_all(harness: &Harness) -> (Vec<ProviderLimitsDto>, Vec<String>) {
    let fetched = Mutex::new(Vec::new());
    let mut entries = Vec::new();
    harness
        .accounts()
        .refresh_usage(AgentId::Claude, true, &mut entries, &|profile, _| {
            fetched.lock().unwrap().push(profile.id.clone());
            FetchResult {
                login: profile.login.clone(),
                result: Ok(weekly_reading(&profile.identity.observation_key())),
            }
        });
    (entries, fetched.into_inner().unwrap())
}

fn read(harness: &Harness) -> Vec<ProviderLimitsDto> {
    let (cards, fetched) = read_all(harness);
    assert!(fetched.is_empty(), "read with a vault login: {fetched:?}");
    cards
}

fn key(user: &str) -> String {
    identity(AgentId::Claude, user, "team").observation_key()
}

fn asked(harness: &Harness) -> Vec<std::path::PathBuf> {
    harness.homes().read.lock().unwrap().clone()
}

fn account_home(harness: &Harness, id: &str) -> std::path::PathBuf {
    super::super::homes::dir(harness.path(), id).unwrap()
}

fn seeded_home(harness: &Harness, profile: usize, login: super::super::store::Login) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    harness.seed(|db| db.profiles[profile].home = Some(id.clone()));
    let dir = account_home(harness, &id);
    std::fs::create_dir_all(&dir).unwrap();
    harness.homes().logins.lock().unwrap().insert(dir, login);
    id
}

#[test]
fn a_saved_login_that_is_not_the_signed_in_one_moves_into_its_home_and_is_read_there() {
    let harness = Harness::new().with_homes();
    let (a, b) = two_accounts(&harness);

    let cards = read(&harness);

    assert_eq!(harness.in_home(&b), Some("b1".into()));
    assert_eq!(
        harness.in_vault(&b),
        None,
        "a second copy stayed in the vault"
    );
    assert_eq!(asked(&harness), [harness.home_of(&b).unwrap()]);
    let read: Vec<_> = cards
        .iter()
        .map(|card| {
            (
                card.account.as_ref().unwrap().id.clone(),
                card.saved_profile,
            )
        })
        .collect();
    assert_eq!(read, [(key("b"), true)]);
    assert_eq!(harness.in_vault(&a), Some("a1".into()));
    assert_eq!(harness.home_of(&a), None);
}

#[test]
fn switching_to_an_account_in_its_home_publishes_its_login_and_empties_the_home() {
    let harness = Harness::new().with_homes();
    let (a, b) = two_accounts(&harness);
    read(&harness);
    let home = harness.home_of(&b).unwrap();

    harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap();

    assert_eq!(harness.live(), Some("b1".into()));
    assert_eq!(harness.in_home(&b), None);
    assert_eq!(
        *harness.homes().emptied.lock().unwrap(),
        std::slice::from_ref(&home)
    );
    assert_eq!(harness.in_vault(&a), Some("a2".into()));
    read(&harness);
    assert_eq!(harness.in_home(&a), Some("a2".into()));
    assert_eq!(harness.in_vault(&a), None);
    assert!(
        !asked(&harness)[1..].contains(&home),
        "read the signed-in account's home"
    );
}

#[test]
fn switching_back_before_any_read_publishes_the_login_the_switch_away_captured() {
    let harness = Harness::new().with_homes();
    let (a, b) = two_accounts(&harness);
    read(&harness);
    let accounts = harness.accounts();

    accounts
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap();
    accounts
        .activate(AgentId::Claude, &a, Activation::Ordinary)
        .unwrap();
    assert_eq!(harness.live(), Some("a2".into()));
    accounts
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap();

    assert_eq!(harness.live(), Some("b1".into()));
}

#[test]
fn a_home_that_cannot_be_emptied_stops_the_switch_before_anything_is_replaced() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    let home = harness.home_of(&b).unwrap();
    harness.homes().refuses.lock().unwrap().push(home);

    let error = harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap_err();

    assert!(error.contains("nothing was replaced"), "{error}");
    assert_eq!(harness.live(), Some("a2".into()));
    assert_eq!(harness.in_home(&b), Some("b1".into()));
    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    harness.homes().refuses.lock().unwrap().clear();
    read(&harness);
    assert_eq!(harness.in_vault(&b), None);
    assert_eq!(harness.in_home(&b), Some("b1".into()));
}

#[test]
fn a_login_left_in_both_its_home_and_the_vault_settles_into_the_home() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    seeded_home(&harness, 1, claude("b", "b1"));

    read(&harness);

    assert_eq!(harness.in_home(&b), Some("b1".into()));
    assert_eq!(harness.in_vault(&b), None);
}

#[test]
fn a_newer_login_in_the_vault_replaces_the_one_its_home_holds() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    seeded_home(&harness, 1, claude("b", "b0"));

    read(&harness);

    assert_eq!(harness.in_home(&b), Some("b1".into()));
    assert_eq!(harness.in_vault(&b), None);
}

#[test]
fn switching_to_an_account_whose_vault_login_is_newer_publishes_that_one() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    seeded_home(&harness, 1, claude("b", "b0"));

    harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap();

    assert_eq!(harness.live(), Some("b1".into()));
    assert_eq!(harness.in_home(&b), None);
}

#[test]
fn a_home_holding_another_accounts_login_is_left_as_it_is_and_its_profile_read_from_the_vault() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    seeded_home(&harness, 1, claude("c", "c1"));

    let (_, fetched) = read_all(&harness);

    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    assert_eq!(harness.in_home(&b), Some("c1".into()));
    assert!(asked(&harness).is_empty(), "read the other account's home");
    assert_eq!(fetched, std::slice::from_ref(&b));
    harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap();
    assert_eq!(harness.live(), Some("b1".into()));
    assert_eq!(harness.in_home(&b), Some("c1".into()));
}

#[test]
fn switching_to_an_account_whose_only_login_is_another_accounts_fails_and_leaves_its_home_alone() {
    let harness = Harness::new().with_homes();
    let (a, b) = two_accounts(&harness);
    seeded_home(&harness, 1, claude("c", "c1"));
    harness.seed(|db| db.profiles[1].login = None);

    let error = harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap_err();

    assert!(error.contains("signs in as a different account"), "{error}");
    assert_eq!(harness.in_home(&b), Some("c1".into()));
    assert_eq!(harness.in_vault(&b), None);
    assert_eq!(harness.live(), Some("a2".into()));
    assert_eq!(harness.in_home(&a), None);
}

#[test]
fn a_login_its_home_does_not_read_back_stays_in_the_vault() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    *harness.homes().drops_writes.lock().unwrap() = true;

    read_all(&harness);

    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    assert_eq!(harness.in_home(&b), None);
}

#[test]
fn an_archived_account_is_neither_moved_nor_read() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    crate::limits::set_archived_at(harness.path(), AgentId::Claude, &[key("b")], true).unwrap();

    let (cards, fetched) = read_all(&harness);

    assert!(cards.is_empty() && fetched.is_empty());
    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    assert_eq!(harness.home_of(&b), None);
}

#[test]
fn an_account_archived_once_in_its_home_is_no_longer_read_there() {
    let harness = Harness::new().with_homes();
    two_accounts(&harness);
    read(&harness);
    crate::limits::set_archived_at(harness.path(), AgentId::Claude, &[key("b")], true).unwrap();

    let cards = read(&harness);

    assert!(cards.is_empty());
    assert_eq!(asked(&harness).len(), 1, "read the archived account's home");
}

#[test]
fn the_signed_in_accounts_home_is_not_read() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    let home = harness.home_of(&b).unwrap();
    harness.signed_in(Some(claude("b", "b2")));

    let cards = read(&harness);

    assert!(
        !asked(&harness)[1..].contains(&home),
        "read the signed-in account's home"
    );
    assert!(cards
        .iter()
        .all(|card| card.account.as_ref().unwrap().id != key("b")));
    assert_eq!(harness.in_home(&b), Some("b1".into()));
}

#[test]
fn nothing_moves_while_an_interrupted_switch_awaits_recovery() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    harness.interrupted(&b, Some(claude("a", "a2")));

    read_all(&harness);

    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    assert_eq!(harness.home_of(&b), None);
}

#[test]
fn a_home_an_older_version_forgot_is_taken_back_at_the_next_read() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    let home = harness.home_of(&b).unwrap();
    harness.seed(|db| db.profiles[1].home = None);

    let cards = read(&harness);

    assert!(harness.homes().deleted.lock().unwrap().is_empty());
    assert_eq!(harness.home_of(&b), Some(home.clone()));
    assert_eq!(harness.in_home(&b), Some("b1".into()));
    assert_eq!(asked(&harness).last(), Some(&home));
    assert!(cards.iter().any(|card| card.status == LimitsStatus::Ok
        && card.account.as_ref().map(|a| a.id.as_str()) == Some(key("b").as_str())));
}

#[test]
fn a_forgotten_home_that_cannot_be_read_is_left_for_a_later_read() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    let home = harness.home_of(&b).unwrap();
    harness.seed(|db| db.profiles[1].home = None);
    harness
        .homes()
        .unreadable
        .lock()
        .unwrap()
        .push(home.clone());

    read(&harness);

    assert!(harness.homes().deleted.lock().unwrap().is_empty());
    assert!(home.exists());
    assert_eq!(harness.home_of(&b), None);
    harness.homes().unreadable.lock().unwrap().clear();
    read(&harness);
    assert_eq!(harness.home_of(&b), Some(home));
    assert_eq!(harness.in_home(&b), Some("b1".into()));
}

#[test]
fn a_forgotten_home_whose_account_signed_in_again_goes_at_the_next_read() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    let home = harness.home_of(&b).unwrap();
    harness.seed(|db| {
        db.profiles[1].home = None;
        db.profiles[1].login = Some(claude("b", "b2"));
    });

    read(&harness);

    assert_eq!(
        *harness.homes().deleted.lock().unwrap(),
        std::slice::from_ref(&home)
    );
    assert_eq!(harness.in_home(&b), Some("b2".into()));
}

#[test]
fn a_removed_accounts_home_goes_at_the_next_read() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    let home = harness.home_of(&b).unwrap();

    harness.accounts().remove(&b).unwrap();
    read(&harness);

    assert_eq!(
        *harness.homes().deleted.lock().unwrap(),
        std::slice::from_ref(&home)
    );
    assert!(!home.exists());
    assert!(harness.vault().profiles.iter().all(|p| p.id != b));
}

#[test]
fn a_home_that_does_not_go_at_once_goes_at_a_later_read() {
    for cause in ["busy", "undeletable"] {
        let harness = Harness::new().with_homes();
        let (_, b) = two_accounts(&harness);
        read(&harness);
        let home = harness.home_of(&b).unwrap();
        let blocked = match cause {
            "busy" => &harness.homes().busy,
            _ => &harness.homes().undeletable,
        };
        blocked.lock().unwrap().push(home.clone());
        harness.accounts().remove(&b).unwrap();

        read(&harness);
        assert!(home.exists(), "{cause}: removed at once");

        blocked.lock().unwrap().clear();
        read(&harness);
        assert!(!home.exists(), "{cause}: never removed");
    }
}

#[test]
fn a_read_that_finds_the_home_already_named_moves_the_login_into_that_one() {
    let harness = Harness::new().with_homes();
    let (a, b) = two_accounts(&harness);
    let other = uuid::Uuid::new_v4().to_string();
    let opened = std::cell::Cell::new(0);
    let open = || {
        opened.set(opened.get() + 1);
        if opened.get() == 2 {
            harness.seed(|db| db.profiles[1].home = Some(other.clone()));
        }
        Store::open_existing(harness.path())
    };
    let accounts = harness.accounts();
    let homes = accounts.account_homes(AgentId::Claude).unwrap();
    let native = identity(AgentId::Claude, "a", "team");

    super::super::homes::settle(
        harness.path(),
        AgentId::Claude,
        Some(&native),
        &open,
        &homes,
    );

    let held: Vec<_> = harness
        .homes()
        .logins
        .lock()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(held, [account_home(&harness, &other)]);
    assert_eq!(harness.in_home(&b), Some("b1".into()));
    assert_eq!(harness.in_vault(&b), None);
    assert_eq!(harness.home_of(&a), None);
}

#[test]
fn a_new_sign_in_replaces_the_home_the_account_had() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    let old = harness.home_of(&b).unwrap();
    *harness.native.signed_in.borrow_mut() = Some(claude("b", "b2"));

    harness
        .accounts()
        .add(
            AgentId::Claude,
            uuid::Uuid::new_v4().to_string(),
            Some(b.clone()),
        )
        .unwrap();

    assert_eq!(harness.in_vault(&b), Some("b2".into()));
    assert_eq!(harness.home_of(&b), None);
    read(&harness);
    assert_eq!(
        *harness.homes().deleted.lock().unwrap(),
        std::slice::from_ref(&old)
    );
    assert_eq!(harness.in_home(&b), Some("b2".into()));
    assert_ne!(harness.home_of(&b), Some(old));
    assert_eq!(harness.in_vault(&b), None);
}

#[test]
fn signing_out_forgets_the_users_saved_logins_in_their_homes_too() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    let elsewhere = harness.saved(
        identity(AgentId::Claude, "a", "elsewhere"),
        claude_in("a", "elsewhere", "e1"),
    );
    read(&harness);
    let home = harness.home_of(&elsewhere).unwrap();

    harness.accounts().sign_out(AgentId::Claude).unwrap();

    let vault = harness.vault();
    let forgotten = vault.profiles.iter().find(|p| p.id == elsewhere).unwrap();
    assert!(forgotten.login.is_none() && forgotten.home.is_none());
    let listed = harness.accounts().list(AgentId::Claude).unwrap();
    assert!(listed
        .profiles
        .iter()
        .any(|p| p.id == elsewhere && p.needs_login));
    read(&harness);
    assert!(!home.exists(), "the signed-out user's home stayed");
    assert_eq!(
        harness.in_home(&b),
        Some("b1".into()),
        "another user's home went"
    );
}

#[test]
fn a_signed_out_accounts_home_that_does_not_go_at_once_is_never_taken_back() {
    let harness = Harness::new().with_homes();
    two_accounts(&harness);
    let elsewhere = harness.saved(
        identity(AgentId::Claude, "a", "elsewhere"),
        claude_in("a", "elsewhere", "e1"),
    );
    read(&harness);
    let home = harness.home_of(&elsewhere).unwrap();
    harness.accounts().sign_out(AgentId::Claude).unwrap();
    harness.homes().busy.lock().unwrap().push(home.clone());

    read(&harness);

    assert!(home.exists());
    assert_eq!(harness.home_of(&elsewhere), None);
    harness.homes().busy.lock().unwrap().clear();
    read(&harness);
    assert!(!home.exists(), "the signed-out user's home stayed");
    assert_eq!(harness.home_of(&elsewhere), None);
    assert!(harness
        .accounts()
        .list(AgentId::Claude)
        .unwrap()
        .profiles
        .iter()
        .any(|p| p.id == elsewhere && p.needs_login));
}

#[test]
fn signing_out_before_a_read_after_signing_in_again_leaves_the_old_home_to_go() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    let old = harness.home_of(&b).unwrap();
    *harness.native.signed_in.borrow_mut() = Some(claude("b", "b2"));
    harness
        .accounts()
        .add(
            AgentId::Claude,
            uuid::Uuid::new_v4().to_string(),
            Some(b.clone()),
        )
        .unwrap();
    harness.signed_in(Some(claude("b", "b3")));

    harness.accounts().sign_out(AgentId::Claude).unwrap();
    read(&harness);

    assert!(!old.exists(), "the replaced home stayed");
    assert_eq!(harness.home_of(&b), None);
    assert_eq!(harness.in_vault(&b), None);
}

#[test]
fn an_account_in_its_home_is_listed_as_signed_in() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);

    let listed = harness.accounts().list(AgentId::Claude).unwrap();

    let b = listed.profiles.iter().find(|p| p.id == b).unwrap();
    assert!(!b.needs_login);
}

#[test]
fn a_home_signed_out_of_its_login_asks_for_a_new_sign_in() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    *harness.homes().answer.lock().unwrap() = Some(crate::limits::SavedReadError::Http(
        crate::http::HttpError::Unauthorized,
    ));

    let cards = read(&harness);

    let card = cards
        .iter()
        .find(|card| card.account.as_ref().unwrap().id == key("b"))
        .unwrap();
    assert_eq!(card.status, LimitsStatus::Failed);
    assert_eq!(
        card.message.as_deref(),
        Some("This account's saved login has ended. Sign in again.")
    );
    assert_eq!(
        harness.in_home(&b),
        Some("b1".into()),
        "the home is left as it is"
    );
}

#[test]
fn a_failed_read_of_a_home_holds_the_next_one_back() {
    let harness = Harness::new().with_homes();
    two_accounts(&harness);
    read(&harness);
    *harness.homes().answer.lock().unwrap() = Some(crate::limits::SavedReadError::Http(
        crate::http::HttpError::RateLimited(crate::http::RateLimitReset::RetryAfter(7200)),
    ));
    read(&harness);

    let cards = read(&harness);

    assert_eq!(
        asked(&harness).len(),
        2,
        "read the home again while held back"
    );
    let card = cards
        .iter()
        .find(|card| card.account.as_ref().unwrap().id == key("b"))
        .unwrap();
    assert_eq!(
        card.message.as_deref(),
        Some("Usage refresh is rate limited. The last reading is retained.")
    );
}

#[test]
fn a_home_read_claude_code_could_not_give_says_why_on_the_card() {
    let harness = Harness::new().with_homes();
    two_accounts(&harness);
    read(&harness);
    *harness.homes().answer.lock().unwrap() = Some(crate::limits::SavedReadError::Unavailable(
        crate::limits::claude_cli::OUTDATED,
    ));

    let cards = read(&harness);

    let card = cards
        .iter()
        .find(|card| card.account.as_ref().unwrap().id == key("b"))
        .unwrap();
    assert_eq!(
        card.message.as_deref(),
        Some(crate::limits::claude_cli::OUTDATED)
    );
}

#[test]
fn a_finished_private_renewal_moves_into_the_home_in_its_logins_place() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    let profile = harness.seed(|db| {
        db.profiles[1].usage_renewal_owned = true;
        db.profiles[1].clone()
    });
    let renewals = Renewals::of(&Store::open_existing(harness.path()).unwrap());
    renewals.record_finished(&profile, Some(&claude("b", "b-renewed")));

    read(&harness);

    assert_eq!(harness.in_home(&b), Some("b-renewed".into()));
    assert_eq!(harness.in_vault(&b), None);
    assert!(
        matches!(renewals.finished(&profile), Ok(None)),
        "the renewal record stayed"
    );
}

#[test]
fn a_login_with_an_unfinished_private_renewal_stays_in_the_vault() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    let profile = harness.seed(|db| {
        db.profiles[1].usage_renewal_owned = true;
        db.profiles[1].clone()
    });
    let renewals = Renewals::of(&Store::open_existing(harness.path()).unwrap());
    renewals.record_finished(&profile, None);

    read_all(&harness);

    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    assert_eq!(harness.home_of(&b), None);
    assert!(
        renewals.finished(&profile).is_err(),
        "the renewal record went"
    );
    let error = harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap_err();
    assert!(error.contains("unfinished usage renewal"), "{error}");
}

#[test]
fn claude_keeps_homes_and_codex_does_not() {
    use super::super::NativeStores;
    let stores = super::super::AdapterStores;

    assert!(stores.homes(AgentId::Claude).is_some());
    assert!(stores.homes(AgentId::Codex).is_none());
}
