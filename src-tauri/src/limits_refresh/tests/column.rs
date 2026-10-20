use super::*;
use crate::accounts::model::Identity;
use crate::accounts::roster_for_test;
use std::sync::mpsc;

fn saved(user: &str) -> Identity {
    Identity {
        provider: AgentId::Codex,
        user_id: user.into(),
        workspace_id: "team".into(),
    }
}

fn roster_of(users: &[&str], categories: &[(&str, &str)]) -> Roster {
    let profiles: Vec<_> = users
        .iter()
        .map(|user| (saved(user), format!("{user}@example.com")))
        .collect();
    let named: Vec<_> = profiles
        .iter()
        .map(|(identity, email)| (identity.clone(), email.as_str()))
        .collect();
    roster_for_test(AgentId::Codex, &named, &[], categories)
}

fn signed_in_as(user: &str) -> ProviderLimitsDto {
    let mut card = snapshot(&saved(user).observation_key(), true, LimitsStatus::Ok);
    card.provider = AgentId::Codex;
    card
}

fn age(cache: &Cache) {
    let mut cached = cache.read.lock().unwrap();
    let read = cached.as_mut().unwrap();
    read.native.refreshed_at = Instant::now() - Duration::from_secs(1800);
}

fn read(
    cache: &Cache,
    native: &str,
    force: bool,
    listed: &dyn Fn(bool, &mut Vec<ProviderLimitsDto>) -> Option<Roster>,
) {
    read_provider(
        cache,
        Duration::from_secs(1800),
        force,
        &Native {
            read: &|_| native_read(vec![signed_in_as(native)]),
            login: &no_login,
        },
        &|_| false,
        listed,
        &|_| {},
    );
}

fn served_column(cache: &Cache) -> Vec<ColumnEntryDto> {
    let entries = cache.served(cache.read.lock().unwrap().as_ref()).entries;
    cache.column(&entries)
}

fn unread(column: &[ColumnEntryDto]) -> Vec<String> {
    column
        .iter()
        .filter(|entry| entry.unread)
        .map(|entry| entry.card.account.as_ref().unwrap().label.clone().unwrap())
        .collect()
}

fn signed_in(column: &[ColumnEntryDto]) -> Vec<String> {
    column
        .iter()
        .filter(|entry| {
            entry
                .profile
                .as_ref()
                .is_some_and(|p| serde_json::to_value(p).unwrap()["active"] == true)
        })
        .map(|entry| entry.card.account.as_ref().unwrap().id.clone())
        .collect()
}

fn not_due(_: bool, _: &mut Vec<ProviderLimitsDto>) -> Option<Roster> {
    panic!("saved accounts are not due")
}

#[test]
fn the_column_lists_the_saved_profiles_the_last_saved_refresh_found_until_another_finds_others() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);

    read(&cache, "me", false, &|_, _| {
        Some(roster_of(&["fresh"], &[]))
    });
    assert_eq!(unread(&served_column(&cache)), ["fresh@example.com"]);
    assert_eq!(
        cache
            .served(cache.read.lock().unwrap().as_ref())
            .entries
            .len(),
        1,
        "a Rust consumer still gets the cards it read"
    );

    age(&cache);
    read(&cache, "me", false, &not_due);
    assert_eq!(unread(&served_column(&cache)), ["fresh@example.com"]);

    read(&cache, "me", true, &|_, _| None);
    assert_eq!(
        unread(&served_column(&cache)),
        ["fresh@example.com"],
        "a saved refresh that could not load the vault keeps the profiles it last found"
    );

    read(&cache, "me", true, &|_, _| Some(roster_of(&["other"], &[])));
    assert_eq!(unread(&served_column(&cache)), ["other@example.com"]);
}

#[test]
fn a_switch_made_in_the_cli_moves_the_signed_in_profile_with_the_next_native_read() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    read(&cache, "a", false, &|_, _| {
        Some(roster_of(&["a", "b"], &[]))
    });
    assert_eq!(
        signed_in(&served_column(&cache)),
        [saved("a").observation_key()]
    );

    age(&cache);
    read(&cache, "b", false, &not_due);

    assert_eq!(
        signed_in(&served_column(&cache)),
        [saved("b").observation_key()],
        "no saved refresh ran, and the dot follows the native read"
    );
}

#[test]
fn a_roster_reread_reaches_the_column_without_a_saved_refresh_and_is_announced_only_when_it_changes(
) {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    read(&cache, "me", false, &|_, _| Some(roster_of(&[], &[])));
    assert!(served_column(&cache)[0].profile.is_none());

    assert_eq!(
        reread_roster(&cache, || Some(roster_of(&["me", "fresh"], &[]))),
        Reading::Replaced(2),
        "a login saved by remembering, or a vault just unlocked"
    );
    let column = served_column(&cache);
    assert_eq!(signed_in(&column), [saved("me").observation_key()]);
    assert_eq!(unread(&column), ["fresh@example.com"]);

    assert_eq!(
        reread_roster(&cache, || Some(roster_of(&["me", "fresh"], &[]))),
        Reading::Unchanged(2)
    );
    assert_eq!(
        reread_roster(&cache, || None),
        Reading::Unchanged(2),
        "a vault this session has not unlocked leaves the roster as it was"
    );
    assert_eq!(unread(&served_column(&cache)), ["fresh@example.com"]);
}

#[test]
fn a_forgotten_category_leaves_the_column_with_the_reread() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    let me = saved("me").observation_key();
    read(&cache, "me", false, &|_, _| {
        Some(roster_of(&["me"], &[(me.as_str(), "Client A")]))
    });
    assert_eq!(
        served_column(&cache)[0].category.as_deref(),
        Some("Client A")
    );

    reread_roster(&cache, || Some(roster_of(&["me"], &[])));

    assert_eq!(served_column(&cache)[0].category, None);
}

#[test]
fn a_category_edit_completes_while_a_read_holds_the_read_lock_and_the_read_keeps_it() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    let me = saved("me").observation_key();
    read(&cache, "me", false, &|_, _| Some(roster_of(&["me"], &[])));
    let edited = || Some(roster_of(&["me"], &[(me.as_str(), "Client A")]));

    let held = cache.read.lock().unwrap();
    std::thread::scope(|scope| {
        let (done, finished) = mpsc::channel();
        let cache = &cache;
        scope.spawn(move || {
            reread_roster(cache, edited);
            let _ = done.send(());
        });
        let answered = finished.recv_timeout(Duration::from_secs(5));
        drop(held);
        assert!(answered.is_ok(), "the edit waited for the read lock");
    });
    assert_eq!(
        served_column(&cache)[0].category.as_deref(),
        Some("Client A")
    );

    read(&cache, "me", true, &|_, _| {
        reread_roster(&cache, edited);
        Some(roster_of(&["me"], &[]))
    });
    assert_eq!(
        served_column(&cache)[0].category.as_deref(),
        Some("Client A"),
        "a saved refresh that loaded the vault before the edit does not undo it"
    );
}

#[test]
fn a_roster_reread_never_reaches_the_credential_store() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    let home = tempfile::tempdir().unwrap();
    let vault = home.path().join(".on-n-off/accounts/vault.enc");
    std::fs::create_dir_all(vault.parent().unwrap()).unwrap();
    std::fs::write(&vault, b"sealed elsewhere").unwrap();

    assert_eq!(
        reread_roster(&cache, || Roster::read(home.path(), AgentId::Codex)),
        Reading::Unchanged(0),
        "a test build panics if the reread asks the OS for the vault key"
    );
}

#[test]
fn an_account_list_hands_its_sign_in_ends_and_a_roster_reread_over_in_one_replacement() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    read(&cache, "me", false, &|_, _| Some(roster_of(&["me"], &[])));
    let me = saved("me").observation_key();
    let ends = SignInDeadlines::from([(me.clone(), "2026-09-30T12:00:00Z".to_string())]);
    let listed = || Some(roster_of(&["me"], &[(me.as_str(), "Client A")]));

    assert_eq!(
        relist(&cache, Some(SignInDeadlines::new()), || Some(roster_of(
            &["me"],
            &[]
        ))),
        Reading::Unchanged(1)
    );
    assert_eq!(
        relist(&cache, Some(ends.clone()), listed),
        Reading::Replaced(2),
        "a new end and a changed roster bump the revision once"
    );
    assert_eq!(
        relist(&cache, Some(ends.clone()), listed),
        Reading::Unchanged(2)
    );
    assert_eq!(
        relist(&cache, Some(ends), || None),
        Reading::Unchanged(2),
        "a vault this session has not unlocked keeps the roster"
    );

    let column = served_column(&cache);
    let profile = serde_json::to_value(column[0].profile.as_ref().unwrap()).unwrap();
    assert_eq!(profile["signInEndsAt"], "2026-09-30T12:00:00Z");
    assert_eq!(column[0].category.as_deref(), Some("Client A"));
}

#[test]
fn an_account_change_reads_the_provider_afresh_and_relists_the_saved_profiles_even_when_it_cannot()
{
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    read(&cache, "me", false, &|_, _| {
        Some(roster_of(&["me", "kept", "removed"], &[]))
    });
    assert_eq!(
        signed_in(&served_column(&cache)),
        [saved("me").observation_key()]
    );

    account_changed_in(
        &cache,
        || Some(roster_of(&["me", "kept"], &[])),
        || read(&cache, "kept", false, &|_, _| None),
    );

    let column = served_column(&cache);
    assert_eq!(
        signed_in(&column),
        [saved("kept").observation_key()],
        "the change cleared the cached read, so the switch is read at once"
    );
    assert_eq!(
        unread(&column),
        ["me@example.com"],
        "the removed profile does not linger although the saved refresh listed nothing"
    );
}

#[test]
fn archiving_an_unread_saved_profile_reaches_the_column_through_the_roster() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    read(&cache, "me", false, &|_, _| {
        Some(roster_of(&["spare"], &[]))
    });
    let spare = saved("spare").observation_key();
    let archived = || {
        let profiles = [(saved("spare"), "spare@example.com")];
        Some(roster_for_test(
            AgentId::Codex,
            &profiles,
            std::slice::from_ref(&spare),
            &[],
        ))
    };

    set_archived_with(
        &cache,
        std::slice::from_ref(&spare),
        true,
        || Ok(true),
        |_| {},
        || {
            reread_roster(&cache, archived);
        },
    )
    .unwrap();

    let column = served_column(&cache);
    assert!(column
        .iter()
        .any(|entry| entry.unread && entry.card.archived));
}
