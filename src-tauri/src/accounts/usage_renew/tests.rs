use super::*;
use crate::{
    accounts::{model, store::Database},
    dto::AgentId,
};
use serde_json::json;
use std::cell::Cell;

fn setup(home: &Path, owned: bool) -> Profile {
    let store = open(home);
    let mut db = Database::default();
    let login = Login {
        auth: json!({"claudeAiOauth":{"accessToken":"old","refreshToken":"original","expiresAt":1}}),
        account: json!({"accountUuid":"user","organizationUuid":"team"}),
    };
    db.save(
        model::identity(AgentId::Claude, &login.auth, &login.account).unwrap(),
        login,
        None,
    )
    .unwrap();
    db.profiles[0].usage_renewal_owned = owned;
    store.persist(&db).unwrap();
    db.profiles[0].clone()
}
fn open(home: &Path) -> Store {
    Store::open_with_key(home, true, |_, _| Ok([7; 32])).unwrap()
}
fn rotated(login: &Login) -> Login {
    let mut login = login.clone();
    login.auth["claudeAiOauth"]["accessToken"] = json!("new-access");
    login.auth["claudeAiOauth"]["refreshToken"] = json!("new-refresh");
    login
}
#[test]
fn owned_renewal_persists_rotated_login_encrypted_without_native_writes() {
    let home = tempfile::tempdir().unwrap();
    let profile = setup(home.path(), true);
    let login = renew_owned(home.path(), &profile, &|| Ok(open(home.path())), &|login| {
        Ok(rotated(login))
    })
    .unwrap();
    assert_eq!(login.auth["claudeAiOauth"]["refreshToken"], "new-refresh");
    let store = open(home.path());
    assert_eq!(
        store.load().unwrap().profiles[0]
            .login
            .as_ref()
            .unwrap()
            .auth,
        login.auth
    );
    let bytes = std::fs::read(store.root.join("vault.enc")).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("new-refresh"));
    assert!(!home.path().join(".claude").exists());
    assert!(!home.path().join(".codex").exists());
}
#[test]
fn native_shadows_never_redeem_a_refresh_token() {
    let home = tempfile::tempdir().unwrap();
    let profile = setup(home.path(), false);
    let calls = Cell::new(0);
    assert!(
        renew_owned(home.path(), &profile, &|| Ok(open(home.path())), &|login| {
            calls.set(calls.get() + 1);
            Ok(rotated(login))
        })
        .is_err()
    );
    assert_eq!(calls.get(), 0);
}
#[test]
fn interrupted_renewal_never_redeems_the_same_generation_again() {
    let home = tempfile::tempdir().unwrap();
    let profile = setup(home.path(), true);
    let calls = Cell::new(0);
    for _ in 0..2 {
        assert!(
            renew_owned(home.path(), &profile, &|| Ok(open(home.path())), &|_| {
                calls.set(calls.get() + 1);
                Err("connection lost".into())
            })
            .is_err()
        );
    }
    assert_eq!(calls.get(), 1);
}
#[test]
fn removing_profile_during_renewal_does_not_resurrect_it() {
    let home = tempfile::tempdir().unwrap();
    let profile = setup(home.path(), true);
    let result = renew_owned(home.path(), &profile, &|| Ok(open(home.path())), &|login| {
        let store = open(home.path());
        let mut db = store.load().unwrap();
        db.profiles.clear();
        store.persist(&db).unwrap();
        Ok(rotated(login))
    });
    assert!(result.is_err());
    assert!(open(home.path()).load().unwrap().profiles.is_empty());
}

#[test]
fn a_completed_reply_recovers_after_publication_failure_without_another_grant() {
    let home = tempfile::tempdir().unwrap();
    let p = setup(home.path(), true);
    let calls = Cell::new(0);
    let opens = Cell::new(0);
    assert!(renew_owned(
        home.path(),
        &p,
        &|| {
            opens.set(opens.get() + 1);
            if opens.get() == 2 {
                Err("vault busy".into())
            } else {
                Ok(open(home.path()))
            }
        },
        &|login| {
            calls.set(calls.get() + 1);
            Ok(rotated(login))
        }
    )
    .is_err());
    assert!(activation_ready(&open(home.path()), &p).is_err());
    let result = renew_owned(home.path(), &p, &|| Ok(open(home.path())), &|login| {
        calls.set(calls.get() + 1);
        Ok(rotated(login))
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(result.auth["claudeAiOauth"]["refreshToken"], "new-refresh");
    let store = open(home.path());
    let current = store.load().unwrap().profiles.remove(0);
    assert!(activation_ready(&store, &current).is_ok());
}
#[test]
fn changed_ownership_during_renewal_never_overwrites_the_native_shadow() {
    let home = tempfile::tempdir().unwrap();
    let p = setup(home.path(), true);
    assert!(
        renew_owned(home.path(), &p, &|| Ok(open(home.path())), &|login| {
            let store = open(home.path());
            let mut db = store.load().unwrap();
            db.profiles[0].usage_renewal_owned = false;
            store.persist(&db).unwrap();
            Ok(rotated(login))
        })
        .is_err()
    );
    assert_eq!(
        open(home.path()).load().unwrap().profiles[0]
            .login
            .as_ref()
            .unwrap()
            .auth["claudeAiOauth"]["refreshToken"],
        "original"
    );
}
#[test]
fn private_claude_grant_preserves_scope_and_saves_rotated_credentials() {
    let home = tempfile::tempdir().unwrap();
    let p = setup(home.path(), true);
    let (url, server) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        r#"{"access_token":"fresh","refresh_token":"rotated","expires_in":28800,"scope":"user:profile user:inference"}"#,
    );
    let result = renew_owned(home.path(), &p, &|| Ok(open(home.path())), &|l| {
        request_at(AgentId::Claude, l, 1000, &url, "unused")
    })
    .unwrap();
    let request = server.join().unwrap();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["grant_type"], "refresh_token");
    assert_eq!(body["refresh_token"], "original");
    assert!(body["scope"].as_str().unwrap().contains("user:profile"));
    assert_eq!(result.auth["claudeAiOauth"]["expiresAt"], 28801000);
    assert_eq!(
        open(home.path()).load().unwrap().profiles[0]
            .login
            .as_ref()
            .unwrap()
            .auth["claudeAiOauth"]["refreshToken"],
        "rotated"
    );
}
#[test]
fn private_codex_grant_retains_identity_when_reply_omits_id_token() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let home = tempfile::tempdir().unwrap();
    let store = open(home.path());
    let mut db = Database::default();
    let claims = json!({"https://api.openai.com/auth":{"chatgpt_user_id":"user","chatgpt_account_id":"team"}});
    let jwt = format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(claims.to_string()));
    let login = Login {
        auth: json!({"tokens":{"access_token":"old","refresh_token":"original","id_token":jwt,"account_id":"team"}}),
        account: Value::Null,
    };
    db.save(
        model::identity(AgentId::Codex, &login.auth, &login.account).unwrap(),
        login,
        None,
    )
    .unwrap();
    db.profiles[0].usage_renewal_owned = true;
    store.persist(&db).unwrap();
    let p = db.profiles.remove(0);
    drop(store);
    let (url, server) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        r#"{"access_token":"fresh","refresh_token":"rotated"}"#,
    );
    let result = renew_owned(home.path(), &p, &|| Ok(open(home.path())), &|l| {
        request_at(AgentId::Codex, l, 1000, "unused", &url)
    })
    .unwrap();
    let request = server.join().unwrap();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["refresh_token"], "original");
    assert_eq!(body["client_id"], "app_EMoamEEZ73f0CkXaXp7hrann");
    assert_eq!(result.auth["tokens"]["refresh_token"], "rotated");
    assert_eq!(
        result.auth["tokens"]["id_token"],
        p.login.unwrap().auth["tokens"]["id_token"]
    );
    assert!(!home.path().join(".codex").exists());
}
