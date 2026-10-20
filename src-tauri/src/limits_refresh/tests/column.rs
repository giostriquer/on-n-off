use super::*;
use crate::accounts::model::Identity;
use crate::accounts::roster_for_test;

fn saved(user: &str) -> Identity {
    Identity {
        provider: AgentId::Codex,
        user_id: user.into(),
        workspace_id: "team".into(),
    }
}

fn roster_of(users: &[&str], archived: &[String]) -> Roster {
    let profiles: Vec<_> = users
        .iter()
        .map(|user| (saved(user), format!("{user}@example.com")))
        .collect();
    let named: Vec<_> = profiles
        .iter()
        .map(|(identity, email)| (identity.clone(), email.as_str()))
        .collect();
    roster_for_test(AgentId::Codex, &named, None, archived, &[])
}

fn unread(column: &[ColumnEntryDto]) -> Vec<(String, bool)> {
    column
        .iter()
        .filter(|entry| entry.unread)
        .map(|entry| {
            (
                entry.card.account.as_ref().unwrap().label.clone().unwrap(),
                entry.card.archived,
            )
        })
        .collect()
}

fn signed_in() -> ProviderLimitsDto {
    let mut card = snapshot("signed-in", true, LimitsStatus::Ok);
    card.provider = AgentId::Codex;
    card
}

fn read(cache: &Cache, force: bool, roster: Option<Roster>) -> Served {
    read_provider(
        cache,
        Duration::from_secs(1800),
        force,
        &|_| native_read(vec![signed_in()]),
        &|_| false,
        &|_, _| roster.clone(),
        &|_| {},
    )
    .0
}

#[test]
fn the_column_lists_the_saved_profiles_the_last_saved_refresh_found_until_another_finds_others() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);

    let first = read(&cache, false, Some(roster_of(&["fresh"], &[])));
    assert_eq!(unread(&first.column), [("fresh@example.com".into(), false)]);
    assert_eq!(
        first.entries.len(),
        1,
        "a Rust consumer still gets the cards it read"
    );

    age_native(&cache, 1800);
    let native_only = read_provider(
        &cache,
        Duration::from_secs(1800),
        false,
        &|_| native_read(vec![signed_in()]),
        &|_| false,
        &|_, _| panic!("saved accounts are not due"),
        &|_| {},
    )
    .0;
    assert_eq!(unread(&native_only.column), unread(&first.column));

    let unreadable = read(&cache, true, None);
    assert_eq!(
        unread(&unreadable.column),
        unread(&first.column),
        "a saved refresh that could not open the vault keeps the profiles it last found"
    );

    let replaced = read(&cache, true, Some(roster_of(&["other"], &[])));
    assert_eq!(
        unread(&replaced.column),
        [("other@example.com".into(), false)]
    );
}

#[test]
fn archiving_puts_away_a_saved_profile_no_read_answered_for() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    read(&cache, false, Some(roster_of(&["fresh"], &[])));
    let id = saved("fresh").observation_key();

    let archived = archive_through_cache(&cache, std::slice::from_ref(&id), true, || Ok(true));

    assert_eq!(archived, Ok((Reading::Replaced(2), true)));
    let served = cache.served(cache.read.lock().unwrap().as_ref());
    assert_eq!(unread(&served.column), [("fresh@example.com".into(), true)]);
}

#[test]
fn a_category_edit_replaces_the_column_and_is_announced_only_when_it_changes() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    assert_eq!(
        category_through_cache(&cache, "signed-in", Some("Client A")),
        Reading::Unchanged(0),
        "nothing to edit before the first read"
    );
    read(&cache, false, Some(roster_of(&["fresh"], &[])));
    let fresh = saved("fresh").observation_key();

    assert_eq!(
        category_through_cache(&cache, "signed-in", Some("Client A")),
        Reading::Replaced(2)
    );
    assert_eq!(
        category_through_cache(&cache, &fresh, Some("Research")),
        Reading::Replaced(3)
    );
    assert_eq!(
        category_through_cache(&cache, &fresh, Some("Research")),
        Reading::Unchanged(3)
    );
    let served = cache.served(cache.read.lock().unwrap().as_ref());
    let categories: Vec<_> = served
        .column
        .iter()
        .map(|entry| entry.category.as_deref())
        .collect();
    assert_eq!(categories, [Some("Client A"), Some("Research")]);
    let profile = serde_json::to_value(served.column[1].profile.as_ref().unwrap()).unwrap();
    assert_eq!(profile["category"], "Research");

    assert_eq!(
        category_through_cache(&cache, &fresh, None),
        Reading::Replaced(4)
    );
}

#[test]
fn the_sign_in_ends_the_account_list_read_reach_the_column_and_are_announced_only_when_they_change()
{
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    read(&cache, false, Some(roster_of(&["fresh"], &[])));
    let fresh = saved("fresh").observation_key();
    let ends = SignInDeadlines::from([(fresh.clone(), "2026-09-30T12:00:00Z".to_string())]);

    assert_eq!(
        ends_through_cache(&cache, SignInDeadlines::new()),
        Reading::Unchanged(1)
    );
    assert_eq!(
        ends_through_cache(&cache, ends.clone()),
        Reading::Replaced(2)
    );
    assert_eq!(ends_through_cache(&cache, ends), Reading::Unchanged(2));

    let served = cache.served(cache.read.lock().unwrap().as_ref());
    let profile = serde_json::to_value(served.column[1].profile.as_ref().unwrap()).unwrap();
    assert_eq!(profile["signInEndsAt"], "2026-09-30T12:00:00Z");
}
