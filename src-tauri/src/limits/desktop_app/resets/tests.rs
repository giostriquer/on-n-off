use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::super::simple_cache::tests::{cached, entry_of, SENT};
use super::*;
use crate::accounts::model::Identity;
use crate::accounts::{roster_for_test, SignInDeadlines};
use crate::dto::{AgentId, ProviderLimitsDto};
use crate::limits::{column, scoped_account};

const ORG_A: &str = "00000000-0000-4000-8000-00000000000a";
const ORG_B: &str = "00000000-0000-4000-8000-00000000000b";
pub(in crate::limits::desktop_app) const NOW_MS: i64 = 1_794_350_000_000;
const SEEN_AT: &str = "2026-11-10T20:49:33Z";

fn grant(left: u64, ends_at: &str) -> Value {
    json!({
        "id": "grant-1", "label": "Launch reset", "type": "full",
        "resets_total": left, "resets_left": left,
        "starts_at": "2026-11-09T18:00:00+00:00", "ends_at": ends_at,
        "paused": false, "usable_now": true,
    })
}

fn answer(grants: Value) -> Value {
    json!({
        "five_hour": null,
        "limits": [],
        "cedar_ember": {"eligible": true, "ineligible_reason": null, "grants": grants},
    })
}

pub(in crate::limits::desktop_app) fn one_reset() -> Value {
    answer(json!([grant(1, "2027-03-09T07:59:00+00:00")]))
}

fn two_resets() -> Value {
    answer(json!([grant(2, "2027-03-09T07:59:00+00:00")]))
}

pub(in crate::limits::desktop_app) fn desktop_saw(home: &Path, organization: &str, body: &Value) {
    let key = usage_key(organization);
    cached(
        &paths::claude_desktop_cache_for(home),
        &key,
        &entry_of(&key, body.to_string().as_bytes(), Some(SENT)),
    );
}

fn home_seeing(answers: &[(&str, Value)]) -> PathBuf {
    let home = paths::scratch_dir("desktop-resets");
    for (organization, body) in answers {
        desktop_saw(&home, organization, body);
    }
    home
}

fn seen_for(home: &Path, organization: &str) -> Option<DesktopResetsDto> {
    seen(&paths::claude_desktop_cache_for(home), organization, NOW_MS)
}

fn counted(body: &Value) -> Option<(u32, Option<String>)> {
    seen_for(&home_seeing(&[(ORG_A, body.clone())]), ORG_A)
        .map(|seen| (seen.available_count, seen.next_expires_at))
}

fn left(count: u32, next: Option<&str>) -> Option<(u32, Option<String>)> {
    Some((count, next.map(str::to_owned)))
}

#[test]
fn the_usage_request_desktop_caches_names_the_organization_and_asks_for_resets() {
    assert_eq!(
        usage_key(ORG_A),
        "1/0/https://claude.ai/api/organizations/00000000-0000-4000-8000-00000000000a/usage?cedar_ember=1&skip_spend=1"
    );
}

#[test]
fn a_cached_answer_gives_the_resets_left_their_soonest_end_and_when_it_was_seen() {
    let home = home_seeing(&[(
        ORG_A,
        answer(json!([
            grant(1, "2027-03-09T07:59:00+00:00"),
            grant(2, "2027-01-15T12:00:00+00:00"),
        ])),
    )]);

    assert_eq!(
        seen_for(&home, ORG_A),
        Some(DesktopResetsDto {
            observed_at: SEEN_AT.into(),
            available_count: 3,
            next_expires_at: Some("2027-01-15T12:00:00Z".into()),
        })
    );
    assert_eq!(seen_for(&home, ORG_B), None, "another organization");
}

#[test]
fn a_reset_without_an_end_is_counted_and_has_no_expiry() {
    let mut unending = grant(1, "");
    unending["ends_at"] = Value::Null;
    unending["starts_at"] = Value::Null;

    assert_eq!(counted(&answer(json!([unending]))), left(1, None));
}

#[test]
fn only_resets_that_can_still_be_used_are_counted() {
    let live = grant(1, "2027-03-09T07:59:00+00:00");
    let with = |change: &dyn Fn(&mut Value)| {
        let mut other = grant(1, "2027-02-01T00:00:00+00:00");
        change(&mut other);
        other
    };
    for (other, why) in [
        (with(&|g| g["paused"] = json!(true)), "paused"),
        (with(&|g| g["resets_left"] = json!(0)), "none left"),
        (
            with(&|g| g["ends_at"] = json!("2026-11-10T00:00:00+00:00")),
            "already ended",
        ),
        (
            with(&|g| g["starts_at"] = json!("2026-12-01T00:00:00+00:00")),
            "not started",
        ),
        (
            with(&|g| g["ends_at"] = json!("soon")),
            "an end that is no date",
        ),
        (
            with(&|g| g["starts_at"] = json!("later")),
            "a start that is no date",
        ),
        (
            with(&|g| g["ends_at"] = json!(1_800_000_000)),
            "an end that is no text",
        ),
        (
            with(&|g| g["resets_left"] = json!("1")),
            "a count that is no number",
        ),
        (with(&|g| g["resets_left"] = json!(101)), "an absurd count"),
        (json!("grant"), "a grant that is no object"),
    ] {
        assert_eq!(
            counted(&answer(json!([other, live]))),
            left(1, Some("2027-03-09T07:59:00Z")),
            "{why}"
        );
    }
    assert_eq!(
        counted(&answer(json!([
            with(&|g| g["resets_left"] = json!(100)),
            live
        ]))),
        left(101, Some("2027-02-01T00:00:00Z")),
        "the largest count a grant may hold"
    );
}

#[test]
fn a_reset_ending_this_very_millisecond_is_over_and_one_starting_now_counts() {
    let mut boundary = grant(1, "2026-11-10T22:33:20+00:00");
    assert_eq!(
        counted(&answer(json!([boundary.clone()]))),
        None,
        "ends now"
    );

    boundary["ends_at"] = json!("2026-11-10T22:33:21+00:00");
    boundary["starts_at"] = json!("2026-11-10T22:33:20+00:00");
    assert_eq!(
        counted(&answer(json!([boundary]))),
        left(1, Some("2026-11-10T22:33:21Z")),
        "starts now"
    );
}

#[test]
fn an_answer_that_holds_no_usable_reset_shows_nothing() {
    for (body, why) in [
        (
            json!({"cedar_ember": null, "limits": []}),
            "the plain usage answer",
        ),
        (json!({"limits": []}), "no block at all"),
        (
            json!({"cedar_ember": {"eligible": false, "ineligible_reason": "surface", "grants": []}}),
            "withheld from this client",
        ),
        (
            json!({"cedar_ember": {"eligible": false, "ineligible_reason": "tier",
                "grants": [grant(1, "2027-03-09T07:59:00+00:00")]}}),
            "grants on an account that cannot use them",
        ),
        (
            json!({"cedar_ember": {"grants": [grant(1, "2027-03-09T07:59:00+00:00")]}}),
            "grants with no word on eligibility",
        ),
        (answer(json!([])), "no grants"),
        (answer(json!({"id": "grant-1"})), "grants that are no list"),
        (json!([1, 2]), "a body that is no object"),
    ] {
        assert_eq!(counted(&body), None, "{why}");
    }

    let home = paths::scratch_dir("desktop-resets");
    let key = usage_key(ORG_A);
    cached(
        &paths::claude_desktop_cache_for(&home),
        &key,
        &entry_of(&key, b"not json", Some(SENT)),
    );
    assert_eq!(seen_for(&home, ORG_A), None, "a body that is not JSON");
}

fn identity(user: &str, organization: &str) -> Identity {
    Identity {
        provider: AgentId::Claude,
        user_id: user.into(),
        workspace_id: organization.into(),
    }
}

fn saved(identity: &Identity, label: &str) -> ProviderLimitsDto {
    let mut card = ProviderLimitsDto::for_test(identity.provider, "replaced");
    card.account = Some(scoped_account(identity, Some(label.to_owned())));
    card.current_account = false;
    card.saved_profile = true;
    card
}

fn signed_in(identity: &Identity, label: &str) -> ProviderLimitsDto {
    let mut card = saved(identity, label);
    card.current_account = true;
    card.saved_profile = false;
    card
}

fn column_of(profiles: &[(Identity, &str)]) -> Vec<ColumnEntryDto> {
    let cards: Vec<ProviderLimitsDto> = profiles
        .iter()
        .map(|(identity, label)| saved(identity, label))
        .collect();
    column(
        &cards,
        &roster_for_test(AgentId::Claude, profiles, &[], &[]),
        &SignInDeadlines::new(),
        0,
    )
}

fn unsaved_column(cards: &[ProviderLimitsDto]) -> Vec<ColumnEntryDto> {
    column(
        cards,
        &roster_for_test(AgentId::Claude, &[], &[], &[]),
        &SignInDeadlines::new(),
        0,
    )
}

pub(in crate::limits::desktop_app) fn counts(column: &[ColumnEntryDto]) -> Vec<Option<u32>> {
    column
        .iter()
        .map(|entry| {
            entry
                .desktop_resets
                .as_ref()
                .map(|seen| seen.available_count)
        })
        .collect()
}

fn config_naming(home: &Path, user: &str, organization: &str) {
    std::fs::write(
        home.join(".claude.json"),
        json!({"oauthAccount": {
            "accountUuid": user, "organizationUuid": organization, "emailAddress": "a@example.com"
        }})
        .to_string(),
    )
    .unwrap();
}

fn attached(mut column: Vec<ColumnEntryDto>, home: &Path) -> Vec<ColumnEntryDto> {
    attach_from_cache(DesktopApp::Claude, &mut column, home, NOW_MS);
    column
}

#[test]
fn each_claude_account_gets_the_resets_desktop_saw_for_its_organization() {
    let home = home_seeing(&[(ORG_A, one_reset()), (ORG_B, two_resets())]);
    let column = attached(
        column_of(&[
            (identity("a", ORG_A), "a@example.com"),
            (identity("b", ORG_B), "b@example.com"),
            (
                identity("c", "00000000-0000-4000-8000-00000000000c"),
                "c@example.com",
            ),
        ]),
        &home,
    );

    assert_eq!(counts(&column), [Some(1), Some(2), None]);
    assert_eq!(
        column[0]
            .desktop_resets
            .as_ref()
            .map(|seen| seen.observed_at.as_str()),
        Some(SEEN_AT)
    );
}

#[test]
fn accounts_that_share_an_organization_get_no_resets() {
    let home = home_seeing(&[(ORG_A, one_reset()), (ORG_B, two_resets())]);
    let column = attached(
        column_of(&[
            (identity("a", ORG_A), "a@example.com"),
            (identity("teammate", ORG_A), "teammate@example.com"),
            (identity("b", ORG_B), "b@example.com"),
        ]),
        &home,
    );

    assert_eq!(counts(&column), [None, None, Some(2)]);
}

#[test]
fn a_column_served_again_loses_the_resets_desktop_no_longer_holds() {
    let home = home_seeing(&[(ORG_A, one_reset())]);
    let column = attached(column_of(&[(identity("a", ORG_A), "a@example.com")]), &home);
    assert_eq!(counts(&column), [Some(1)]);

    std::fs::remove_dir_all(paths::claude_desktop_cache_for(&home)).unwrap();

    assert_eq!(counts(&attached(column, &home)), [None]);
}

#[test]
fn the_signed_in_account_without_a_saved_profile_is_matched_by_the_config_that_names_it() {
    let home = home_seeing(&[(ORG_A, one_reset())]);
    config_naming(&home, "a", ORG_A);
    let card = signed_in(&identity("a", ORG_A), "a@example.com");
    let unsaved = saved(&identity("b", ORG_A), "b@example.com");

    assert_eq!(
        counts(&attached(
            unsaved_column(std::slice::from_ref(&card)),
            &home
        )),
        [Some(1)]
    );
    assert_eq!(
        counts(&attached(unsaved_column(&[card, unsaved]), &home)),
        [Some(1), None],
        "a card with no profile that is not the signed-in one names no organization"
    );
}

#[test]
fn a_card_that_names_nobody_gets_no_resets_while_the_config_still_names_an_organization() {
    let home = home_seeing(&[(ORG_A, one_reset())]);
    config_naming(&home, "a", ORG_A);
    let mut nobody = ProviderLimitsDto::for_test(AgentId::Claude, "replaced");
    nobody.account = None;
    nobody.current_account = true;
    let saved_a = identity("a", ORG_A);
    let profiles = [(saved_a.clone(), "a@example.com")];
    let column = column(
        &[nobody, saved(&saved_a, "a@example.com")],
        &roster_for_test(AgentId::Claude, &profiles, &[], &[]),
        &SignInDeadlines::new(),
        0,
    );

    assert_eq!(
        counts(&attached(column, &home)),
        [None, Some(1)],
        "the signed-out card shows none and does not take the saved account's row away"
    );
}

#[test]
fn a_signed_in_card_gets_no_resets_once_the_config_names_another_account() {
    let home = home_seeing(&[(ORG_A, one_reset()), (ORG_B, two_resets())]);
    let card = signed_in(&identity("a", ORG_A), "a@example.com");

    for (user, organization, why) in [
        ("b", ORG_B, "another account in another organization"),
        ("b", ORG_A, "another user of the same organization"),
        ("a", ORG_B, "the same user in another organization"),
    ] {
        config_naming(&home, user, organization);
        assert_eq!(
            counts(&attached(
                unsaved_column(std::slice::from_ref(&card)),
                &home
            )),
            [None],
            "{why}"
        );
    }
}

#[test]
fn the_resets_are_sent_beside_when_they_were_seen() {
    let home = home_seeing(&[(ORG_A, one_reset())]);
    let column = column_of(&[(identity("a", ORG_A), "a@example.com")]);
    let untouched = serde_json::to_value(&column[0]).unwrap();
    assert!(untouched.get("desktopResets").is_none(), "{untouched}");

    assert_eq!(
        serde_json::to_value(&attached(column, &home)[0]).unwrap()["desktopResets"],
        json!({
            "observedAt": SEEN_AT,
            "availableCount": 1,
            "nextExpiresAt": "2027-03-09T07:59:00Z",
        })
    );
}

#[test]
#[ignore = "reads the saved-reset answer Claude Desktop cached in the home ON_N_OFF_PROBE_HOME names"]
fn probe_real_home_desktop_resets() {
    let home = paths::probe_home();
    let organization =
        crate::limits::claude_config::read_claude_config_account(&home.join(".claude.json"))
            .and_then(|account| account.identity.organization_id);
    let seen = organization.as_deref().and_then(|organization| {
        seen(
            &paths::claude_desktop_cache_for(&home),
            organization,
            chrono::Utc::now().timestamp_millis(),
        )
    });
    println!(
        "Claude Code names an organization: {}",
        organization.is_some()
    );
    println!(
        "Desktop cached a usable reset answer for it: {}",
        seen.is_some()
    );
    if let Some(seen) = seen {
        println!("resets left: {}", seen.available_count);
        println!("next expiry known: {}", seen.next_expires_at.is_some());
        println!("seen at: {}", seen.observed_at);
    }
}
