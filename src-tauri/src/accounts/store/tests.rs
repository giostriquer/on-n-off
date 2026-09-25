use super::*;
use crate::dto::AgentId;
use serde_json::json;
fn identity(user: &str) -> Identity {
    Identity {
        provider: AgentId::Codex,
        user_id: user.into(),
        workspace_id: "team".into(),
    }
}
fn login(refresh: &str) -> Login {
    Login {
        auth: json!({"refresh":refresh}),
        account: Value::Null,
    }
}
fn held(root: &std::path::Path) -> FileLease {
    let file = std::fs::File::create(root.join("lock")).unwrap();
    FileLease::acquire(file, std::fs::File::try_lock).unwrap()
}
#[test]
fn adopts_the_latest_native_generation_without_duplicate_profiles() {
    let mut db = Database::default();
    let id = db.save(identity("a"), login("old"), None).unwrap();
    assert_eq!(
        db.save(identity("a"), login("cli-rotated"), None).unwrap(),
        id
    );
    db.save(identity("b"), login("b"), None).unwrap();
    assert_eq!(db.profiles.len(), 2);
    assert_eq!(
        db.profiles[0].login.as_ref().unwrap().auth["refresh"],
        "cli-rotated"
    );
}
#[test]
fn wrong_identity_or_removed_profile_cannot_be_reauthenticated() {
    let mut db = Database::default();
    let id = db.save(identity("a"), login("old"), None).unwrap();
    assert!(db.save(identity("b"), login("wrong"), Some(&id)).is_err());
    assert_eq!(
        db.profiles[0].login.as_ref().unwrap().auth["refresh"],
        "old"
    );
    db.profiles.clear();
    assert!(db.save(identity("a"), login("late"), Some(&id)).is_err());
    assert!(db.profiles.is_empty());
}

#[test]
fn persisted_vault_roundtrip_never_exposes_tokens_in_plaintext() {
    let root = tempfile::tempdir().unwrap();
    let store = Store {
        root: root.path().into(),
        key: [3; 32],
        _lease: held(root.path()),
    };
    let mut db = Database::default();
    db.save(identity("a"), login("never-plaintext"), None)
        .unwrap();
    store.persist(&db).unwrap();
    let bytes = std::fs::read(root.path().join("vault.enc")).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("never-plaintext"));
    assert_eq!(
        store.load().unwrap().profiles[0]
            .login
            .as_ref()
            .unwrap()
            .auth["refresh"],
        "never-plaintext"
    );
    let mut damaged = bytes;
    damaged[30] ^= 1;
    std::fs::write(root.path().join("vault.enc"), &damaged).unwrap();
    assert!(store.load().is_err());
    assert_eq!(
        std::fs::read(root.path().join("vault.enc")).unwrap(),
        damaged
    );
}

#[test]
fn another_manager_logout_invalidates_an_earlier_signin_after_reload() {
    let mut db = Database::default();
    let started = db.login_epoch;
    db.invalidate_logins().unwrap();
    let reloaded: Database = serde_json::from_slice(&serde_json::to_vec(&db).unwrap()).unwrap();
    assert!(reloaded.allow_publication(started).is_err());
}
#[test]
fn pending_recovery_refuses_login_publication_even_at_current_epoch() {
    let db = Database {
        recovery: Some(super::super::transaction::Recovery {
            target_id: "target".into(),
            outgoing: None,
            outgoing_identity: None,
        }),
        ..Database::default()
    };
    assert!(db.allow_publication(db.login_epoch).is_err());
}

#[test]
fn saved_account_name_comes_from_email_even_when_captured_without_a_name() {
    let mut db = Database::default();
    let user = Identity {
        provider: AgentId::Claude,
        user_id: "stable-user".into(),
        workspace_id: "team".into(),
    };
    let native = Login {
        auth: json!({"claudeAiOauth":{"refreshToken":"fixture"}}),
        account: json!({"emailAddress":"person@example.com"}),
    };
    db.save(user, native, None).unwrap();
    assert_eq!(db.profiles[0].label, "person@example.com");
}

#[test]
fn category_survives_credential_updates_and_can_be_cleared() {
    let root = tempfile::tempdir().unwrap();
    let store = Store {
        root: root.path().into(),
        key: [9; 32],
        _lease: held(root.path()),
    };
    let mut db = Database::default();
    let user = Identity {
        provider: AgentId::Claude,
        user_id: "a".into(),
        workspace_id: "team".into(),
    };
    let mut native = Login {
        auth: json!({"claudeAiOauth":{"refreshToken":"a1"}}),
        account: json!({"emailAddress":"a@example.com"}),
    };
    let id = db.save(user.clone(), native.clone(), None).unwrap();
    db.set_category(&id, "  Client A / research  ").unwrap();
    native.auth["claudeAiOauth"]["refreshToken"] = json!("a2");
    native.account["emailAddress"] = json!("updated@example.com");
    db.save(user, native, None).unwrap();
    store.persist(&db).unwrap();
    let mut loaded = store.load().unwrap();
    assert_eq!(
        loaded.profiles[0].email.as_deref(),
        Some("updated@example.com")
    );
    assert_eq!(
        loaded.profiles[0].category.as_deref(),
        Some("Client A / research")
    );
    assert!(loaded.set_category(&id, &"x".repeat(101)).is_err());
    assert_eq!(
        loaded.profiles[0].category.as_deref(),
        Some("Client A / research")
    );
    loaded.set_category(&id, "  ").unwrap();
    store.persist(&loaded).unwrap();
    assert_eq!(store.load().unwrap().profiles[0].category, None);
}

#[test]
fn older_profile_names_load_as_categories_without_becoming_the_email() {
    let root = tempfile::tempdir().unwrap();
    let store = Store {
        root: root.path().into(),
        key: [7; 32],
        _lease: held(root.path()),
    };
    let old = json!({"profiles":[{"id":"old","identity":{"provider":"claude","userId":"a","workspaceId":"team"},"label":"Client A","saved_at":"2026-09-13T00:00:00Z","login":{"auth":{},"account":{"emailAddress":"a@example.com"}}}]});
    let sealed = super::super::vault::seal(&[7; 32], &serde_json::to_vec(&old).unwrap()).unwrap();
    std::fs::write(root.path().join("vault.enc"), sealed).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.profiles[0].email.as_deref(), Some("a@example.com"));
    assert_eq!(loaded.profiles[0].label, "a@example.com");
    assert_eq!(loaded.profiles[0].category.as_deref(), Some("Client A"));
}

/// A login whose ID token carries these `https://api.openai.com/auth` claims.
fn login_with_claims(claims: Value) -> Login {
    Login {
        auth: json!({"tokens": {
            "id_token": crate::accounts::model::tests::id_token(&claims),
            "refresh_token": "never-projected"
        }}),
        account: Value::Null,
    }
}

/// Save one Codex profile with this login into `home`'s vault, which `vault::tests::unlock_fixture`
/// has already unlocked, and give back its observation key.
pub(crate) fn saved_codex_fixture(
    home: &std::path::Path,
    identity: Identity,
    auth: Value,
) -> String {
    let store = Store::open(home, true).unwrap();
    let mut db = store.load().unwrap();
    let key = identity.observation_key();
    db.save(
        identity,
        Login {
            auth,
            account: Value::Null,
        },
        None,
    )
    .unwrap();
    store.persist(&db).unwrap();
    key
}

#[test]
fn codex_claims_come_only_from_the_exact_saved_login_after_vault_reload() {
    let root = tempfile::tempdir().unwrap();
    let store = Store {
        root: root.path().into(),
        key: [9; 32],
        _lease: held(root.path()),
    };
    let mut db = Database::default();
    let saved = identity("inactive");
    let key = saved.observation_key();
    db.save(
        saved,
        login_with_claims(json!({"chatgpt_plan_type": "pro"})),
        None,
    )
    .unwrap();
    store.persist(&db).unwrap();
    let mut loaded = store.load().unwrap();
    let claims = loaded.codex_claims(&key).unwrap();
    assert_eq!(claims["chatgpt_plan_type"], "pro");
    assert!(claims.get("refresh_token").is_none() && claims.get("id_token").is_none());
    assert!(loaded
        .codex_claims(&identity("other-user").observation_key())
        .is_none());
    let mut other_workspace = identity("inactive");
    other_workspace.workspace_id = "other".into();
    assert!(loaded
        .codex_claims(&other_workspace.observation_key())
        .is_none());
    loaded.profiles[0].login = None;
    assert!(
        loaded.codex_claims(&key).is_none(),
        "a profile awaiting sign-in has no token to read"
    );
    loaded.profiles.clear();
    store.persist(&loaded).unwrap();
    assert!(store.load().unwrap().codex_claims(&key).is_none());
}

#[test]
fn concurrent_save_waits_for_vault_read_then_preserves_both_profiles() {
    use std::sync::mpsc;
    use std::time::Duration;
    let home = tempfile::tempdir().unwrap();
    let (root, lease) = Store::lease(home.path()).unwrap();
    let store = Store {
        root,
        key: [7; 32],
        _lease: lease,
    };
    let mut db = Database::default();
    db.save(identity("first"), login("first-fixture"), None)
        .unwrap();
    store.persist(&db).unwrap();
    let home_path = home.path().to_owned();
    let (started, waiting) = mpsc::channel();
    let (finished, result) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started.send(()).unwrap();
        let saved = (|| -> Result<(), String> {
            let (root, lease) = Store::lease(&home_path)?;
            let store = Store {
                root,
                key: [7; 32],
                _lease: lease,
            };
            let mut db = store.load()?;
            db.save(identity("second"), login("second-fixture"), None)?;
            store.persist(&db)
        })();
        finished.send(saved).unwrap();
    });
    waiting.recv().unwrap();
    let while_locked = result.recv_timeout(Duration::from_millis(100));
    drop(store);
    worker.join().unwrap();
    assert!(
        matches!(while_locked, Err(mpsc::RecvTimeoutError::Timeout)),
        "a save must wait for an ordinary vault read instead of failing: {while_locked:?}"
    );
    result
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let (root, lease) = Store::lease(home.path()).unwrap();
    let store = Store {
        root,
        key: [7; 32],
        _lease: lease,
    };
    let saved = store.load().unwrap();
    assert_eq!(saved.profiles.len(), 2);
    assert_eq!(saved.profiles[0].identity, identity("first"));
    assert_eq!(saved.profiles[1].identity, identity("second"));
}

#[test]
fn busy_vault_times_out_without_replacing_the_lock_or_data() {
    use std::time::Duration;
    let home = tempfile::tempdir().unwrap();
    let (root, held) = Store::lease(home.path()).unwrap();
    std::fs::write(root.join("vault.enc"), b"existing protected bytes").unwrap();
    let error = Store::lease_with_timeout(home.path(), Duration::from_millis(40)).unwrap_err();
    assert_eq!(
        error,
        "Another account operation is running. Retry when it finishes."
    );
    assert_eq!(
        std::fs::read(root.join("vault.enc")).unwrap(),
        b"existing protected bytes"
    );
    assert!(Store::lease_with_timeout(home.path(), Duration::ZERO).is_err());
    drop(held);
    assert!(Store::lease(home.path()).is_ok());
}

#[test]
fn unlocking_an_existing_vault_does_not_hold_the_shared_account_lease() {
    let home = tempfile::tempdir().unwrap();
    let (root, lease) = Store::lease(home.path()).unwrap();
    std::fs::write(root.join("vault.enc"), b"existing fixture").unwrap();
    drop(lease);
    let opened = Store::open_with_key(home.path(), false, |_, create| {
        assert!(!create);
        assert!(
            Store::lease_with_timeout(home.path(), std::time::Duration::ZERO).is_ok(),
            "an OS unlock prompt must not block Codex's account storage lease"
        );
        Ok([7; 32])
    })
    .unwrap();
    assert!(Store::lease_with_timeout(home.path(), std::time::Duration::ZERO).is_err());
    drop(opened);
}

#[cfg(unix)]
#[test]
fn a_finished_operation_frees_the_vault_lease_while_a_spawned_child_still_shares_it() {
    // A child that another thread spawns meanwhile inherits the descriptor until it execs; the
    // next operation must not wait on it.
    let home = tempfile::tempdir().unwrap();
    let (_, lease) = Store::lease(home.path()).unwrap();
    let inherited = lease.duplicate().unwrap();
    assert!(Store::lease_with_timeout(home.path(), std::time::Duration::ZERO).is_err());
    drop(lease);
    assert!(Store::lease_with_timeout(home.path(), std::time::Duration::ZERO).is_ok());
    drop(inherited);
}

#[test]
fn a_lease_timeout_override_lasts_only_as_long_as_its_guard() {
    use std::time::Duration;
    assert_eq!(lease_timeout(), Duration::from_secs(10));
    {
        let _short = lease_timeout_override::set(Duration::from_millis(50));
        assert_eq!(lease_timeout(), Duration::from_millis(50));
        let busy = std::thread::spawn(lease_timeout).join().unwrap();
        assert_eq!(
            busy,
            Duration::from_secs(10),
            "other threads keep the production wait"
        );
    }
    assert_eq!(lease_timeout(), Duration::from_secs(10));
}

#[test]
fn a_busy_vault_lease_gives_up_after_the_overridden_wait() {
    use std::time::{Duration, Instant};
    let home = tempfile::tempdir().unwrap();
    let (_, holder) = Store::lease(home.path()).unwrap();
    let _short = lease_timeout_override::set(Duration::from_millis(20));
    let started = Instant::now();
    assert_eq!(
        Store::lease(home.path()).unwrap_err(),
        "Another account operation is running. Retry when it finishes."
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "Store::lease must wait the overridden timeout, not the production one"
    );
    drop(holder);
}

#[test]
fn capturing_a_native_login_revokes_private_renewal_ownership() {
    let mut db = Database::default();
    let id = db.save(identity("a"), login("old"), None).unwrap();
    db.profiles[0].usage_renewal_owned = true;
    db.save(identity("a"), login("native"), Some(&id)).unwrap();
    assert!(!db.profiles[0].usage_renewal_owned);
}

#[test]
fn old_vaults_do_not_assume_renewal_ownership() {
    let mut db = Database::default();
    db.save(identity("a"), login("old"), None).unwrap();
    let mut encoded = serde_json::to_value(&db).unwrap();
    encoded["profiles"][0]
        .as_object_mut()
        .unwrap()
        .remove("usage_renewal_owned");
    let restored: Database = serde_json::from_value(encoded).unwrap();
    assert!(!restored.profiles[0].usage_renewal_owned);
}

/// The vault is JSON sealed in `vault.enc`, and every earlier vault must keep loading: the names
/// the database writes are its on-disk format.
#[test]
fn a_sealed_database_keeps_the_vault_format() {
    let root = tempfile::tempdir().unwrap();
    let store = Store {
        root: root.path().into(),
        key: [5; 32],
        _lease: held(root.path()),
    };
    let mut db = Database::default();
    let id = db.save(identity("a"), login("fixture-a"), None).unwrap();
    db.invalidate_logins().unwrap();
    db.ignored_accounts.push(identity("removed"));
    db.ignored_credentials.push("signed-out-fingerprint".into());
    db.recovery = Some(super::super::transaction::Recovery {
        target_id: id.clone(),
        outgoing: Some(login("outgoing")),
        outgoing_identity: Some(identity("b")),
    });
    store.persist(&db).unwrap();

    let sealed = std::fs::read(root.path().join("vault.enc")).unwrap();
    let plain = super::super::vault::unseal(&[5; 32], &sealed).unwrap();
    let mut written: Value = serde_json::from_slice(&plain).unwrap();
    written["profiles"][0]["saved_at"] = json!("<saved at>");
    assert_eq!(
        written,
        json!({
            "profiles": [{
                "id": id,
                "identity": {"provider": "codex", "userId": "a", "workspaceId": "team"},
                "label": "Email unavailable",
                "email": null,
                "category": null,
                "saved_at": "<saved at>",
                "login": {"auth": {"refresh": "fixture-a"}, "account": null},
                "pending_activation": false,
                "usage_renewal_owned": false
            }],
            "login_epoch": 1,
            "ignored_accounts": [{"provider": "codex", "userId": "removed", "workspaceId": "team"}],
            "ignored_credentials": ["signed-out-fingerprint"],
            "recovery": {
                "target_id": id,
                "outgoing": {"auth": {"refresh": "outgoing"}, "account": null},
                "outgoing_identity": {"provider": "codex", "userId": "b", "workspaceId": "team"}
            }
        })
    );
}

/// A vault written before epochs, exclusions, recovery and renewal ownership existed loads with
/// none of them: no pending recovery, nothing excluded, nothing awaiting activation or renewing.
#[test]
fn a_vault_from_before_the_later_fields_loads_without_them() {
    let root = tempfile::tempdir().unwrap();
    let store = Store {
        root: root.path().into(),
        key: [4; 32],
        _lease: held(root.path()),
    };
    let old = json!({"profiles": [{
        "id": "old",
        "identity": {"provider": "codex", "userId": "a", "workspaceId": "team"},
        "label": "Codex account",
        "saved_at": "2026-09-01T00:00:00Z",
        "login": {"auth": {"refresh": "old-fixture"}, "account": null}
    }]});
    let sealed = super::super::vault::seal(&[4; 32], &serde_json::to_vec(&old).unwrap()).unwrap();
    std::fs::write(root.path().join("vault.enc"), sealed).unwrap();

    let loaded = store.load().unwrap();
    assert_eq!(loaded.login_epoch, 0);
    assert!(loaded.recovery.is_none());
    assert!(loaded.ignored_accounts.is_empty() && loaded.ignored_credentials.is_empty());
    let profile = &loaded.profiles[0];
    assert_eq!(profile.identity, identity("a"));
    assert!(!profile.pending_activation && !profile.usage_renewal_owned);
    assert_eq!(
        profile.login.as_ref().unwrap().auth["refresh"],
        "old-fixture"
    );
    assert_eq!(profile.label, "Email unavailable");
    assert_eq!(profile.category, None, "a default name is not a category");
}
