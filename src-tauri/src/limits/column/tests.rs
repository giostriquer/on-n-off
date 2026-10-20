use super::*;
use crate::accounts::model::Identity;
use crate::accounts::{roster_for_test, roster_with_logins_for_test};
use crate::dto::{AgentId, LimitWindowKind, LimitsAccountDto, LimitsStatus, Reading};
use crate::limits::outcome::Parked;
use crate::limits::window::fixture;

fn identity(provider: AgentId, user: &str, workspace: &str) -> Identity {
    Identity {
        provider,
        user_id: user.into(),
        workspace_id: workspace.into(),
    }
}

fn reading(provider: AgentId, id: &str, label: &str) -> ProviderLimitsDto {
    let mut card = ProviderLimitsDto::for_test(provider, id)
        .labelled(label)
        .with_reading(Reading {
            windows: vec![fixture(
                "primary",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                42.0,
                None,
            )],
            ..Reading::default()
        });
    card.current_account = false;
    card
}

fn scoped(identity: &Identity, label: &str) -> ProviderLimitsDto {
    let mut card = reading(identity.provider, &identity.observation_key(), label).with_legacy_id(
        &identity
            .legacy_key()
            .expect("a provider with saved accounts"),
    );
    card.saved_profile = true;
    card
}

fn history(identity: &Identity, label: &str) -> ProviderLimitsDto {
    reading(
        identity.provider,
        &identity
            .legacy_key()
            .expect("a provider with saved accounts"),
        label,
    )
}

fn roster(provider: AgentId, profiles: &[(Identity, &str)]) -> Roster {
    roster_for_test(provider, profiles, &[], &[])
}

fn no_ends() -> SignInDeadlines {
    SignInDeadlines::new()
}

fn ms(at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(at)
        .expect("an instant")
        .timestamp_millis()
}

fn ids(column: &[ColumnEntryDto]) -> Vec<&str> {
    column
        .iter()
        .map(|entry| entry.card.account.as_ref().map_or("", |a| a.id.as_str()))
        .collect()
}

fn replaced(entry: &ColumnEntryDto) -> Vec<(&str, Option<&str>)> {
    entry
        .replaced
        .iter()
        .map(|account| (account.id.as_str(), account.label.as_deref()))
        .collect()
}

#[test]
fn a_saved_profiles_reading_replaces_the_history_its_listed_legacy_id_names_and_lists_it() {
    let listed: serde_json::Value =
        serde_json::from_str(include_str!("../../accounts/fixtures/listed-profiles.json")).unwrap();
    for (provider, wire) in listed.as_object().unwrap() {
        let account: Identity = serde_json::from_value(wire["identity"].clone()).unwrap();
        let legacy = wire["legacyId"].as_str().unwrap();
        let mut card = scoped(&account, "you@example.com");
        card.account.as_mut().unwrap().legacy_id = None;
        let old = history(&account, "you@example.com");

        let column = column(
            &[card, old],
            &roster(account.provider, &[(account.clone(), "you@example.com")]),
            &no_ends(),
            0,
        );

        assert_eq!(
            ids(&column),
            [wire["observationId"].as_str().unwrap()],
            "{provider}"
        );
        assert_eq!(
            replaced(&column[0]),
            [(legacy, Some("you@example.com"))],
            "{provider}"
        );
        assert_eq!(
            column[0]
                .card
                .account
                .as_ref()
                .unwrap()
                .legacy_id
                .as_deref(),
            Some(legacy),
            "{provider}: an older app dropped the snapshot's legacy id, so the profile's stands in"
        );
        assert!(column[0].profile.is_some(), "{provider}");
    }
}

#[test]
fn a_saved_muse_logins_email_keyed_history_folds_into_its_scoped_card() {
    let you = identity(AgentId::Muse, "you@example.com", "");
    let other = identity(AgentId::Muse, "other@example.com", "");

    let column = column(
        &[
            scoped(&you, "you@example.com"),
            history(&you, "you@example.com"),
            history(&other, "other@example.com"),
        ],
        &roster(AgentId::Muse, &[(you.clone(), "you@example.com")]),
        &no_ends(),
        0,
    );

    assert_eq!(
        ids(&column),
        [you.observation_key().as_str(), "email:other@example.com"]
    );
    assert_eq!(
        replaced(&column[0]),
        [("email:you@example.com", Some("you@example.com"))]
    );
}

#[test]
fn the_email_matches_ignoring_ascii_case_and_surrounding_space_but_not_other_case() {
    let ascii = identity(AgentId::Codex, "user", "team");
    let folded = column(
        &[
            scoped(&ascii, " Person@Example.com "),
            history(&ascii, "person@example.com"),
        ],
        &roster(AgentId::Codex, &[(ascii.clone(), "person@example.com")]),
        &no_ends(),
        0,
    );
    assert_eq!(ids(&folded), [ascii.observation_key().as_str()]);

    let accented = identity(AgentId::Codex, "user", "équipe");
    let kept = column(
        &[
            scoped(&accented, "josé@example.com"),
            history(&accented, "JOSÉ@example.com"),
        ],
        &roster(AgentId::Codex, &[(accented.clone(), "josé@example.com")]),
        &no_ends(),
        0,
    );
    assert_eq!(
        ids(&kept),
        [accented.observation_key().as_str(), "équipe"],
        "only ASCII letters fold case, as the snapshot store's own rule does"
    );
}

#[test]
fn history_stays_beside_a_replacement_it_cannot_verify() {
    let account = identity(AgentId::Codex, "user", "team");
    let mut failed = scoped(&account, "you@example.com");
    failed.status = LimitsStatus::Failed;
    let other_email = scoped(&account, "other@example.com");
    let mut other_history = scoped(&account, "you@example.com");
    other_history.account.as_mut().unwrap().legacy_id = Some("other-team".into());
    let mut unlabelled = scoped(&account, "you@example.com");
    unlabelled.account.as_mut().unwrap().label = None;
    let mut current_history = history(&account, "you@example.com");
    current_history.current_account = true;

    for (case, entries) in [
        (
            "a failed read",
            vec![failed, history(&account, "you@example.com")],
        ),
        (
            "another email",
            vec![other_email, history(&account, "you@example.com")],
        ),
        (
            "another legacy id",
            vec![other_history, history(&account, "you@example.com")],
        ),
        (
            "no email",
            vec![unlabelled, history(&account, "you@example.com")],
        ),
        (
            "the signed-in history",
            vec![scoped(&account, "you@example.com"), current_history],
        ),
    ] {
        let column = column(&entries, &Roster::default(), &no_ends(), 0);
        assert_eq!(column.len(), 2, "{case}");
        assert!(
            column.iter().all(|entry| entry.replaced.is_empty()),
            "{case}"
        );
    }
}

#[test]
fn a_scoped_card_without_a_saved_profile_still_replaces_the_history_it_names() {
    let account = identity(AgentId::Codex, "user", "team");

    let column = column(
        &[
            scoped(&account, "you@example.com"),
            history(&account, "you@example.com"),
        ],
        &Roster::default(),
        &no_ends(),
        0,
    );

    assert_eq!(ids(&column), [account.observation_key().as_str()]);
    assert_eq!(replaced(&column[0]), [("team", Some("you@example.com"))]);
    assert!(column[0].profile.is_none());
}

#[test]
fn a_saved_profile_no_read_answered_for_is_an_unread_entry_of_its_own() {
    let read = identity(AgentId::Claude, "read", "team");
    let unread = identity(AgentId::Claude, "unread", "team");
    let unread_key = unread.observation_key();

    let column = column(
        &[scoped(&read, "read@example.com")],
        &roster_for_test(
            AgentId::Claude,
            &[
                (read.clone(), "read@example.com"),
                (unread.clone(), "unread@example.com"),
            ],
            &[],
            &[(unread_key.as_str(), "Client B")],
        ),
        &SignInDeadlines::from([(unread_key.clone(), ms("2026-09-30T12:00:00Z"))]),
        0,
    );

    assert_eq!(
        ids(&column),
        [
            read.observation_key().as_str(),
            unread.observation_key().as_str()
        ]
    );
    assert!(!column[0].unread);
    let entry = &column[1];
    assert!(entry.unread);
    assert_eq!(
        entry.standing, None,
        "unread, it stands live and the card shows its empty copy"
    );
    assert_eq!(
        entry.card.account,
        Some(LimitsAccountDto {
            id: unread.observation_key(),
            legacy_id: Some("unread".into()),
            label: Some("unread@example.com".into()),
        })
    );
    assert_eq!(entry.card.provider, AgentId::Claude);
    assert_eq!(entry.card.status, LimitsStatus::Ok);
    assert!(!entry.card.current_account && !entry.card.archived);
    assert!(entry.card.reading.windows.is_empty());
    assert_eq!(entry.category.as_deref(), Some("Client B"));
    let profile = serde_json::to_value(entry.profile.as_ref().unwrap()).unwrap();
    assert_eq!(profile["observationId"], unread.observation_key());
    assert_eq!(profile["signInEndsAt"], "2026-09-30T12:00:00Z");
    assert_eq!(profile["active"], false);
}

#[test]
fn an_unread_saved_profile_carries_its_login_state_as_the_account_list_decides_it() {
    const NOW: i64 = 1_790_000_000_000;
    const DAY: i64 = 86_400_000;
    let named = [
        "signed-in",
        "ended",
        "ending",
        "refused",
        "elsewhere",
        "usable",
    ]
    .map(|user| identity(AgentId::Claude, user, "team"));
    let profiles: Vec<_> = named
        .iter()
        .map(|identity| (identity.clone(), identity.user_id.as_str()))
        .collect();
    let roster = roster_with_logins_for_test(
        std::path::Path::new("/Users/me/column-login-states"),
        AgentId::Claude,
        &profiles,
        &[],
        &[],
        &|user| match user {
            "ended" | "ending" => Some(NOW - DAY),
            "elsewhere" => Some(NOW + 3 * DAY),
            _ => None,
        },
    );
    for (user, parked) in [
        ("signed-in", Parked::Refused),
        ("ended", Parked::Refused),
        ("refused", Parked::Refused),
        ("elsewhere", Parked::OtherAccount),
    ] {
        roster.park_for_test(
            &identity(AgentId::Claude, user, "team").observation_key(),
            parked,
        );
    }
    let mut current = reading(AgentId::Claude, &named[0].observation_key(), "signed-in");
    current.current_account = true;
    let listed = SignInDeadlines::from([(named[2].observation_key(), ms("2026-09-25T12:00:00Z"))]);

    let column = column(&[current], &roster, &listed, NOW);

    let states: Vec<_> = column
        .iter()
        .map(|entry| {
            let profile = serde_json::to_value(entry.profile.as_ref().unwrap()).unwrap();
            (entry.unread, profile["loginState"].clone())
        })
        .collect();
    assert_eq!(
        states,
        [
            (false, serde_json::json!({"state": "usable"})),
            (
                true,
                serde_json::json!({"state": "ended", "endsAt": "2026-09-20T14:13:20Z"})
            ),
            (
                true,
                serde_json::json!({"state": "ending", "endsAt": "2026-09-25T12:00:00Z"})
            ),
            (true, serde_json::json!({"state": "refused"})),
            (true, serde_json::json!({"state": "otherAccount"})),
            (true, serde_json::json!({"state": "usable"})),
        ]
    );
}

#[test]
fn the_signed_in_profile_is_dated_only_by_the_end_the_account_list_handed_over() {
    const NOW: i64 = 1_790_000_000_000;
    const DAY: i64 = 86_400_000;
    let account = identity(AgentId::Claude, "renewed", "team");
    let roster = roster_with_logins_for_test(
        std::path::Path::new("/Users/me/column-renewed"),
        AgentId::Claude,
        &[(account.clone(), "renewed@example.com")],
        &[],
        &[],
        &|_| Some(NOW - DAY),
    );
    let mut signed_in = scoped(&account, "renewed@example.com");
    signed_in.current_account = true;
    let stated = |ends: &SignInDeadlines| {
        let column = column(&[signed_in.clone()], &roster, ends, NOW);
        let profile = serde_json::to_value(column[0].profile.as_ref().unwrap()).unwrap();
        (
            profile["loginState"].clone(),
            profile.get("signInEndsAt").cloned(),
        )
    };

    assert_eq!(
        stated(&no_ends()),
        (serde_json::json!({"state": "usable"}), None),
        "the saved copy's passed end never dates the login signed in again in the terminal"
    );
    assert_eq!(
        stated(&SignInDeadlines::from([(
            account.observation_key(),
            ms("2026-10-05T12:00:00Z")
        )])),
        (
            serde_json::json!({"state": "ending", "endsAt": "2026-10-05T12:00:00Z"}),
            Some(serde_json::json!("2026-10-05T12:00:00Z"))
        )
    );
}

#[test]
fn an_unread_entry_is_archived_as_its_profile_is() {
    let put_away = identity(AgentId::Codex, "put-away", "team");
    let kept = identity(AgentId::Codex, "kept", "team");

    let column = column(
        &[],
        &roster_for_test(
            AgentId::Codex,
            &[
                (put_away.clone(), "put-away@example.com"),
                (kept.clone(), "kept@example.com"),
            ],
            &[put_away.observation_key()],
            &[],
        ),
        &no_ends(),
        0,
    );

    assert_eq!(
        column
            .iter()
            .map(|entry| (entry.unread, entry.card.archived))
            .collect::<Vec<_>>(),
        [(true, true), (true, false)]
    );
}

#[test]
fn the_signed_in_profile_is_the_one_the_native_read_names_and_none_when_it_names_nobody() {
    let a = identity(AgentId::Codex, "a", "team");
    let b = identity(AgentId::Codex, "b", "team");
    let roster = roster(
        AgentId::Codex,
        &[(a.clone(), "a@example.com"), (b.clone(), "b@example.com")],
    );
    let active = |column: &[ColumnEntryDto]| {
        column
            .iter()
            .filter(|entry| {
                entry
                    .profile
                    .as_ref()
                    .is_some_and(|p| serde_json::to_value(p).unwrap()["active"] == true)
            })
            .map(|entry| entry.card.account.as_ref().unwrap().id.clone())
            .collect::<Vec<_>>()
    };
    let mut signed_in = scoped(&b, "b@example.com");
    signed_in.current_account = true;
    let mut nobody = ProviderLimitsDto::for_test(AgentId::Codex, "unused");
    nobody.account = None;
    nobody.status = LimitsStatus::SignedOut;

    assert_eq!(
        active(&column(&[signed_in], &roster, &no_ends(), 0)),
        [b.observation_key()]
    );
    assert!(active(&column(&[nobody], &roster, &no_ends(), 0)).is_empty());
}

#[test]
fn history_merged_into_an_archived_card_is_listed_with_it() {
    let account = identity(AgentId::Codex, "user", "team");
    let mut card = scoped(&account, "you@example.com");
    card.archived = true;
    let mut old = history(&account, "you@example.com");
    old.archived = true;

    let column = column(&[card, old], &Roster::default(), &no_ends(), 0);

    assert_eq!(column.len(), 1);
    assert!(column[0].card.archived);
    assert_eq!(replaced(&column[0]), [("team", Some("you@example.com"))]);
}

#[test]
fn every_entry_carries_its_accounts_category_and_a_saved_accounts_profile() {
    let saved = identity(AgentId::Codex, "saved", "team");
    let saved_key = saved.observation_key();
    let history_card = reading(AgentId::Codex, "history", "old@example.com");

    let column = column(
        &[scoped(&saved, "saved@example.com"), history_card],
        &roster_for_test(
            AgentId::Codex,
            &[(saved.clone(), "saved@example.com")],
            &[],
            &[(saved_key.as_str(), "Client A"), ("history", "Old team")],
        ),
        &no_ends(),
        0,
    );

    assert_eq!(
        column
            .iter()
            .map(|entry| (entry.category.as_deref(), entry.profile.is_some()))
            .collect::<Vec<_>>(),
        [(Some("Client A"), true), (Some("Old team"), false)]
    );
}

#[test]
fn a_saved_profile_carries_the_sign_in_end_the_account_list_last_read() {
    let account = identity(AgentId::Claude, "user", "team");
    let ends = SignInDeadlines::from([(account.observation_key(), ms("2026-09-30T12:00:00Z"))]);

    let column = column(
        &[scoped(&account, "you@example.com")],
        &roster(AgentId::Claude, &[(account.clone(), "you@example.com")]),
        &ends,
        0,
    );

    let profile = serde_json::to_value(column[0].profile.as_ref().unwrap()).unwrap();
    assert_eq!(profile["signInEndsAt"], "2026-09-30T12:00:00Z");
}

#[test]
fn the_column_reaches_the_ui_as_the_reading_with_its_account_facts_beside_it() {
    let account = identity(AgentId::Codex, "user", "team");
    let column = column(
        &[
            scoped(&account, "you@example.com"),
            history(&account, "you@example.com"),
        ],
        &roster_for_test(
            AgentId::Codex,
            &[(account.clone(), "you@example.com")],
            &[],
            &[(account.observation_key().as_str(), "Client A")],
        ),
        &no_ends(),
        0,
    );

    let wire = serde_json::to_value(&column).unwrap();

    assert_eq!(wire[0]["account"]["id"], account.observation_key());
    assert_eq!(wire[0]["account"]["legacyId"], "team");
    assert_eq!(wire[0]["windows"][0]["id"], "primary");
    assert_eq!(wire[0]["category"], "Client A");
    assert_eq!(
        wire[0]["profile"]["observationId"],
        account.observation_key()
    );
    assert_eq!(
        wire[0]["replaced"],
        serde_json::json!([{ "id": "team", "label": "you@example.com" }])
    );
    assert!(wire[0].get("unread").is_none());
}
