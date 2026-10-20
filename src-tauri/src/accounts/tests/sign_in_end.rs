use super::super::{login_state::SignInEnd, model::Identity, store::Login, Activation};
use super::fixture::{claude, identity, Harness};
use crate::dto::AgentId;
use serde_json::{json, Value};

const AHEAD_MS: i64 = 4_102_444_800_000;
const ENDING: &str = "2100-01-01T00:00:00Z";

fn dated(user: &str, generation: &str) -> Login {
    let mut login = claude(user, generation);
    login.auth["claudeAiOauth"]["refreshTokenExpiresAt"] = AHEAD_MS.into();
    login
}

fn you() -> Identity {
    identity(AgentId::Claude, "you", "team")
}

fn recorded(harness: &Harness, id: &str) -> Value {
    let profile = harness
        .vault()
        .profiles
        .into_iter()
        .find(|p| p.id == id)
        .unwrap();
    serde_json::to_value(&profile).unwrap()["sign_in_end"].clone()
}

fn homed(harness: &Harness, id: &str, login: Login) {
    let home = uuid::Uuid::new_v4().to_string();
    harness.seed(|db| {
        let profile = db.profiles.iter_mut().find(|p| p.id == id).unwrap();
        profile.login = None;
        profile.home = Some(home.clone());
    });
    let dir = super::super::homes::dir(harness.path(), &home).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    harness.homes().logins.lock().unwrap().insert(dir, login);
}

fn unread(harness: &Harness, id: &str) {
    harness.seed(|db| {
        let profile = db.profiles.iter_mut().find(|p| p.id == id).unwrap();
        profile.sign_in_end = SignInEnd::Unread;
    });
}

fn login_state(harness: &Harness) -> Value {
    let listed = serde_json::to_value(harness.accounts().list(AgentId::Claude).unwrap()).unwrap();
    listed["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["identity"]["userId"] == "you")
        .unwrap()["loginState"]
        .clone()
}

fn read_saved_accounts(harness: &Harness) {
    let mut entries = Vec::new();
    harness
        .accounts()
        .refresh_usage(AgentId::Claude, false, &mut entries, &|_, _| {
            panic!("a Claude login is read in its home")
        });
}

#[test]
fn a_login_moved_into_its_home_records_when_its_sign_in_ends() {
    let harness = Harness::new().with_homes();
    let you_id = harness.saved(you(), dated("you", "y1"));
    harness.signed_in(Some(claude("me", "m1")));

    read_saved_accounts(&harness);

    assert!(harness.home_of(&you_id).is_some(), "moved into its home");
    assert_eq!(recorded(&harness, &you_id), json!({"at": AHEAD_MS}));
}

#[test]
fn switching_to_an_account_in_its_home_records_when_its_sign_in_ends() {
    let harness = Harness::new().with_homes();
    let you_id = harness.saved(you(), claude("you", "y0"));
    harness.signed_in(Some(claude("me", "m1")));
    homed(&harness, &you_id, dated("you", "y1"));
    unread(&harness, &you_id);

    harness
        .accounts()
        .activate(AgentId::Claude, &you_id, Activation::Ordinary)
        .unwrap();

    assert_eq!(recorded(&harness, &you_id), json!({"at": AHEAD_MS}));
}

#[test]
fn a_listing_reads_no_home_whose_sign_in_end_is_recorded() {
    let harness = Harness::new().with_homes();
    let you_id = harness.saved(you(), dated("you", "y1"));
    harness.signed_in(Some(claude("me", "m1")));
    read_saved_accounts(&harness);
    *harness.homes().sealed.lock().unwrap() = true;

    assert_eq!(
        login_state(&harness),
        json!({"state": "ending", "endsAt": ENDING})
    );
    assert!(harness.home_of(&you_id).is_some());
}

#[test]
fn a_home_with_no_recorded_end_is_read_once_and_remembered() {
    let harness = Harness::new().with_homes();
    let you_id = harness.saved(you(), claude("you", "y0"));
    harness.signed_in(Some(claude("me", "m1")));
    homed(&harness, &you_id, dated("you", "y1"));
    unread(&harness, &you_id);
    let reads = || *harness.homes().secrets_read.lock().unwrap();

    let before = reads();
    let first = login_state(&harness);
    let second = login_state(&harness);

    assert_eq!(first, json!({"state": "ending", "endsAt": ENDING}));
    assert_eq!(second, first);
    assert_eq!(reads() - before, 1, "one lazy read, then the recorded date");
    assert_eq!(recorded(&harness, &you_id), json!({"at": AHEAD_MS}));
}

#[test]
fn a_home_whose_one_read_fails_records_no_end_and_is_not_read_again() {
    let harness = Harness::new().with_homes();
    let you_id = harness.saved(you(), claude("you", "y0"));
    harness.signed_in(Some(claude("me", "m1")));
    homed(&harness, &you_id, dated("you", "y1"));
    unread(&harness, &you_id);
    let dir = harness.home_of(&you_id).unwrap();
    harness.homes().unreadable.lock().unwrap().push(dir);
    let reads = || *harness.homes().secrets_read.lock().unwrap();

    let before = reads();
    let first = login_state(&harness);
    let second = login_state(&harness);

    assert_eq!(first, json!({"state": "usable"}));
    assert_eq!(second, first);
    assert_eq!(reads() - before, 1, "the failed read is not made again");
    assert_eq!(recorded(&harness, &you_id), json!("unknown"));
}
