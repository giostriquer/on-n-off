use super::*;
use crate::accounts::model::Identity;
use crate::accounts::{roster_for_test, Forgot};
use crate::dto::{ColumnEntryDto, LimitsAccountDto};
use crate::limits_refresh::account_actions::{
    archive_ids, archive_with, forget_with, remove_with, AccountStores, AtHome,
};
use std::cell::RefCell;

const EMAIL: &str = "shared@example.com";

fn saved_identity() -> Identity {
    Identity {
        provider: AgentId::Codex,
        user_id: "user-a".into(),
        workspace_id: "team".into(),
    }
}

fn key() -> String {
    saved_identity().observation_key()
}

fn card(id: &str, email: &str, current_account: bool) -> ProviderLimitsDto {
    ProviderLimitsDto {
        current_account,
        ..ProviderLimitsDto::for_test(AgentId::Codex, id).labelled(email)
    }
}

fn column_cards() -> Vec<ProviderLimitsDto> {
    vec![
        card("signed-in", "me@example.com", true),
        card(&key(), EMAIL, false),
        card("team", EMAIL, false),
        card("other", "other@example.com", false),
    ]
}

fn listed() -> Roster {
    roster_for_test(AgentId::Codex, &[(saved_identity(), EMAIL)], &[], &[])
}

fn column_cache() -> Cache {
    let cache = seeded(column_cards());
    cache.listing().roster = Some(listed());
    cache
}

fn held(cache: &Cache) -> Vec<String> {
    let read = cache.read.lock().unwrap();
    read.as_ref()
        .unwrap()
        .entries
        .iter()
        .filter_map(|entry| entry.account.as_ref().map(|account| account.id.clone()))
        .collect()
}

fn ids_of(accounts: &[LimitsAccountDto]) -> Vec<String> {
    accounts.iter().map(|account| account.id.clone()).collect()
}

struct Fake<'a> {
    cache: &'a Cache,
    checked: Result<(), String>,
    vault: Result<Forgot, String>,
    history: Result<(), String>,
    archive: Result<bool, String>,
    roster: Option<Roster>,
    calls: RefCell<Vec<String>>,
}

impl<'a> Fake<'a> {
    fn new(cache: &'a Cache) -> Self {
        Self {
            cache,
            checked: Ok(()),
            vault: Ok(Forgot {
                profile: true,
                changed: true,
                home_left: None,
            }),
            history: Ok(()),
            archive: Ok(true),
            roster: Some(roster_for_test(AgentId::Codex, &[], &[], &[])),
            calls: RefCell::default(),
        }
    }

    fn locked(&self) -> bool {
        self.cache.read.try_lock().is_err()
    }

    fn calls(&self) -> Vec<String> {
        self.calls.take()
    }
}

impl AccountStores for Fake<'_> {
    fn check_history(&self, replaced: &[LimitsAccountDto]) -> Result<(), String> {
        assert!(
            self.locked(),
            "the history is checked while the removal is planned"
        );
        let replaced = ids_of(replaced);
        self.calls.borrow_mut().push(format!("check {replaced:?}"));
        self.checked.clone()
    }
    fn forget_account(&self, account: &str, ids: &[String]) -> Result<Forgot, String> {
        assert!(!self.locked(), "the vault changes outside the cache lock");
        self.calls
            .borrow_mut()
            .push(format!("vault {account} {ids:?}"));
        self.vault.clone()
    }
    fn forget_history(
        &self,
        account: &str,
        replaced: &[LimitsAccountDto],
    ) -> Result<Vec<String>, String> {
        assert!(self.locked(), "history goes under the cache lock");
        let replaced = ids_of(replaced);
        self.calls
            .borrow_mut()
            .push(format!("history {account} {replaced:?}"));
        self.history
            .clone()
            .map(|()| replaced.into_iter().chain([account.to_string()]).collect())
    }
    fn set_archived(&self, ids: &[String], archived: bool) -> Result<bool, String> {
        assert!(self.locked(), "the archive is written under the cache lock");
        self.calls
            .borrow_mut()
            .push(format!("archive {archived} {ids:?}"));
        self.archive.clone()
    }
    fn roster(&self) -> Option<Roster> {
        assert!(
            !self.locked(),
            "the roster is reread outside the cache lock"
        );
        self.calls.borrow_mut().push("roster".into());
        self.roster.clone()
    }
    fn read_again(&self, force: bool) -> Changed {
        assert!(
            !self.locked(),
            "the provider is read again outside the cache lock"
        );
        self.calls
            .borrow_mut()
            .push(format!("read again, forced {force}"));
        read_revision::announce(self.cache.source);
        Changed {
            limits: true,
            accounts: false,
        }
    }
}

#[test]
fn removing_a_card_forgets_its_saved_login_and_the_history_its_column_entry_replaced_and_announces_once(
) {
    let cache = column_cache();
    let stores = Fake::new(&cache);
    let _ = read_revision::take_announced();
    let revision = cache.revision.current();

    assert_eq!(remove_with(&cache, &key(), &stores), Ok(()));
    assert_eq!(cache.revision.current(), revision + 1, "one replacement");

    assert_eq!(
        stores.calls(),
        [
            "check [\"team\"]".to_string(),
            format!("vault {} [\"team\", \"{}\"]", key(), key()),
            format!("history {} [\"team\"]", key()),
            "roster".to_string(),
        ]
    );
    assert_eq!(held(&cache), ["signed-in", "other"]);
    let column = cache.column(&cache.served(cache.read.lock().unwrap().as_ref()).entries);
    assert!(column.iter().all(|entry| entry.profile.is_none()));
    assert_eq!(
        read_revision::take_announced(),
        [Source::LimitsCodex, Source::Accounts]
    );
}

#[test]
fn a_vault_that_refuses_the_removal_changes_nothing_and_announces_nothing() {
    let cache = column_cache();
    let mut stores = Fake::new(&cache);
    stores.vault = Err("Recover the interrupted account change first.".into());
    let _ = read_revision::take_announced();
    let revision = cache.revision.current();

    assert_eq!(
        remove_with(&cache, &key(), &stores),
        Err("Recover the interrupted account change first.".into())
    );

    assert_eq!(
        stores.calls().len(),
        2,
        "only the history check and the vault were asked"
    );
    assert_eq!(held(&cache), ["signed-in", key().as_str(), "team", "other"]);
    assert_eq!(cache.revision.current(), revision);
    assert!(read_revision::take_announced().is_empty());
}

#[test]
fn history_that_cannot_go_once_the_saved_login_went_is_one_error_and_what_changed_is_announced() {
    let cache = column_cache();
    let mut stores = Fake::new(&cache);
    stores.history = Err("Account history changed. Refresh accounts before removing it.".into());
    let _ = read_revision::take_announced();

    assert_eq!(
        remove_with(&cache, &key(), &stores),
        Err(
            "The saved login was removed, but its usage history was not: Account history \
             changed. Refresh accounts before removing it."
                .into()
        )
    );

    assert_eq!(held(&cache), ["signed-in", key().as_str(), "team", "other"]);
    let column = cache.column(&cache.served(cache.read.lock().unwrap().as_ref()).entries);
    assert!(
        column.iter().all(|entry| entry.profile.is_none()),
        "the history stays as a remembered card without its saved login"
    );
    assert_eq!(
        read_revision::take_announced(),
        [Source::LimitsCodex, Source::Accounts]
    );
}

#[test]
fn a_home_left_for_a_later_read_joins_the_one_error_only_when_the_history_failed_too() {
    let left = || {
        Ok(Forgot {
            profile: true,
            changed: true,
            home_left: Some("Claude is busy with this home.".into()),
        })
    };
    let cache = column_cache();
    let mut stores = Fake::new(&cache);
    stores.vault = left();
    assert_eq!(remove_with(&cache, &key(), &stores), Ok(()));

    let cache = column_cache();
    let mut stores = Fake::new(&cache);
    stores.vault = left();
    stores.history = Err("disk full".into());
    assert_eq!(
        remove_with(&cache, &key(), &stores),
        Err(
            "The saved login was removed, but its usage history was not: disk full Its \
             sign-in home goes at a later read: Claude is busy with this home."
                .into()
        )
    );
}

#[test]
fn history_that_cannot_go_from_a_card_with_no_saved_login_is_the_bare_error_and_changes_nothing() {
    let cache = column_cache();
    let mut stores = Fake::new(&cache);
    stores.vault = Ok(Forgot::default());
    stores.history = Err("disk full".into());
    let _ = read_revision::take_announced();
    let revision = cache.revision.current();

    assert_eq!(
        remove_with(&cache, "other", &stores),
        Err("disk full".into())
    );

    assert_eq!(held(&cache), ["signed-in", key().as_str(), "team", "other"]);
    assert_eq!(cache.revision.current(), revision);
    assert!(
        read_revision::take_announced().is_empty(),
        "neither the vault nor the cache changed"
    );
}

#[test]
fn removing_a_remembered_card_with_no_saved_login_announces_only_its_column() {
    let cache = column_cache();
    let mut stores = Fake::new(&cache);
    stores.vault = Ok(Forgot::default());
    let _ = read_revision::take_announced();

    assert_eq!(remove_with(&cache, "other", &stores), Ok(()));

    assert_eq!(
        stores.calls(),
        ["check []", "vault other [\"other\"]", "history other []"]
    );
    assert_eq!(held(&cache), ["signed-in", key().as_str(), "team"]);
    assert_eq!(read_revision::take_announced(), [Source::LimitsCodex]);
}

#[test]
fn a_removal_whose_replaced_history_changed_hands_removes_nothing() {
    let cache = column_cache();
    let mut stores = Fake::new(&cache);
    stores.checked = Err("Account history changed. Refresh accounts before removing it.".into());
    let _ = read_revision::take_announced();

    assert_eq!(
        remove_with(&cache, &key(), &stores),
        Err("Account history changed. Refresh accounts before removing it.".into())
    );

    assert_eq!(
        stores.calls(),
        ["check [\"team\"]"],
        "refused before the saved login goes"
    );
    assert_eq!(held(&cache), ["signed-in", key().as_str(), "team", "other"]);
    assert!(read_revision::take_announced().is_empty());
}

#[test]
fn forgetting_a_snapshot_drops_every_card_it_forgot_once_the_disk_agrees_and_announces_once() {
    let cache = column_cache();
    let _ = read_revision::take_announced();
    let revision = cache.revision.current();

    assert_eq!(
        forget_with(&cache, || Ok(vec!["team".to_string(), key()])),
        Ok(vec!["team".to_string(), key()])
    );
    assert_eq!(
        held(&cache),
        ["signed-in", "other"],
        "the superseded history it forgot goes with it"
    );
    assert_eq!(cache.revision.current(), revision + 1);
    assert_eq!(read_revision::take_announced(), [Source::LimitsCodex]);

    assert_eq!(forget_with(&cache, || Ok(vec![key()])), Ok(vec![key()]));
    assert!(
        read_revision::take_announced().is_empty(),
        "nothing was removed, so nobody is sent to re-read"
    );
    assert_eq!(
        forget_with(&cache, || Err("disk failed".into())),
        Err("disk failed".into())
    );
    assert_eq!(
        forget_with(&cache, || Ok(vec!["signed-in".to_string()])),
        Ok(vec!["signed-in".to_string()])
    );
    assert_eq!(held(&cache), ["signed-in", "other"]);
    assert!(read_revision::take_announced().is_empty());
}

#[test]
fn the_signed_in_card_is_neither_removed_nor_archived() {
    let cache = column_cache();
    let stores = Fake::new(&cache);
    let _ = read_revision::take_announced();

    assert!(remove_with(&cache, "signed-in", &stores).is_err());
    assert!(archive_with(&cache, "signed-in", true, &stores).is_err());
    assert!(archive_with(&cache, "", true, &stores).is_err());

    assert!(stores.calls().is_empty());
    assert!(read_revision::take_announced().is_empty());
}

#[test]
fn archiving_a_card_covers_the_history_its_column_entry_replaced_and_unarchiving_reverses_it() {
    let cache = column_cache();
    let mut stores = Fake::new(&cache);
    stores.roster = Some(listed());
    let _ = read_revision::take_announced();
    let ids = format!("[\"team\", \"{}\"]", key());
    let revision = cache.revision.current();

    assert_eq!(archive_with(&cache, &key(), true, &stores), Ok(()));
    assert_eq!(cache.revision.current(), revision + 1, "one replacement");
    assert_eq!(
        stores.calls(),
        [format!("archive true {ids}"), "roster".to_string()]
    );
    assert_eq!(flags(&cache), [false, true, true, false]);
    assert_eq!(
        read_revision::take_announced(),
        [Source::LimitsCodex, Source::Accounts]
    );

    assert_eq!(archive_with(&cache, &key(), false, &stores), Ok(()));
    assert_eq!(cache.revision.current(), revision + 2);
    assert_eq!(
        stores.calls(),
        [
            format!("archive false {ids}"),
            "roster".to_string(),
            "read again, forced true".to_string(),
        ]
    );
    assert_eq!(flags(&cache), [false, false, false, false]);
    assert_eq!(
        read_revision::take_announced(),
        [Source::LimitsCodex, Source::Accounts],
        "the forced read it makes is announced with it, once"
    );
}

#[test]
fn an_archive_that_changed_nothing_rereads_and_announces_nothing() {
    let cache = column_cache();
    let mut stores = Fake::new(&cache);
    stores.archive = Ok(false);
    let _ = read_revision::take_announced();
    archive_with(&cache, "other", true, &stores).unwrap();
    let _ = stores.calls();
    let _ = read_revision::take_announced();
    let revision = cache.revision.current();

    assert_eq!(archive_with(&cache, "other", true, &stores), Ok(()));

    assert_eq!(stores.calls(), ["archive true [\"other\"]"]);
    assert!(read_revision::take_announced().is_empty());
    assert_eq!(cache.revision.current(), revision, "nothing replaced");
}

#[test]
fn a_failed_archive_write_flags_nothing_and_reads_nothing() {
    let cache = column_cache();
    let mut stores = Fake::new(&cache);
    stores.archive = Err("disk full".into());
    let _ = read_revision::take_announced();

    assert_eq!(
        archive_with(&cache, "other", false, &stores),
        Err("disk full".into())
    );

    assert_eq!(stores.calls(), ["archive false [\"other\"]"]);
    assert_eq!(flags(&cache), [false, false, false, false]);
    assert!(read_revision::take_announced().is_empty());
}

#[test]
fn archiving_an_unread_saved_profile_reaches_the_column_through_the_roster() {
    let spare = saved_identity();
    let cache = seeded(vec![card("signed-in", "me@example.com", true)]);
    cache.listing().roster = Some(listed());
    let mut stores = Fake::new(&cache);
    stores.roster = Some(roster_for_test(
        AgentId::Codex,
        &[(spare.clone(), EMAIL)],
        &[spare.observation_key()],
        &[],
    ));
    let _ = read_revision::take_announced();

    assert_eq!(archive_with(&cache, &key(), true, &stores), Ok(()));

    let column = cache.column(&cache.served(cache.read.lock().unwrap().as_ref()).entries);
    assert!(column
        .iter()
        .any(|entry| entry.unread && entry.card.archived));
    assert_eq!(
        read_revision::take_announced(),
        [Source::LimitsCodex, Source::Accounts]
    );
}

#[test]
fn the_archive_command_archives_the_ids_it_is_given_and_never_flags_the_signed_in_card() {
    let cache = column_cache();
    let stores = Fake::new(&cache);
    let _ = read_revision::take_announced();
    let given = |ids: &[&str]| {
        let ids: Vec<String> = ids.iter().map(|id| (*id).to_string()).collect();
        move |_: &[ProviderLimitsDto]| Ok(ids)
    };

    assert_eq!(
        archive_ids(&cache, true, &stores, given(&["signed-in", "other"])),
        Ok(())
    );
    assert_eq!(flags(&cache), [false, false, false, true]);
    assert_eq!(
        archive_ids(&cache, true, &stores, given(&[])),
        Err("No account to archive.".into())
    );
    assert_eq!(
        archive_ids(&cache, false, &stores, given(&["other"])),
        Ok(())
    );
    assert_eq!(flags(&cache), [false, false, false, false]);

    assert_eq!(
        stores.calls(),
        [
            "archive true [\"signed-in\", \"other\"]",
            "roster",
            "archive false [\"other\"]",
            "roster",
            "read again, forced true",
        ]
    );
    assert_eq!(
        read_revision::take_announced(),
        [
            Source::LimitsCodex,
            Source::Accounts,
            Source::LimitsCodex,
            Source::Accounts
        ]
    );
}

fn observed(card: ProviderLimitsDto, at: &str) -> ProviderLimitsDto {
    card.with_reading(crate::dto::Reading {
        windows: vec![crate::dto::LimitWindowDto {
            id: "weekly".into(),
            label: "Weekly".into(),
            kind: crate::dto::LimitWindowKind::Weekly,
            used_percent: 40.0,
            window_seconds: Some(604_800),
            resets_at: None,
            observed_at: at.into(),
            role: None,
            starts_on_first_message: false,
        }],
        ..crate::dto::Reading::default()
    })
}

thread_local! {
    static READS: RefCell<Vec<(AgentId, bool)>> = const { RefCell::new(Vec::new()) };
}

fn recorded_read(agent: AgentId, force: bool) -> Changed {
    READS.with(|reads| reads.borrow_mut().push((agent, force)));
    Changed::default()
}

fn other_identity() -> Identity {
    Identity {
        provider: AgentId::Codex,
        user_id: "user-b".into(),
        workspace_id: "elsewhere".into(),
    }
}

struct OnDisk {
    home: tempfile::TempDir,
    key: String,
    kept: String,
    cache: Cache,
    stores: AtHome,
}

impl OnDisk {
    fn saved() -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".on-n-off/accounts")).unwrap();
        crate::accounts::vault::tests::unlock_fixture(&home);
        let key = crate::accounts::saved_codex_fixture(
            home.path(),
            saved_identity(),
            serde_json::json!({}),
        );
        let kept = crate::accounts::saved_codex_fixture(
            home.path(),
            other_identity(),
            serde_json::json!({}),
        );
        let cards: Vec<_> = column_cards()
            .into_iter()
            .map(|card| observed(card, "2026-09-13T12:00:00Z"))
            .collect();
        for card in cards.iter().filter(|card| !card.current_account) {
            crate::limits::remember(home.path(), card.clone())
                .saved
                .unwrap();
        }
        let cache = seeded(cards);
        cache.listing().roster = Roster::read(home.path(), AgentId::Codex);
        let stores = AtHome {
            home: home.path().to_path_buf(),
            agent: AgentId::Codex,
            read: recorded_read,
        };
        READS.with(|reads| reads.borrow_mut().clear());
        Self {
            home,
            key,
            kept,
            cache,
            stores,
        }
    }

    fn archive_all(&self) {
        let all = ["team".to_string(), self.key.clone(), "other".to_string()];
        crate::limits::set_archived_at(self.home.path(), AgentId::Codex, &all, true).unwrap();
    }

    fn profiles(&self) -> usize {
        Roster::read(self.home.path(), AgentId::Codex)
            .unwrap()
            .profiles()
            .len()
    }

    fn remembered(&self) -> Vec<String> {
        let mut ids: Vec<_> = crate::limits::remembered(self.home.path(), AgentId::Codex)
            .into_iter()
            .filter_map(|card| card.account.map(|account| account.id))
            .collect();
        ids.sort();
        ids
    }

    fn archived(&self) -> std::collections::BTreeSet<String> {
        crate::limits::archived(self.home.path(), AgentId::Codex)
    }

    fn column(&self) -> Vec<ColumnEntryDto> {
        self.cache.column(
            &self
                .cache
                .served(self.cache.read.lock().unwrap().as_ref())
                .entries,
        )
    }
}

fn set(ids: &[&str]) -> std::collections::BTreeSet<String> {
    ids.iter().map(|id| (*id).to_string()).collect()
}

#[test]
fn removing_a_saved_account_in_one_call_leaves_no_profile_history_or_archive_entry_on_disk() {
    let disk = OnDisk::saved();
    disk.archive_all();
    let _ = read_revision::take_announced();

    assert_eq!(remove_with(&disk.cache, &disk.key, &disk.stores), Ok(()));

    assert_eq!(
        disk.profiles(),
        1,
        "the saved login is gone, the other stays"
    );
    assert_eq!(disk.remembered(), ["other"]);
    assert_eq!(disk.archived(), set(&["other"]));
    assert_eq!(held(&disk.cache), ["signed-in", "other"]);
    let column = disk.column();
    assert!(
        column
            .iter()
            .all(|entry| entry.card.account.as_ref().is_none_or(|a| a.id != disk.key)),
        "the removed login does not come back as an unread card"
    );
    let listed: Vec<_> = column
        .iter()
        .filter_map(|entry| entry.profile.as_ref())
        .map(|profile| serde_json::to_value(profile).unwrap()["observationId"].clone())
        .collect();
    assert_eq!(
        listed,
        [serde_json::json!(disk.kept)],
        "the roster was read again"
    );
    assert_eq!(
        read_revision::take_announced(),
        [Source::LimitsCodex, Source::Accounts]
    );
}

#[test]
fn archiving_a_saved_account_writes_it_and_the_history_it_replaced_to_disk_and_unarchiving_reads_it_again_forced(
) {
    let disk = OnDisk::saved();

    assert_eq!(
        archive_with(&disk.cache, &disk.key, true, &disk.stores),
        Ok(())
    );
    assert_eq!(disk.archived(), set(&["team", &disk.key]));
    assert!(READS.with(|reads| reads.borrow().is_empty()));

    assert_eq!(
        archive_with(&disk.cache, &disk.key, false, &disk.stores),
        Ok(())
    );
    assert!(disk.archived().is_empty());
    assert_eq!(
        READS.with(|reads| reads.borrow().clone()),
        [(AgentId::Codex, true)]
    );
}

#[test]
fn a_removal_whose_history_changed_hands_on_disk_keeps_the_saved_login_and_every_snapshot() {
    let disk = OnDisk::saved();
    disk.archive_all();
    let elsewhere = observed(
        card("team", "second@example.com", false),
        "2026-09-14T12:00:00Z",
    );
    crate::limits::remember(disk.home.path(), elsewhere)
        .saved
        .unwrap();
    let before = (disk.remembered(), disk.archived());
    let _ = read_revision::take_announced();

    assert_eq!(
        remove_with(&disk.cache, &disk.key, &disk.stores),
        Err("Account history changed. Refresh accounts before removing it.".into())
    );

    assert_eq!(disk.profiles(), 2, "the saved login stays");
    assert_eq!((disk.remembered(), disk.archived()), before);
    assert!(read_revision::take_announced().is_empty());
}
