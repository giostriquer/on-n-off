use super::*;
use crate::dto::AgentId;
use serde_json::{json, Value};
use std::cell::RefCell;
fn login(user: &str, refresh: &str) -> Login {
    Login {
        auth: json!({"user":user,"refresh":refresh}),
        account: Value::Null,
    }
}
struct Store {
    live: RefCell<Option<Login>>,
    fail_verify: bool,
}
impl Native for Store {
    fn read(&self) -> Result<Option<Login>, String> {
        Ok(self.live.borrow().clone())
    }
    fn identify(&self, login: &Login) -> Result<Identity, String> {
        Ok(Identity {
            provider: AgentId::Codex,
            user_id: login.auth["user"].as_str().unwrap().into(),
            workspace_id: "team".into(),
        })
    }
    fn write(&self, login: Option<&Login>) -> Result<(), String> {
        *self.live.borrow_mut() = login.cloned();
        Ok(())
    }
    fn verify(&self) -> Result<(), String> {
        if self.fail_verify {
            Err("verification refused".into())
        } else {
            Ok(())
        }
    }
}
fn fixture(fail_verify: bool) -> (Database, Store, String, String) {
    let native = Store {
        live: RefCell::new(Some(login("a", "cli-rotated-a"))),
        fail_verify,
    };
    let mut db = Database::default();
    let a = db
        .save(
            native.identify(&login("a", "old")).unwrap(),
            login("a", "old"),
            None,
        )
        .unwrap();
    let b = db
        .save(
            native.identify(&login("b", "b1")).unwrap(),
            login("b", "b1"),
            None,
        )
        .unwrap();
    (db, native, a, b)
}
#[test]
fn switch_back_uses_the_outgoing_cli_rotated_generation() {
    let (mut db, native, a, b) = fixture(false);
    let mut durable = String::new();
    activate(&mut db, &native, &b, &mut |db| {
        durable = serde_json::to_string(db).unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(native.read().unwrap().unwrap().auth["user"], "b");
    let mut reloaded: Database = serde_json::from_str(&durable).unwrap();
    activate(&mut reloaded, &native, &a, &mut |_| Ok(())).unwrap();
    assert_eq!(
        native.read().unwrap().unwrap().auth["refresh"],
        "cli-rotated-a"
    );
    assert!(reloaded.recovery.is_none());
}
#[test]
fn backup_failure_never_changes_native_login() {
    let (mut db, native, _, b) = fixture(false);
    assert!(activate(&mut db, &native, &b, &mut |_| Err("disk full".into())).is_err());
    assert_eq!(
        native.read().unwrap().unwrap().auth["refresh"],
        "cli-rotated-a"
    );
}
#[test]
fn failed_readback_restores_the_newest_outgoing_login() {
    let (mut db, native, _, b) = fixture(true);
    assert!(activate(&mut db, &native, &b, &mut |_| Ok(())).is_err());
    assert_eq!(
        native.read().unwrap().unwrap().auth["refresh"],
        "cli-rotated-a"
    );
}

#[test]
fn repairs_a_partial_identity_write_without_saving_outgoing_tokens_as_the_target() {
    struct Partial(RefCell<Option<Login>>, std::cell::Cell<bool>);
    impl Native for Partial {
        fn read(&self) -> Result<Option<Login>, String> {
            Ok(self.0.borrow().clone())
        }
        fn identify(&self, l: &Login) -> Result<Identity, String> {
            Ok(Identity {
                provider: AgentId::Claude,
                user_id: l.account["user"].as_str().unwrap().into(),
                workspace_id: "team".into(),
            })
        }
        fn write(&self, l: Option<&Login>) -> Result<(), String> {
            let l = l.unwrap();
            if !self.1.replace(true) {
                self.0.borrow_mut().as_mut().unwrap().account = l.account.clone();
                Err("credential write refused".into())
            } else {
                *self.0.borrow_mut() = Some(l.clone());
                Ok(())
            }
        }
        fn verify(&self) -> Result<(), String> {
            Ok(())
        }
    }
    let make = |user: &str| Login {
        auth: json!({"refresh":format!("token-{user}")}),
        account: json!({"user":user}),
    };
    let native = Partial(RefCell::new(Some(make("a"))), std::cell::Cell::new(false));
    let mut db = Database::default();
    db.save(native.identify(&make("a")).unwrap(), make("a"), None)
        .unwrap();
    let b = db
        .save(native.identify(&make("b")).unwrap(), make("b"), None)
        .unwrap();
    assert!(activate(&mut db, &native, &b, &mut |_| Ok(())).is_err());
    assert_eq!(native.read().unwrap().unwrap().account["user"], "a");
    assert_eq!(
        db.profiles
            .iter()
            .find(|p| p.id == b)
            .unwrap()
            .login
            .as_ref()
            .unwrap()
            .auth["refresh"],
        "token-b"
    );
}

#[test]
fn reauthenticated_current_profile_can_activate_its_new_login() {
    let (mut db, native, a, _) = fixture(false);
    let profile = db.profiles.iter_mut().find(|p| p.id == a).unwrap();
    profile.login = Some(login("a", "new-sign-in-a"));
    profile.pending_activation = true;
    activate(&mut db, &native, &a, &mut |_| Ok(())).unwrap();
    assert_eq!(
        native.read().unwrap().unwrap().auth["refresh"],
        "new-sign-in-a"
    );
}

#[test]
fn capture_waits_for_native_lock_and_preserves_the_completed_refresh() {
    struct RefreshBeforeLock(Store);
    impl Native for RefreshBeforeLock {
        fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
            *self.0.live.borrow_mut() = Some(login("a", "a2-rotated-before-lock"));
            Ok(Box::new(()))
        }
        fn read(&self) -> Result<Option<Login>, String> {
            self.0.read()
        }
        fn identify(&self, login: &Login) -> Result<Identity, String> {
            self.0.identify(login)
        }
        fn write(&self, login: Option<&Login>) -> Result<(), String> {
            self.0.write(login)
        }
        fn verify(&self) -> Result<(), String> {
            self.0.verify()
        }
    }
    let (mut db, native, a, b) = fixture(false);
    activate(&mut db, &RefreshBeforeLock(native), &b, &mut |_| Ok(())).unwrap();
    assert_eq!(
        db.profiles
            .iter()
            .find(|p| p.id == a)
            .unwrap()
            .login
            .as_ref()
            .unwrap()
            .auth["refresh"],
        "a2-rotated-before-lock"
    );
}
#[test]
fn switching_away_preserves_a_pending_reauthentication() {
    let (mut db, native, a, b) = fixture(false);
    let profile = db.profiles.iter_mut().find(|p| p.id == a).unwrap();
    profile.login = Some(login("a", "fresh-isolated-login"));
    profile.pending_activation = true;
    activate(&mut db, &native, &b, &mut |_| Ok(())).unwrap();
    let profile = db.profiles.iter().find(|p| p.id == a).unwrap();
    assert_eq!(
        profile.login.as_ref().unwrap().auth["refresh"],
        "fresh-isolated-login"
    );
    assert!(profile.pending_activation);
}

#[test]
fn switching_away_from_a_removed_account_keeps_only_its_recovery_backup() {
    let (mut db, native, a, b) = fixture(false);
    let identity = native.identify(&native.read().unwrap().unwrap()).unwrap();
    db.profiles.retain(|p| p.id != a);
    db.ignored_accounts.push(identity.clone());
    let mut saw_protected_outgoing = false;
    activate(&mut db, &native, &b, &mut |db| {
        if let Some(journal) = &db.recovery {
            saw_protected_outgoing = journal.outgoing.is_some();
        }
        Ok(())
    })
    .unwrap();
    assert!(saw_protected_outgoing);
    assert!(db.profiles.iter().all(|p| p.identity != identity));
    assert!(db.ignored_accounts.contains(&identity));
}
