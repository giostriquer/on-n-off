//! Saved Claude accounts kept in their homes (`accounts::homes`): each login lives in one store at
//! a time, the signed-in account's in Claude Code's own and every other one in its home, where
//! Claude Code renews it.
use super::super::{store::Store, usage_renew::Renewals, Activation};
use super::fixture::{claude, identity, weekly_reading, Harness};
use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto};

/// Two saved Claude accounts, `a` signed in with a newer login than the one saved, `b` not.
fn two_accounts(harness: &Harness) -> (String, String) {
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.signed_in(Some(claude("a", "a2")));
    (a, b)
}

/// A read of the saved Claude accounts, as Limits makes one, and the cards it produced. A login
/// still in the vault is never read: every one of them belongs in a home by then.
fn read(harness: &Harness) -> Vec<ProviderLimitsDto> {
    let mut entries = Vec::new();
    harness
        .accounts()
        .refresh_usage(AgentId::Claude, true, &mut entries, &|profile, _| {
            panic!("read {} with its vault login", profile.id)
        });
    entries
}

fn key(user: &str) -> String {
    identity(AgentId::Claude, user, "team").observation_key()
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
    let home = harness.home_of(&b).unwrap();
    assert_eq!(*harness.homes().read.lock().unwrap(), [home]);
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
    // The signed-in account is Claude Code's own: nothing of it moves.
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
    assert_eq!(*harness.homes().emptied.lock().unwrap(), [home]);
    // The outgoing login is captured as ever, and moves into its own home at the next read.
    assert_eq!(harness.in_vault(&a), Some("a2".into()));
    read(&harness);
    assert_eq!(harness.in_home(&a), Some("a2".into()));
    assert_eq!(harness.in_vault(&a), None);
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
    // Both copies are the one login, which nothing renewed in between: the next read keeps the
    // home's and drops the vault's.
    assert_eq!(harness.in_home(&b), Some("b1".into()));
    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    harness.homes().refuses.lock().unwrap().clear();
    read(&harness);
    assert_eq!(harness.in_vault(&b), None);
    assert_eq!(harness.in_home(&b), Some("b1".into()));
}

/// A move interrupted before the login left the vault leaves it in both; the home's copy is the one
/// any renewal since went to, so it stands.
#[test]
fn a_login_left_in_both_its_home_and_the_vault_keeps_the_homes() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    let id = uuid::Uuid::new_v4().to_string();
    harness.seed(|db| db.profiles[1].home = Some(id.clone()));
    let dir = super::super::homes::dir(harness.path(), &id).unwrap();
    harness
        .homes()
        .logins
        .lock()
        .unwrap()
        .insert(dir, claude("b", "b-home"));

    read(&harness);

    assert_eq!(harness.in_home(&b), Some("b-home".into()));
    assert_eq!(harness.in_vault(&b), None);
}

#[test]
fn a_home_holding_another_accounts_login_is_left_as_it_is() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    let id = uuid::Uuid::new_v4().to_string();
    harness.seed(|db| db.profiles[1].home = Some(id.clone()));
    let dir = super::super::homes::dir(harness.path(), &id).unwrap();
    harness
        .homes()
        .logins
        .lock()
        .unwrap()
        .insert(dir, claude("c", "c1"));

    read(&harness);

    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    assert_eq!(harness.in_home(&b), Some("c1".into()));
}

#[test]
fn an_archived_account_is_neither_moved_nor_read() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    crate::limits::set_archived_at(harness.path(), AgentId::Claude, &[key("b")], true).unwrap();

    let cards = read(&harness);

    assert!(cards.is_empty());
    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    assert_eq!(harness.home_of(&b), None);
}

#[test]
fn nothing_moves_while_an_interrupted_switch_awaits_recovery() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    harness.interrupted(&b, Some(claude("a", "a2")));

    read(&harness);

    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    assert_eq!(harness.home_of(&b), None);
}

#[test]
fn removing_an_account_tears_its_home_down() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    let home = harness.home_of(&b).unwrap();

    harness.accounts().remove(&b).unwrap();

    assert_eq!(*harness.homes().deleted.lock().unwrap(), [home]);
    let vault = harness.vault();
    assert!(vault.profiles.iter().all(|p| p.id != b));
    assert!(vault.retired_homes.is_empty());
}

#[test]
fn a_home_its_client_holds_when_the_account_is_removed_goes_at_the_next_read() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    read(&harness);
    let home = harness.home_of(&b).unwrap();
    let id = harness.vault().profiles[1].home.clone().unwrap();
    harness.homes().busy.lock().unwrap().push(home.clone());

    harness.accounts().remove(&b).unwrap();

    assert!(harness.homes().deleted.lock().unwrap().is_empty());
    assert_eq!(harness.vault().retired_homes, [id]);
    harness.homes().busy.lock().unwrap().clear();
    read(&harness);
    assert_eq!(*harness.homes().deleted.lock().unwrap(), [home]);
    assert!(harness.vault().retired_homes.is_empty());
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

/// A renewal an earlier version made of a login it owned, finished but never published, moves in
/// with the login it renewed to, and no grant is sent for it again.
#[test]
fn a_finished_private_renewal_moves_into_the_home_in_its_login_s_place() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    let profile = harness.seed(|db| {
        db.profiles[1].usage_renewal_owned = true;
        db.profiles[1].clone()
    });
    let renewals = Renewals::of(&Store::open_existing(harness.path()).unwrap());
    renewals.record_finished(&profile, &claude("b", "b-renewed"));

    read(&harness);

    assert_eq!(harness.in_home(&b), Some("b-renewed".into()));
    assert_eq!(harness.in_vault(&b), None);
    // Asked about the login it renewed, the record would still answer had it stayed.
    assert!(
        matches!(renewals.finished(&profile), Ok(None)),
        "the renewal record stayed"
    );
}

/// Without homes, as for Codex, a saved login stays in the vault and is read from there.
#[test]
fn a_provider_without_homes_reads_saved_logins_from_the_vault() {
    let harness = Harness::new();
    let (_, b) = two_accounts(&harness);
    let fetched = std::sync::Mutex::new(Vec::new());
    let mut entries = Vec::new();

    harness
        .accounts()
        .refresh_usage(AgentId::Claude, true, &mut entries, &|profile, _| {
            fetched.lock().unwrap().push(profile.id.clone());
            super::super::usage::FetchResult {
                login: profile.login.clone(),
                result: Ok(weekly_reading(&profile.identity.observation_key())),
            }
        });

    assert_eq!(*fetched.lock().unwrap(), std::slice::from_ref(&b));
    assert_eq!(harness.in_vault(&b), Some("b1".into()));
}
