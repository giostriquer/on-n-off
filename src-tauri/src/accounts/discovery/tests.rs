use super::*;
use crate::dto::AgentId;
use serde_json::json;
use std::cell::RefCell;
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
    for (enabled, epoch) in [(false, 0), (true, 1)] {
        let native = client();
        let mut db = Database {
            login_epoch: epoch,
            ..Database::default()
        };
        let result = publish(&mut db, &native, login("a"), 0, enabled, &mut |_| {
            panic!("late publication")
        });
        assert!(result.is_err() || result == Ok(false));
        assert!(db.profiles.is_empty());
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
    let mut db = Database::default();
    assert!(
        publish(&mut db, &native, login("a"), 0, true, &mut |_| panic!(
            "wrong account saved"
        ))
        .is_err()
    );
    assert!(db.profiles.is_empty());
}
#[test]
fn natural_accounts_are_saved_once_and_pending_reauthentication_is_preserved() {
    let native = client();
    let mut db = Database::default();
    let mut persisted = String::new();
    let found = candidate(&native, &db).unwrap().unwrap();
    assert!(publish(&mut db, &native, found, 0, true, &mut |db| {
        persisted = serde_json::to_string(db).unwrap();
        Ok(())
    })
    .unwrap());
    let mut loaded: Database = serde_json::from_str(&persisted).unwrap();
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
