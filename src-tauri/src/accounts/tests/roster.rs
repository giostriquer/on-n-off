use super::super::usage::FetchResult;
use super::fixture::{claude, codex, identity, weekly_reading, Harness, Heard};
use crate::accounts::Roster;
use crate::dto::{AgentId, ProviderLimitsDto};

fn answered(harness: &Harness, provider: AgentId) -> Option<Roster> {
    let mut entries: Vec<ProviderLimitsDto> = Vec::new();
    harness
        .accounts()
        .refresh_usage(provider, false, &mut entries, &|profile, _| FetchResult {
            login: profile.login.clone(),
            result: Ok(crate::limits::Accepted::for_test(weekly_reading(
                &profile.identity.observation_key(),
            ))),
        })
}

fn in_column(listed: &crate::accounts::Listed) -> serde_json::Value {
    serde_json::to_value(listed.in_column(&crate::accounts::SignInDeadlines::new(), None, 0))
        .unwrap()
}

#[test]
fn the_saved_refresh_lists_each_profile_as_the_account_list_does_once_given_its_ends_and_who_is_signed_in(
) {
    let harness = Harness::new().with_homes();
    let a = identity(AgentId::Claude, "a", "team");
    let b = identity(AgentId::Claude, "b", "team");
    harness.saved(a.clone(), claude("a", "a1"));
    harness.saved(b.clone(), claude("b", "b1"));
    harness.saved(identity(AgentId::Codex, "c", "team"), codex("c", "c1"));
    harness.signed_in(Some(claude("a", "a2")));
    harness
        .accounts()
        .set_account_category(AgentId::Claude, &b.observation_key(), "Client A")
        .unwrap();
    harness
        .accounts()
        .set_account_category(AgentId::Claude, "history", "Old team")
        .unwrap();
    crate::limits::set_archived_at(
        harness.path(),
        AgentId::Claude,
        &[b.observation_key()],
        true,
    )
    .unwrap();
    *harness.native.deadline.borrow_mut() = Some(super::super::model::SignInDeadline {
        identity: a.clone(),
        ends_at_ms: 1_792_081_900_000,
    });

    let roster = answered(&harness, AgentId::Claude).unwrap();
    harness.heard();
    let listed = harness.accounts().list(AgentId::Claude).unwrap();

    let ends = listed.sign_in_deadlines();
    let signed_in = a.observation_key();
    let now = chrono::Utc::now().timestamp_millis();
    let read: Vec<_> = roster
        .profiles()
        .iter()
        .map(|p| serde_json::to_value(p.in_column(&ends, Some(&signed_in), now)).unwrap())
        .collect();
    let shown = serde_json::to_value(&listed).unwrap()["profiles"].clone();
    assert_eq!(serde_json::Value::from(read), shown);
    assert_eq!(
        shown[0]["loginState"],
        serde_json::json!({"state": "ended", "endsAt": "2026-10-15T16:31:40Z"})
    );
    assert_eq!(
        roster
            .profiles()
            .iter()
            .map(|p| in_column(p)["active"].clone())
            .collect::<Vec<_>>(),
        [false, false],
        "who is signed in is the native read's, decided as the column is built"
    );
    assert_eq!(roster.category(&b.observation_key()), Some("Client A"));
    assert_eq!(roster.category("history"), Some("Old team"));
    assert_eq!(
        harness.heard(),
        [(
            Heard::Listed(
                AgentId::Claude,
                crate::accounts::SignInDeadlines::from([(
                    a.observation_key(),
                    "2026-10-15T16:31:40Z".to_string()
                )])
            ),
            true
        )],
        "the account list hands Limits the sign-in ends it read, and Limits rereads its roster"
    );
}

#[test]
fn the_saved_refresh_reads_no_sign_in_end_and_the_native_login_only_once() {
    let harness = Harness::new().with_homes();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.signed_in(Some(claude("a", "a2")));
    answered(&harness, AgentId::Claude);
    harness.native.reads.set(0);

    let roster = answered(&harness, AgentId::Claude).unwrap();

    assert_eq!(roster.profiles().len(), 2);
    assert_eq!(
        harness.homes().logins.lock().unwrap().len(),
        1,
        "b's login waits in its home"
    );
    assert_eq!(harness.native.deadline_reads.get(), 0);
    assert!(harness.homes().deadline_reads.lock().unwrap().is_empty());
    assert_eq!(
        harness.native.reads.get(),
        1,
        "the read the saved poll needs"
    );
}

#[test]
fn a_roster_reread_opens_the_vault_only_with_a_key_this_session_unlocked() {
    let home = tempfile::tempdir().unwrap();
    let vault = home.path().join(".on-n-off/accounts/vault.enc");
    std::fs::create_dir_all(vault.parent().unwrap()).unwrap();
    std::fs::write(&vault, b"sealed elsewhere").unwrap();

    assert_eq!(Roster::read(home.path(), AgentId::Claude), None);
}

#[test]
fn unlocking_the_vault_tells_limits_every_roster_changed() {
    let harness = Harness::new();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    harness.heard();

    harness.accounts().unlock().unwrap();

    assert_eq!(
        harness.heard(),
        [
            (Heard::Roster(AgentId::Claude), true),
            (Heard::Roster(AgentId::Codex), true),
            (Heard::Roster(AgentId::Muse), true),
        ]
    );
}

#[test]
fn the_saved_refresh_lists_the_saved_profiles_when_the_signed_in_login_cannot_be_identified() {
    let harness = Harness::new();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    let mut unreadable = claude("a", "a2");
    unreadable.account = serde_json::json!({});
    harness.signed_in(Some(unreadable));

    let roster = answered(&harness, AgentId::Claude);

    assert_eq!(
        roster.map(|roster| roster
            .profiles()
            .iter()
            .map(|p| p.email().unwrap_or_default().to_string())
            .collect::<Vec<_>>()),
        Some(vec![
            "a@example.com".to_string(),
            "b@example.com".to_string()
        ]),
        "the saved profiles stay listed even though no saved poll could run"
    );
}

#[test]
fn the_saved_refresh_dates_a_vault_login_by_its_recorded_end_or_the_ends_the_account_list_hands_over(
) {
    let harness = Harness::new();
    let dated = |user: &str, ends_at_ms: i64| {
        let mut login = claude(user, "1");
        login.auth["claudeAiOauth"]["refreshTokenExpiresAt"] = ends_at_ms.into();
        login
    };
    let recorded = identity(AgentId::Claude, "recorded", "team");
    let unrecorded = identity(AgentId::Claude, "unrecorded", "team");
    harness.saved(recorded.clone(), dated("recorded", 4_070_908_800_000));
    harness.saved(unrecorded.clone(), dated("unrecorded", 4_102_444_800_000));
    harness.seed(|db| {
        db.profiles[1].sign_in_end = super::super::login_state::SignInEnd::Unread;
    });
    harness.signed_in(Some(claude("a", "a2")));

    let roster = answered(&harness, AgentId::Claude).unwrap();
    let listed = harness.accounts().list(AgentId::Claude).unwrap();
    let in_column = |ends: &crate::accounts::SignInDeadlines| {
        [&recorded, &unrecorded].map(|identity| {
            let profile = roster.profile(&identity.observation_key()).unwrap();
            serde_json::to_value(profile.in_column(ends, None, 0)).unwrap()["signInEndsAt"].clone()
        })
    };

    assert_eq!(
        in_column(&crate::accounts::SignInDeadlines::new()),
        [
            serde_json::json!("2099-01-01T00:00:00Z"),
            serde_json::Value::Null
        ],
        "a profile saved before its end was recorded: the Limits read never opens its login to date it"
    );
    assert_eq!(
        in_column(&listed.sign_in_deadlines()),
        [
            serde_json::json!("2099-01-01T00:00:00Z"),
            serde_json::json!("2100-01-01T00:00:00Z")
        ]
    );
}
