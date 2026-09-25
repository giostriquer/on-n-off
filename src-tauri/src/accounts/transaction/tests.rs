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
            workspace_id: login.auth["workspace"].as_str().unwrap_or("team").into(),
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
    activate(&mut db, &native, &b, false, &mut |db| {
        durable = serde_json::to_string(db).unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(native.read().unwrap().unwrap().auth["user"], "b");
    let mut reloaded: Database = serde_json::from_str(&durable).unwrap();
    activate(&mut reloaded, &native, &a, false, &mut |_| Ok(())).unwrap();
    assert_eq!(
        native.read().unwrap().unwrap().auth["refresh"],
        "cli-rotated-a"
    );
    assert!(reloaded.recovery.is_none());
}
#[test]
fn backup_failure_never_changes_native_login() {
    let (mut db, native, _, b) = fixture(false);
    assert!(activate(
        &mut db,
        &native,
        &b,
        false,
        &mut |_| Err("disk full".into())
    )
    .is_err());
    assert_eq!(
        native.read().unwrap().unwrap().auth["refresh"],
        "cli-rotated-a"
    );
}
#[test]
fn failed_readback_restores_the_newest_outgoing_login() {
    let (mut db, native, _, b) = fixture(true);
    assert!(activate(&mut db, &native, &b, false, &mut |_| Ok(())).is_err());
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
    assert!(activate(&mut db, &native, &b, false, &mut |_| Ok(())).is_err());
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
    activate(&mut db, &native, &a, false, &mut |_| Ok(())).unwrap();
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
    activate(&mut db, &RefreshBeforeLock(native), &b, false, &mut |_| {
        Ok(())
    })
    .unwrap();
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
    activate(&mut db, &native, &b, false, &mut |_| Ok(())).unwrap();
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
    activate(&mut db, &native, &b, false, &mut |db| {
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

fn in_workspace(user: &str, refresh: &str, workspace: &str) -> Login {
    Login {
        auth: json!({"user":user,"refresh":refresh,"workspace":workspace}),
        account: Value::Null,
    }
}
/// A running client: `during` runs when on-n-off persists its journal, `after_write` replaces the
/// new login right after on-n-off publishes it, and `in_verify` writes while verification runs.
#[derive(Default)]
struct Client {
    during: Option<Option<Login>>,
    after_write: Option<Option<Login>>,
    in_verify: Option<Option<Login>>,
    renewing: bool,
    fail_verify: bool,
}
struct Running {
    store: Store,
    client: Client,
    locked: std::rc::Rc<std::cell::Cell<bool>>,
    wrote: std::cell::Cell<bool>,
    readback_locked: std::cell::Cell<Option<bool>>,
    verified: std::cell::Cell<usize>,
    verified_locked: std::cell::Cell<bool>,
}
struct Guard(std::rc::Rc<std::cell::Cell<bool>>);
impl NativeGuard for Guard {}
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
impl Native for Running {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        self.locked.set(true);
        Ok(Box::new(Guard(self.locked.clone())))
    }
    fn read(&self) -> Result<Option<Login>, String> {
        if self.wrote.take() {
            self.readback_locked.set(Some(self.locked.get()));
        }
        self.store.read()
    }
    fn identify(&self, login: &Login) -> Result<Identity, String> {
        self.store.identify(login)
    }
    fn write(&self, login: Option<&Login>) -> Result<(), String> {
        self.store.write(login)?;
        if login.is_some_and(|l| l.auth["user"] == "b") {
            self.wrote.set(true);
            if let Some(then) = &self.client.after_write {
                *self.store.live.borrow_mut() = then.clone();
            }
        }
        Ok(())
    }
    fn verify(&self) -> Result<(), String> {
        self.verified.set(self.verified.get() + 1);
        if self.locked.get() {
            self.verified_locked.set(true);
        }
        if let Some(then) = &self.client.in_verify {
            *self.store.live.borrow_mut() = then.clone();
        }
        if self.client.fail_verify {
            Err("verification refused".into())
        } else {
            Ok(())
        }
    }
    fn renews_soon(&self, _login: &Login) -> bool {
        self.client.renewing
    }
}
/// Profile a (the signed-in login, in workspace one) and profile b (in workspace two).
fn running(client: Client) -> (Database, Running, String, String) {
    let native = Store {
        live: RefCell::new(Some(in_workspace("a", "cli-rotated-a", "one"))),
        fail_verify: false,
    };
    let mut db = Database::default();
    let a_login = in_workspace("a", "old", "one");
    let a = db
        .save(native.identify(&a_login).unwrap(), a_login, None)
        .unwrap();
    let b_login = in_workspace("b", "b1", "two");
    let b = db
        .save(native.identify(&b_login).unwrap(), b_login, None)
        .unwrap();
    let native = Running {
        store: native,
        client,
        locked: Default::default(),
        wrote: Default::default(),
        readback_locked: Default::default(),
        verified: Default::default(),
        verified_locked: Default::default(),
    };
    (db, native, a, b)
}
fn switch(
    db: &mut Database,
    native: &Running,
    id: &str,
    alongside: bool,
) -> (Result<(), String>, Vec<Database>) {
    let mut snapshots = Vec::new();
    let result = activate(db, native, id, alongside, &mut |db| {
        let snapshot: Database = serde_json::from_str(&serde_json::to_string(db).unwrap()).unwrap();
        if snapshot.recovery.is_some()
            && native
                .store
                .live
                .borrow()
                .as_ref()
                .is_some_and(|l| l.auth["user"] == "a")
        {
            if let Some(then) = &native.client.during {
                *native.store.live.borrow_mut() = then.clone();
            }
        }
        snapshots.push(snapshot);
        Ok(())
    });
    (result, snapshots)
}
fn live_refresh(native: &Running) -> String {
    native
        .store
        .live
        .borrow()
        .as_ref()
        .map_or("none".into(), |l| {
            l.auth["refresh"].as_str().unwrap().into()
        })
}

#[test]
fn recaptures_and_journals_an_outgoing_login_a_client_rotated_during_the_switch() {
    let rotated = Some(in_workspace("a", "client-rotated-a", "one"));
    let (mut db, native, a, b) = running(Client {
        during: Some(rotated.clone()),
        ..Default::default()
    });
    let (result, snapshots) = switch(&mut db, &native, &b, false);
    result.unwrap();
    assert_eq!(
        native.store.live.borrow().as_ref().unwrap().auth["user"],
        "b"
    );
    let saved = db.profiles.iter().find(|p| p.id == a).unwrap();
    assert_eq!(
        saved.login.as_ref().unwrap().auth["refresh"],
        "client-rotated-a"
    );
    let journaled = snapshots
        .iter()
        .rev()
        .find_map(|s| s.recovery.as_ref())
        .unwrap();
    assert_eq!(
        journaled.outgoing.as_ref().unwrap().auth["refresh"],
        "client-rotated-a"
    );

    let (mut db, native, _, b) = running(Client {
        during: Some(rotated),
        fail_verify: true,
        ..Default::default()
    });
    assert!(switch(&mut db, &native, &b, false).0.is_err());
    assert_eq!(
        live_refresh(&native),
        "client-rotated-a",
        "recovery restores the rotated generation"
    );
}

#[test]
fn never_publishes_over_a_login_that_changed_account_and_clears_its_journal() {
    let (mut db, native, _, b) = running(Client {
        during: Some(Some(in_workspace("c", "external-c", "three"))),
        ..Default::default()
    });
    let (result, snapshots) = switch(&mut db, &native, &b, false);
    let error = result.unwrap_err();
    assert!(error.ends_with("Nothing was replaced."), "{error}");
    assert_eq!(
        native.store.live.borrow().as_ref().unwrap().auth["user"],
        "c"
    );
    assert!(db.recovery.is_none());
    assert!(snapshots.last().unwrap().recovery.is_none());
}

#[test]
fn reads_the_published_login_back_under_the_native_locks() {
    let (mut db, native, _, b) = running(Client::default());
    switch(&mut db, &native, &b, false).0.unwrap();
    assert_eq!(native.readback_locked.get(), Some(true));
}

/// Verification renews an expired Claude login, and the renewal takes the native locks itself: run
/// under them, it would find them busy and fail the account change, while Claude Code waited on
/// them for as long as verification took. So neither activation nor recovery verifies while it
/// holds them.
#[test]
fn verification_never_runs_under_the_native_locks() {
    let (mut db, native, _, b) = running(Client::default());
    switch(&mut db, &native, &b, false).0.unwrap();
    assert_eq!(native.verified.get(), 1);
    assert!(
        !native.verified_locked.get(),
        "activation verified under the locks"
    );

    let (mut db, native, _, b) = running(Client {
        after_write: Some(Some(in_workspace("a", "client-refreshed-a", "one"))),
        ..Default::default()
    });
    assert!(switch(&mut db, &native, &b, false).0.is_err());
    assert!(db.recovery.is_some());
    recover(&mut db, &native, &mut |_| Ok(())).unwrap();
    assert_eq!(
        native.verified.get(),
        1,
        "recovery verified the login a client rotated"
    );
    assert!(
        !native.verified_locked.get(),
        "recovery verified under the locks"
    );
    assert!(db.recovery.is_none());
}

#[test]
fn keeps_the_journal_without_verifying_when_a_client_overwrites_the_new_login() {
    let (mut db, native, _, b) = running(Client {
        after_write: Some(Some(in_workspace("a", "client-refreshed-a", "one"))),
        ..Default::default()
    });
    let (result, snapshots) = switch(&mut db, &native, &b, false);
    let error = result.unwrap_err();
    assert!(error.contains("pending recovery"), "{error}");
    assert_eq!(native.verified.get(), 0);
    assert!(db.recovery.is_some());
    assert!(snapshots.last().unwrap().recovery.is_some());
    assert_eq!(live_refresh(&native), "client-refreshed-a");
}

#[test]
fn a_client_signing_out_after_the_switch_is_left_signed_out() {
    let (mut db, native, _, b) = running(Client {
        after_write: Some(None),
        ..Default::default()
    });
    let error = switch(&mut db, &native, &b, true).0.unwrap_err();
    assert!(error.contains("pending recovery"), "{error}");
    assert_eq!(live_refresh(&native), "none");
    assert!(db.recovery.is_some());
}

#[test]
fn restores_the_outgoing_login_a_client_wrote_back_without_verifying() {
    let (mut db, native, _, b) = running(Client {
        after_write: Some(Some(in_workspace("a", "cli-rotated-a", "one"))),
        ..Default::default()
    });
    let error = switch(&mut db, &native, &b, true).0.unwrap_err();
    assert!(error.contains("The previous login was restored"), "{error}");
    assert_eq!(native.verified.get(), 0);
    assert!(db.recovery.is_none());
    assert_eq!(live_refresh(&native), "cli-rotated-a");
}

#[test]
fn a_failed_verification_beside_clients_never_verifies_bytes_a_client_wrote() {
    let (mut db, native, _, b) = running(Client {
        in_verify: Some(Some(in_workspace("a", "client-refreshed-a", "one"))),
        fail_verify: true,
        ..Default::default()
    });
    let error = switch(&mut db, &native, &b, true).0.unwrap_err();
    assert!(error.contains("pending recovery"), "{error}");
    assert_eq!(native.verified.get(), 1);
    assert!(db.recovery.is_some());
    assert_eq!(live_refresh(&native), "client-refreshed-a");
}

#[test]
fn switching_beside_clients_refuses_the_same_workspace_or_a_renewing_login() {
    let (mut db, native, _, b) = running(Client {
        renewing: true,
        ..Default::default()
    });
    let error = switch(&mut db, &native, &b, true).0.unwrap_err();
    assert!(error.contains("about to renew"), "{error}");
    assert!(db.recovery.is_none());
    assert_eq!(live_refresh(&native), "cli-rotated-a");
    assert!(
        switch(&mut db, &native, &b, false).0.is_ok(),
        "an ordinary switch has no such gate"
    );

    let (mut db, native, _, _) = running(Client::default());
    let same = in_workspace("c", "c1", "one");
    let c = db
        .save(native.identify(&same).unwrap(), same, None)
        .unwrap();
    let error = switch(&mut db, &native, &c, true).0.unwrap_err();
    assert!(error.contains("same workspace"), "{error}");
    assert_eq!(live_refresh(&native), "cli-rotated-a");
}

#[test]
fn a_failed_verification_beside_clients_restores_bytes_it_owns() {
    let (mut db, native, _, b) = running(Client {
        fail_verify: true,
        ..Default::default()
    });
    let error = switch(&mut db, &native, &b, true).0.unwrap_err();
    assert!(error.contains("The previous login was restored"), "{error}");
    assert_eq!(native.verified.get(), 1);
    assert!(db.recovery.is_none());
    assert_eq!(live_refresh(&native), "cli-rotated-a");
}

#[test]
fn a_switch_from_signed_out_that_a_client_signs_out_again_ends_signed_out() {
    let (mut db, native, _, b) = running(Client {
        after_write: Some(None),
        ..Default::default()
    });
    *native.store.live.borrow_mut() = None;
    let error = switch(&mut db, &native, &b, true).0.unwrap_err();
    assert!(error.contains("The previous login was restored"), "{error}");
    assert!(db.recovery.is_none());
    assert_eq!(live_refresh(&native), "none");
}

#[test]
fn an_unreadable_read_back_is_not_blamed_on_a_client() {
    struct Unreadable(Running);
    impl Native for Unreadable {
        fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
            self.0.lock()
        }
        fn read(&self) -> Result<Option<Login>, String> {
            if self.0.wrote.get() {
                return Err("auth.json is malformed".into());
            }
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
    let (mut db, native, _, b) = running(Client::default());
    let native = Unreadable(native);
    let error = activate(&mut db, &native, &b, false, &mut |_| Ok(())).unwrap_err();
    assert!(
        error.starts_with("The new login could not be read back."),
        "{error}"
    );
    assert_eq!(native.0.verified.get(), 0);
    assert!(db.recovery.is_some());
}

#[test]
fn a_journal_that_cannot_be_cleared_before_publication_says_so() {
    let (mut db, native, _, b) = running(Client {
        during: Some(Some(in_workspace("c", "external-c", "three"))),
        ..Default::default()
    });
    let mut writes = 0;
    let error = activate(&mut db, &native, &b, false, &mut |db| {
        writes += 1;
        if db.recovery.is_some() {
            *native.store.live.borrow_mut() = native.client.during.clone().unwrap();
            return Ok(());
        }
        Err("disk full".into())
    })
    .unwrap_err();
    assert_eq!(writes, 2);
    assert!(error.contains("could not be cleared"), "{error}");
    assert_eq!(
        native.store.live.borrow().as_ref().unwrap().auth["user"],
        "c"
    );
}

#[test]
fn activation_revokes_private_ownership_in_the_durable_journal_before_native_publication() {
    let (mut db, native, _, b) = fixture(false);
    db.profiles
        .iter_mut()
        .find(|p| p.id == b)
        .unwrap()
        .usage_renewal_owned = true;
    let mut journaled = false;
    activate(&mut db, &native, &b, false, &mut |db| {
        if db.recovery.is_some() {
            assert!(
                !db.profiles
                    .iter()
                    .find(|p| p.id == b)
                    .unwrap()
                    .usage_renewal_owned
            );
            assert_eq!(native.read().unwrap().unwrap().auth["user"], "a");
            journaled = true;
        }
        Ok(())
    })
    .unwrap();
    assert!(journaled);
    assert!(
        !db.profiles
            .iter()
            .find(|p| p.id == b)
            .unwrap()
            .usage_renewal_owned
    );
}
