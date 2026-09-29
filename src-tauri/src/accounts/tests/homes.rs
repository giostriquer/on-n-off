//! Saved Claude accounts kept in their homes (`accounts::homes`): each login lives in one store at
//! a time, the signed-in account's in Claude Code's own and every other one in its home, where
//! Claude Code renews it.
use super::super::{store::Store, usage::FetchResult, usage_renew::Renewals, Activation};
use super::fixture::{claude, claude_in, identity, weekly_reading, Harness};
use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto};
use std::sync::Mutex;

/// Two saved Claude accounts, `a` signed in with a newer login than the one saved, `b` not.
fn two_accounts(harness: &Harness) -> (String, String) {
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.signed_in(Some(claude("a", "a2")));
    (a, b)
}

/// A read of the saved Claude accounts, as Limits makes one: its cards, and the profiles read with
/// a login still in the vault.
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

/// A read in which every saved login that belongs in a home is in one by the time it is read.
fn read(harness: &Harness) -> Vec<ProviderLimitsDto> {
    let (cards, fetched) = read_all(harness);
    assert!(fetched.is_empty(), "read with a vault login: {fetched:?}");
    cards
}

fn key(user: &str) -> String {
    identity(AgentId::Claude, user, "team").observation_key()
}

/// The homes a read asked for usage, in order.
fn asked(harness: &Harness) -> Vec<std::path::PathBuf> {
    harness.homes().read.lock().unwrap().clone()
}

/// The directory of the home `id` names, whichever profile names it.
fn home_dir(harness: &Harness, id: &str) -> std::path::PathBuf {
    super::super::homes::dir(harness.path(), id).unwrap()
}

/// Seeds profile `profile`'s home, holding `login`, beside whatever the vault holds for it.
fn seeded_home(harness: &Harness, profile: usize, login: super::super::store::Login) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    harness.seed(|db| db.profiles[profile].home = Some(id.clone()));
    let dir = home_dir(harness, &id);
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
    assert_eq!(
        *harness.homes().emptied.lock().unwrap(),
        std::slice::from_ref(&home)
    );
    // The outgoing login is captured as ever, and moves into its own home at the next read, which
    // leaves the now signed-in account's emptied home alone.
    assert_eq!(harness.in_vault(&a), Some("a2".into()));
    read(&harness);
    assert_eq!(harness.in_home(&a), Some("a2".into()));
    assert_eq!(harness.in_vault(&a), None);
    assert!(
        !asked(&harness)[1..].contains(&home),
        "read the signed-in account's home"
    );
}

/// A switch away and back with no read between: the account's home was emptied, and its login is
/// the one the switch away captured into the vault.
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
    // The one login is in both, and nothing renewed it in between: the next read keeps one.
    assert_eq!(harness.in_home(&b), Some("b1".into()));
    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    harness.homes().refuses.lock().unwrap().clear();
    read(&harness);
    assert_eq!(harness.in_vault(&b), None);
    assert_eq!(harness.in_home(&b), Some("b1".into()));
}

/// Left in both by a move that stopped halfway, the one login settles into the home.
#[test]
fn a_login_left_in_both_its_home_and_the_vault_settles_into_the_home() {
    let harness = Harness::new().with_homes();
    let (_, b) = two_accounts(&harness);
    seeded_home(&harness, 1, claude("b", "b1"));

    read(&harness);

    assert_eq!(harness.in_home(&b), Some("b1".into()));
    assert_eq!(harness.in_vault(&b), None);
}

/// A vault login unlike the home's was put there since the home got its copy, from the native
/// store: it is the newer, and it replaces the home's.
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
    // Switching to it publishes the account's own login, from the vault.
    harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap();
    assert_eq!(harness.live(), Some("b1".into()));
}

/// A write that did not land, as one filed where Claude Code never reads, must not cost the login:
/// the vault keeps it.
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

/// An account signed in to outside on-n-off while its login was in its home: the signed-in read is
/// its card, so its home is not read beside it, nor its login renewed there.
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

/// A home its client holds, or one that could not be removed, is not forgotten: every read tries
/// again until it goes.
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

/// Two reads moving one login at once: the one that records its home second takes the home the
/// first recorded, so the login lands in the home the vault names and no other.
#[test]
fn a_read_that_finds_the_home_already_named_moves_the_login_into_that_one() {
    let harness = Harness::new().with_homes();
    let (a, b) = two_accounts(&harness);
    let other = uuid::Uuid::new_v4().to_string();
    let opened = std::cell::Cell::new(0);
    let open = || {
        opened.set(opened.get() + 1);
        // The second open is the move's own: another read recorded a home for b in between.
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
    assert_eq!(held, [home_dir(&harness, &other)]);
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

/// Logout may end every login of the user, so signing out forgets them all, homes included.
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

/// A home's read that failed backs off as a vault login's does, even forced: Claude Code, which may
/// renew the login each time, is not started again until the service's wait has passed.
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

/// A renewal an earlier version made of a login it owned, finished but never published, moves in
/// with the login it renewed to, and no grant is sent for it again.
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
    // Asked about the login it renewed, the record would still answer had it stayed.
    assert!(
        matches!(renewals.finished(&profile), Ok(None)),
        "the renewal record stayed"
    );
}

/// One whose outcome is unknown may have spent its refresh token: the login stays where it is and
/// is not switched to until it is signed in again.
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

/// Without homes, as for Codex, a saved login stays in the vault and is read from there.
#[test]
fn a_provider_without_homes_reads_saved_logins_from_the_vault() {
    let harness = Harness::new();
    let (_, b) = two_accounts(&harness);

    let (_, fetched) = read_all(&harness);

    assert_eq!(fetched, std::slice::from_ref(&b));
    assert_eq!(harness.in_vault(&b), Some("b1".into()));
}

/// The app's own stores: Claude's saved logins wait in homes, Codex's in the vault.
#[test]
fn claude_keeps_homes_and_codex_does_not() {
    use super::super::NativeStores;
    let stores = super::super::AdapterStores;

    assert!(stores.homes(AgentId::Claude).is_some());
    assert!(stores.homes(AgentId::Codex).is_none());
}
