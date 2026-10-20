use super::super::{homes, store::Login, usage::FetchResult, Activation};
use super::fixture::{claude, codex, identity, muse, Harness};
use crate::dto::AgentId;
use std::path::{Path, PathBuf};

const CLAUDE_LAYOUT: &str = ".claude";
const MUSE_LAYOUT: &str = "config/muse";

fn read(harness: &Harness, provider: AgentId) {
    let mut entries = Vec::new();
    harness
        .accounts()
        .refresh_usage(provider, true, &mut entries, &|profile, _| FetchResult {
            login: profile.login.clone(),
            result: Err(crate::limits::ReadFailure::Unavailable("not read".into())),
        });
}

fn account_home(harness: &Harness, id: &str) -> PathBuf {
    homes::dir(harness.path(), id).unwrap()
}

fn unnamed_home(harness: &Harness, layout: &[&str], login: Login) -> PathBuf {
    let dir = account_home(harness, &uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    for part in layout {
        std::fs::create_dir_all(dir.join(part)).unwrap();
    }
    harness
        .homes()
        .logins
        .lock()
        .unwrap()
        .insert(dir.clone(), login);
    dir
}

fn marked(dir: &Path, provider: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("provider.json"), format!("\"{provider}\"")).unwrap();
}

fn recorded_in(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("provider.json")).ok()
}

fn sorted(mut homes: Vec<(PathBuf, AgentId)>) -> Vec<(PathBuf, AgentId)> {
    homes.sort_by(|a, b| a.0.cmp(&b.0));
    homes
}

fn vault(harness: &Harness) {
    harness.saved(identity(AgentId::Codex, "c", "team"), codex("c", "c1"));
    assert!(super::super::store::Store::vault_exists(harness.path()));
}

fn claude_accounts(harness: &Harness) -> String {
    harness.saved(identity(AgentId::Claude, "a", "team"), claude("a", "a1"));
    let b = harness.saved(identity(AgentId::Claude, "b", "team"), claude("b", "b1"));
    harness.signed_in(Some(claude("a", "a2")));
    b
}

fn named_home(harness: &Harness, profile: usize) -> PathBuf {
    let id = uuid::Uuid::new_v4().to_string();
    harness.seed(|db| db.profiles[profile].home = Some(id.clone()));
    account_home(harness, &id)
}

#[test]
fn a_muse_read_never_tears_down_a_claude_home() {
    let harness = Harness::new().with_homes();
    vault(&harness);
    let laid_out = unnamed_home(&harness, &[CLAUDE_LAYOUT], claude("a", "a1"));
    let recorded = unnamed_home(&harness, &[], claude("b", "b1"));
    marked(&recorded, "claude");

    read(&harness, AgentId::Muse);

    assert!(
        laid_out.exists() && recorded.exists(),
        "a Muse read removed a Claude home"
    );
    assert!(harness.homes().deleted_by.lock().unwrap().is_empty());
    read(&harness, AgentId::Claude);
    assert_eq!(
        sorted(harness.homes().deleted_by.lock().unwrap().clone()),
        sorted(vec![
            (laid_out.clone(), AgentId::Claude),
            (recorded.clone(), AgentId::Claude)
        ]),
        "torn down through Claude's own home"
    );
    assert!(!laid_out.exists() && !recorded.exists());
}

#[test]
fn a_claude_read_never_tears_down_a_muse_home_and_muse_takes_back_the_one_it_forgot() {
    let harness = Harness::new().with_homes();
    let account = identity(AgentId::Muse, "you@example.com", "");
    let id = harness.saved(account, muse("you@example.com", "m1"));
    harness.seed(|db| db.profiles[0].login = None);
    let forgotten = unnamed_home(&harness, &[MUSE_LAYOUT], muse("you@example.com", "m1"));
    let recorded = unnamed_home(&harness, &[], muse("other@example.com", "o1"));
    marked(&recorded, "muse");

    read(&harness, AgentId::Claude);

    assert!(
        forgotten.exists() && recorded.exists(),
        "a Claude read removed a Muse home"
    );
    assert!(
        !forgotten.join(".claude").exists(),
        "a Claude read laid its config dir into a Muse home"
    );
    assert!(harness.homes().deleted_by.lock().unwrap().is_empty());
    read(&harness, AgentId::Muse);
    assert_eq!(harness.home_of(&id), Some(forgotten.clone()));
    assert_eq!(recorded_in(&forgotten).as_deref(), Some("\"muse\""));
    assert_eq!(
        *harness.homes().deleted_by.lock().unwrap(),
        [(recorded, AgentId::Muse)]
    );
}

#[test]
fn a_home_no_provider_can_claim_is_left_and_its_login_never_read() {
    let harness = Harness::new().with_homes();
    vault(&harness);
    let bare = unnamed_home(&harness, &[], claude("a", "a1"));
    let both = unnamed_home(&harness, &[CLAUDE_LAYOUT, MUSE_LAYOUT], claude("a", "a1"));
    let unknown = unnamed_home(&harness, &[CLAUDE_LAYOUT], claude("a", "a1"));
    marked(&unknown, "acme");
    *harness.homes().sealed.lock().unwrap() = true;

    for provider in [AgentId::Claude, AgentId::Muse] {
        read(&harness, provider);
    }

    for dir in [&bare, &both, &unknown] {
        assert!(dir.exists(), "{} was removed", dir.display());
    }
    assert!(harness.homes().deleted_by.lock().unwrap().is_empty());
}

#[test]
fn a_login_moves_into_a_home_only_once_the_home_names_its_provider() {
    let harness = Harness::new().with_homes();
    let b = claude_accounts(&harness);
    let dir = named_home(&harness, 1);
    harness.homes().refuses.lock().unwrap().push(dir.clone());

    read(&harness, AgentId::Claude);

    assert_eq!(
        recorded_in(&dir).as_deref(),
        Some("\"claude\""),
        "the home did not name its provider before the login was put"
    );
    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    harness.homes().refuses.lock().unwrap().clear();
    read(&harness, AgentId::Claude);
    assert_eq!(harness.in_home(&b), Some("b1".into()));
    assert_eq!(harness.in_vault(&b), None);
}

fn foreign_home(harness: &Harness) -> PathBuf {
    let dir = named_home(harness, 1);
    marked(&dir, "muse");
    harness
        .homes()
        .logins
        .lock()
        .unwrap()
        .insert(dir.clone(), claude("b", "b0"));
    dir
}

#[test]
fn a_home_another_provider_owns_is_never_moved_into_and_a_switch_uses_the_vault_login() {
    let harness = Harness::new().with_homes();
    let b = claude_accounts(&harness);
    let dir = foreign_home(&harness);

    read(&harness, AgentId::Claude);

    assert_eq!(harness.in_vault(&b), Some("b1".into()));
    assert_eq!(
        harness.in_home(&b),
        Some("b0".into()),
        "moved a login into Muse's home"
    );
    harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap();
    assert_eq!(harness.live(), Some("b1".into()));
    assert_eq!(harness.in_home(&b), Some("b0".into()));
    assert!(harness.homes().emptied.lock().unwrap().is_empty());
    assert_eq!(recorded_in(&dir).as_deref(), Some("\"muse\""));
}

#[test]
fn a_switch_never_takes_a_login_out_of_a_home_another_provider_owns() {
    let harness = Harness::new().with_homes();
    let b = claude_accounts(&harness);
    let dir = foreign_home(&harness);
    harness.seed(|db| db.profiles[1].login = None);

    let error = harness
        .accounts()
        .activate(AgentId::Claude, &b, Activation::Ordinary)
        .unwrap_err();

    assert!(error.contains("another provider"), "{error}");
    assert_eq!(harness.live(), Some("a2".into()));
    assert_eq!(harness.in_home(&b), Some("b0".into()));
    assert_eq!(harness.in_vault(&b), None);
    assert!(harness.homes().emptied.lock().unwrap().is_empty());
    assert_eq!(recorded_in(&dir).as_deref(), Some("\"muse\""));
}
