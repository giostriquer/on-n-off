use super::*;
use crate::dto::AgentId;
use serde_json::json;
use std::cell::RefCell;

/// A scratch home with an empty vault under a fixture key.
fn vault() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    open(home.path())
        .change(ChangeKind::Metadata, |_| Ok(()))
        .unwrap();
    home
}
fn open(home: &Path) -> Store {
    Store::open_with_key(home, true, |_, _| Ok([3; 32])).unwrap()
}
fn loaded(home: &Path) -> Database {
    open(home).load().unwrap()
}
fn sealed(home: &Path) -> Vec<u8> {
    std::fs::read(home.join(".on-n-off/accounts/vault.enc")).unwrap()
}
fn sign_in_ticket(home: &Path) -> Ticket {
    loaded(home).ticket(Guard::SignIn).unwrap()
}
fn interrupted() -> super::super::transaction::Recovery {
    super::super::transaction::Recovery {
        target_id: "target".into(),
        outgoing: None,
        outgoing_identity: None,
    }
}
struct Client {
    live: RefCell<Option<Login>>,
    refused: bool,
    change_during_verify: bool,
}
impl Native for Client {
    fn read(&self) -> Result<Option<Login>, String> {
        Ok(self.live.borrow().clone())
    }
    fn identify(&self, login: &Login) -> Result<Identity, String> {
        Ok(Identity {
            provider: AgentId::Claude,
            user_id: login.account["accountUuid"].as_str().unwrap().into(),
            workspace_id: "team".into(),
        })
    }
    fn write(&self, _: Option<&Login>) -> Result<(), String> {
        panic!("Discovery must never write native credentials")
    }
    fn verify(&self) -> Result<(), String> {
        if self.refused {
            return Err("unverified".into());
        }
        if self.change_during_verify {
            *self.live.borrow_mut() = Some(login("other"));
        }
        Ok(())
    }
}
fn login(user: &str) -> Login {
    Login {
        auth: json!({"claudeAiOauth":{"accessToken":format!("token-{user}"),"refreshToken":"fixture"}}),
        account: json!({"accountUuid":user,"organizationUuid":"team","emailAddress":format!("{user}@example.com")}),
    }
}
fn client() -> Client {
    Client {
        live: RefCell::new(Some(login("a"))),
        refused: false,
        change_during_verify: false,
    }
}
#[test]
fn a_failed_identity_verification_never_becomes_a_saved_candidate() {
    let mut native = client();
    native.refused = true;
    assert!(candidate(&native, &Database::default()).is_err());
}
#[test]
fn a_natural_switch_during_verification_is_not_saved_under_the_previous_account() {
    let mut native = client();
    native.change_during_verify = true;
    assert!(candidate(&native, &Database::default()).is_err());
}
#[test]
fn disabling_remembering_or_a_newer_operation_prevents_late_publication() {
    for (enabled, changed) in [(false, false), (true, true)] {
        let native = client();
        let home = vault();
        let ticket = sign_in_ticket(home.path());
        if changed {
            open(home.path())
                .change(ChangeKind::Account, |_| Ok(()))
                .unwrap();
        }
        let before = sealed(home.path());
        let result = publish(open(home.path()), &ticket, &native, login("a"), enabled);
        assert!(result.is_err() || result == Ok(false));
        assert!(loaded(home.path()).profiles.is_empty());
        assert_eq!(sealed(home.path()), before, "late publication");
    }
}
#[test]
fn removed_accounts_and_signed_out_credential_generations_stay_excluded() {
    let native = client();
    let mut db = Database::default();
    db.ignored_accounts
        .push(native.identify(&login("a")).unwrap());
    assert!(candidate(&native, &db).unwrap().is_none());
    db.ignored_accounts.clear();
    db.ignored_credentials.push(login("a").fingerprint());
    assert!(candidate(&native, &db).unwrap().is_none());
}
#[test]
fn native_changes_before_publication_do_not_save_the_old_candidate() {
    let native = client();
    *native.live.borrow_mut() = Some(login("b"));
    let home = vault();
    let ticket = sign_in_ticket(home.path());
    let before = sealed(home.path());
    assert!(publish(open(home.path()), &ticket, &native, login("a"), true).is_err());
    assert!(
        loaded(home.path()).profiles.is_empty(),
        "wrong account saved"
    );
    assert_eq!(sealed(home.path()), before, "nothing persisted");
}
#[test]
fn natural_accounts_are_saved_once_and_pending_reauthentication_is_preserved() {
    let native = client();
    let home = vault();
    let db = loaded(home.path());
    let ticket = db.ticket(Guard::SignIn).unwrap();
    let found = candidate(&native, &db).unwrap().unwrap();
    assert!(publish(open(home.path()), &ticket, &native, found, true).unwrap());
    let mut loaded = loaded(home.path());
    assert_eq!(loaded.profiles[0].email.as_deref(), Some("a@example.com"));
    assert!(candidate(&native, &loaded).unwrap().is_none());
    loaded.profiles[0].pending_activation = true;
    loaded.profiles[0].login = Some(login("replacement"));
    assert!(candidate(&native, &loaded).unwrap().is_none());
}

#[test]
fn remembering_is_off_by_default_and_does_not_create_a_vault() {
    let root = tempfile::tempdir().unwrap();
    assert!(!enabled(root.path()).unwrap());
    assert!(!poll_home(root.path(), AgentId::Claude).unwrap());
    assert!(!poll_home(root.path(), AgentId::Codex).unwrap());
    assert!(!root.path().join(".on-n-off").exists());
}

#[test]
fn opting_out_never_needs_to_decrypt_a_damaged_vault() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(".on-n-off/accounts");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("remembering.json"), r#"{"enabled":true}"#).unwrap();
    std::fs::write(root.join("vault.enc"), b"damaged vault").unwrap();
    set_enabled_in(home.path(), false, &|_| {
        panic!("Opting out must not unlock the vault")
    })
    .unwrap();
    assert!(!enabled(home.path()).unwrap());
    assert_eq!(
        std::fs::read(root.join("vault.enc")).unwrap(),
        b"damaged vault"
    );
}
#[test]
fn metadata_changes_cannot_reenroll_a_signed_out_credential_generation() {
    let native = client();
    let mut db = Database::default();
    db.ignored_credentials.push(login("a").fingerprint());
    native.live.borrow_mut().as_mut().unwrap().account["emailAddress"] =
        json!("updated@example.com");
    assert!(candidate(&native, &db).unwrap().is_none());
}
#[test]
fn a_pending_recovery_prevents_late_publication_even_at_the_same_epoch() {
    let native = client();
    let home = vault();
    let ticket = sign_in_ticket(home.path());
    open(home.path())
        .change(ChangeKind::Metadata, |db| {
            db.begin_recovery(interrupted());
            Ok(())
        })
        .unwrap();
    let before = sealed(home.path());
    assert!(publish(open(home.path()), &ticket, &native, login("a"), true).is_err());
    assert!(loaded(home.path()).profiles.is_empty());
    assert_eq!(sealed(home.path()), before, "published during recovery");
}

/// Turning remembering on rejects every check made before it, so one from before an opt-out
/// cannot publish after the opt-in. It changes no login, so a pending recovery does not refuse it.
#[test]
fn turning_remembering_on_rejects_earlier_checks_and_is_allowed_during_recovery() {
    let native = client();
    let home = vault();
    let ticket = sign_in_ticket(home.path());
    set_enabled_in(home.path(), true, &|_| Ok(open(home.path()))).unwrap();
    assert!(enabled(home.path()).unwrap());
    assert!(publish(open(home.path()), &ticket, &native, login("a"), true).is_err());

    let home = vault();
    open(home.path())
        .change(ChangeKind::Metadata, |db| {
            db.begin_recovery(interrupted());
            Ok(())
        })
        .unwrap();
    set_enabled_in(home.path(), true, &|_| Ok(open(home.path()))).unwrap();
    assert!(enabled(home.path()).unwrap());
    assert!(loaded(home.path()).recovery().is_some());
}
