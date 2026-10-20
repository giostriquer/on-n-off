use super::super::model::SignInDeadline;
use super::fixture::{claude, codex, identity, Harness, Heard};
use crate::dto::AgentId;

#[test]
fn listing_shows_this_providers_profiles_and_which_one_the_cli_uses() {
    let harness = Harness::new();
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.saved(identity(AgentId::Codex, "c", "team"), codex("c", "c1"));
    harness.seed(|db| db.profiles[1].login = None);
    harness.signed_in(Some(claude("a", "a2")));
    let sealed = harness.sealed();

    let listed = harness.accounts().list(AgentId::Claude).unwrap();

    let shown: Vec<_> = listed
        .profiles
        .iter()
        .map(|p| (p.id.as_str(), p.active, p.needs_login))
        .collect();
    assert_eq!(
        shown,
        [(a.as_str(), true, false), (b.as_str(), false, true)]
    );
    assert_eq!(listed.profiles[0].email.as_deref(), Some("a@example.com"));
    assert_eq!(
        listed.native_account,
        Some(identity(AgentId::Claude, "a", "team"))
    );
    assert!(!listed.recovery_required);
    assert_eq!(harness.sealed(), sealed, "listing never writes the vault");
    assert_eq!(
        harness.heard(),
        [(
            Heard::Listed(AgentId::Claude, crate::accounts::SignInDeadlines::new()),
            true
        )],
        "Limits is handed what the list read, once, with every lease released"
    );
}

#[test]
fn listing_a_device_without_a_vault_neither_creates_nor_unlocks_one() {
    let harness = Harness::new();
    harness.signed_in(Some(claude("a", "a1")));

    let listed = harness.accounts().list(AgentId::Claude).unwrap();

    assert!(listed.profiles.is_empty());
    assert_eq!(
        listed.native_account,
        Some(identity(AgentId::Claude, "a", "team"))
    );
    assert_eq!(harness.sealed(), None);
}

#[test]
fn a_pending_recovery_is_reported_only_to_the_provider_it_belongs_to() {
    let harness = Harness::new();
    let target = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.interrupted(&target, Some(claude("a", "a1")));

    assert!(
        harness
            .accounts()
            .list(AgentId::Claude)
            .unwrap()
            .recovery_required
    );
    assert!(
        !harness
            .accounts()
            .list(AgentId::Codex)
            .unwrap()
            .recovery_required
    );
}

#[test]
fn each_provider_names_the_history_its_saved_accounts_replace() {
    let key = |provider| identity(provider, "you@example.com", "team").legacy_key();

    assert_eq!(key(AgentId::Claude).as_deref(), Some("you@example.com"));
    assert_eq!(key(AgentId::Codex).as_deref(), Some("team"));
    assert_eq!(key(AgentId::Muse).as_deref(), Some("email:you@example.com"));
    assert_eq!(key(AgentId::Antigravity), None);
    assert_eq!(key(AgentId::Cursor), None);
}

#[test]
fn listing_names_the_history_each_profile_replaces() {
    let harness = Harness::new();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    harness.saved(identity(AgentId::Codex, "c", "team"), codex("c", "c1"));

    let wire = |provider| {
        let listed = harness.accounts().list(provider).unwrap();
        let profile = &serde_json::to_value(&listed).unwrap()["profiles"][0];
        serde_json::json!({
            "identity": profile["identity"],
            "observationId": profile["observationId"],
            "legacyId": profile["legacyId"],
        })
    };
    let written = serde_json::json!({
        "claude": wire(AgentId::Claude),
        "codex": wire(AgentId::Codex),
    });

    assert_eq!(written["claude"]["legacyId"], "a");
    assert_eq!(written["codex"]["legacyId"], "team");
    assert_eq!(
        written,
        serde_json::from_str::<serde_json::Value>(include_str!("../fixtures/listed-profiles.json"))
            .unwrap(),
        "src-tauri/src/accounts/fixtures/listed-profiles.json must read:\n{}",
        serde_json::to_string_pretty(&written).unwrap()
    );
}

#[test]
fn listing_says_which_profiles_are_archived() {
    let harness = Harness::new();
    let a = harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b_identity = identity(AgentId::Claude, "b", "team");
    let b = harness.saved(b_identity.clone(), claude("b", "b1"));
    crate::limits::set_archived_at(
        harness.path(),
        AgentId::Claude,
        &[b_identity.observation_key()],
        true,
    )
    .unwrap();

    let listed = harness.accounts().list(AgentId::Claude).unwrap();

    let shown: Vec<_> = listed
        .profiles
        .iter()
        .map(|p| (p.id.as_str(), p.archived))
        .collect();
    assert_eq!(shown, [(a.as_str(), false), (b.as_str(), true)]);
    let json = serde_json::to_value(&listed).unwrap();
    assert!(json["profiles"][0].get("archived").is_none(), "{json}");
    assert_eq!(json["profiles"][1]["archived"], true);
}

fn dated(mut login: super::super::store::Login, ends_at_ms: i64) -> super::super::store::Login {
    login.auth["claudeAiOauth"]["refreshTokenExpiresAt"] = ends_at_ms.into();
    login
}

fn deadline(user: &str, ends_at_ms: i64) -> Option<SignInDeadline> {
    Some(SignInDeadline {
        identity: identity(AgentId::Claude, user, "team"),
        ends_at_ms,
    })
}

fn homed(harness: &Harness, profile: usize, login: super::super::store::Login) {
    let id = uuid::Uuid::new_v4().to_string();
    harness.seed(|db| {
        db.profiles[profile].login = None;
        db.profiles[profile].home = Some(id.clone());
        db.profiles[profile].sign_in_end = super::super::login_state::SignInEnd::Unread;
    });
    let dir = super::super::homes::dir(harness.path(), &id).unwrap();
    harness.homes().logins.lock().unwrap().insert(dir, login);
}

fn ends(harness: &Harness) -> Vec<(bool, Option<String>)> {
    let listed = harness.accounts().list(AgentId::Claude).unwrap();
    listed
        .profiles
        .into_iter()
        .map(|p| (p.active, p.sign_in_ends_at))
        .collect()
}

#[test]
fn listing_says_when_each_saved_claude_sign_in_ends() {
    let harness = Harness::new().with_homes();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.saved(identity(AgentId::Claude, "c", "team"), claude("c", "c1"));
    harness.signed_in(Some(claude("a", "a2")));
    *harness.native.deadline.borrow_mut() = deadline("a", 1_792_081_900_000);
    homed(&harness, 1, dated(claude("b", "b2"), 1_794_110_392_000));
    homed(&harness, 2, claude("c", "c2"));

    assert_eq!(
        ends(&harness),
        [
            (true, Some("2026-10-15T16:31:40Z".into())),
            (false, Some("2026-11-08T03:59:52Z".into())),
            (false, None),
        ]
    );
    let json = serde_json::to_value(harness.accounts().list(AgentId::Claude).unwrap()).unwrap();
    assert_eq!(json["profiles"][0]["signInEndsAt"], "2026-10-15T16:31:40Z");
    assert!(json["profiles"][2].get("signInEndsAt").is_none(), "{json}");
}

#[test]
fn a_sign_in_claude_code_already_ended_still_says_when() {
    let harness = Harness::new();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    harness.signed_in(None);
    *harness.native.deadline.borrow_mut() = deadline("a", 1_792_081_900_000);

    assert_eq!(
        ends(&harness),
        [(false, Some("2026-10-15T16:31:40Z".into()))]
    );
}

#[test]
fn a_stored_deadline_belongs_only_to_the_account_it_signs_in_as() {
    let harness = Harness::new().with_homes();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.signed_in(Some(claude("x", "x1")));
    *harness.native.deadline.borrow_mut() = deadline("x", 1_792_081_900_000);
    homed(&harness, 1, dated(claude("a", "a2"), 1_794_110_392_000));

    assert_eq!(ends(&harness), [(false, None), (false, None)]);
}

#[test]
fn the_signed_in_store_outranks_an_older_sign_in_left_in_the_accounts_home() {
    let harness = Harness::new().with_homes();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    homed(&harness, 0, dated(claude("a", "a0"), 1_792_081_900_000));
    harness.signed_in(Some(claude("a", "a2")));
    *harness.native.deadline.borrow_mut() = deadline("a", 1_794_313_502_000);

    assert_eq!(
        ends(&harness),
        [(true, Some("2026-11-10T12:25:02Z".into()))]
    );
}

#[test]
fn a_sign_in_awaiting_use_shows_its_own_end_over_the_one_the_default_store_left() {
    let harness = Harness::new().with_homes();
    harness.saved(
        identity(AgentId::Claude, "a", "team"),
        dated(claude("a", "a2"), 1_794_313_502_000),
    );
    harness.seed(|db| db.profiles[0].pending_activation = true);
    harness.signed_in(None);
    *harness.native.deadline.borrow_mut() = deadline("a", 1_792_081_900_000);
    let renewed = [(false, Some("2026-11-10T12:25:02Z".into()))];

    assert_eq!(ends(&harness), renewed, "while the sign-in is in the vault");
    homed(&harness, 0, dated(claude("a", "a2"), 1_794_313_502_000));
    assert_eq!(ends(&harness), renewed, "once it has moved into its home");
}

#[test]
fn a_sign_in_awaiting_use_never_borrows_the_end_the_default_store_left() {
    let harness = Harness::new();
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a2"));
    harness.seed(|db| db.profiles[0].pending_activation = true);
    harness.signed_in(None);
    *harness.native.deadline.borrow_mut() = deadline("a", 1_792_081_900_000);

    assert_eq!(ends(&harness), [(false, None)]);
}
