use super::*;
use crate::{
    accounts::store::{ChangeKind, Database},
    dto::AgentId,
};
use serde_json::{json, Value};
use std::cell::Cell;

fn setup(home: &Path, owned: bool) -> Profile {
    let login = Login {
        auth: json!({"claudeAiOauth":{"accessToken":"old","refreshToken":"original","expiresAt":1}}),
        account: json!({"accountUuid":"user","organizationUuid":"team"}),
    };
    saved(home, AgentId::Claude, login, owned)
}
/// Save `login` as the vault's one profile, owning its renewal or not, and give it back.
fn saved(home: &Path, provider: AgentId, login: Login, owned: bool) -> Profile {
    open(home)
        .change(ChangeKind::Metadata, |db| {
            let identity = super::super::view(provider, &login)?.identity()?;
            db.save(identity, login, None)?;
            db.profiles[0].usage_renewal_owned = owned;
            Ok(db.profiles[0].clone())
        })
        .unwrap()
}
/// `provider`'s private renewal of `login` at 1,000 ms, its grant sent to `token_url`.
fn renew_at(provider: AgentId, login: &Login, token_url: &str) -> Result<Login, String> {
    super::super::adapter(provider)?.renew_private(login, 1000, token_url)
}
fn open(home: &Path) -> Store {
    Store::open_with_key(home, true, |_, _| Ok([7; 32])).unwrap()
}
/// Change the vault as another account operation would, meanwhile.
fn meanwhile(home: &Path, kind: ChangeKind<'_>, edit: impl FnOnce(&mut Database)) {
    open(home)
        .change(kind, |db| {
            edit(db);
            Ok(())
        })
        .unwrap();
}
/// Whether `profile`'s login may be activated, as `use` asks before it switches.
fn ready(home: &Path, profile: &Profile) -> Result<(), String> {
    Renewals::of(&open(home)).activation_ready(profile)
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
    let login = renew_owned(&profile, &|| Ok(open(home.path())), &|login| {
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
    assert!(renew_owned(&profile, &|| Ok(open(home.path())), &|login| {
        calls.set(calls.get() + 1);
        Ok(rotated(login))
    })
    .is_err());
    assert_eq!(calls.get(), 0);
}
#[test]
fn interrupted_renewal_never_redeems_the_same_generation_again() {
    let home = tempfile::tempdir().unwrap();
    let profile = setup(home.path(), true);
    let calls = Cell::new(0);
    let renew = || {
        renew_owned(&profile, &|| Ok(open(home.path())), &|_| {
            calls.set(calls.get() + 1);
            Err("connection lost".into())
        })
        .err()
    };
    assert_eq!(renew().as_deref(), Some("connection lost"));
    // The journal refuses the retry, not a lease the first attempt left behind.
    assert_eq!(
        renew().as_deref(),
        Some("A prior renewal did not finish. Sign in again to refresh this account.")
    );
    assert_eq!(calls.get(), 1);
}
#[test]
fn a_second_renewal_is_refused_while_a_grant_is_in_flight() {
    let home = tempfile::tempdir().unwrap();
    let profile = setup(home.path(), true);
    let nested = std::cell::RefCell::new(None);
    let login = renew_owned(&profile, &|| Ok(open(home.path())), &|login| {
        *nested.borrow_mut() = renew_owned(&profile, &|| Ok(open(home.path())), &|_| {
            panic!("a second grant must not start while the first is in flight")
        })
        .err();
        Ok(rotated(login))
    })
    .unwrap();
    assert_eq!(
        nested.into_inner().as_deref(),
        Some("Another account renewal is running.")
    );
    assert_eq!(login.auth["claudeAiOauth"]["refreshToken"], "new-refresh");
}
#[cfg(unix)]
#[test]
fn a_finished_renewal_frees_its_lease_while_a_spawned_child_still_shares_it() {
    // A child that another thread spawns during the grant inherits the lease's descriptor and
    // keeps it until it execs; the next renewal must not see the lease as still running.
    let root = tempfile::tempdir().unwrap();
    let lease = acquire_renewal_lease(root.path(), "profile").unwrap();
    let inherited = lease.duplicate().unwrap();
    assert_eq!(
        acquire_renewal_lease(root.path(), "profile")
            .err()
            .as_deref(),
        Some("Another account renewal is running.")
    );
    drop(lease);
    assert!(acquire_renewal_lease(root.path(), "profile").is_ok());
    drop(inherited);
}
#[test]
fn removing_profile_during_renewal_does_not_resurrect_it() {
    let home = tempfile::tempdir().unwrap();
    let profile = setup(home.path(), true);
    let result = renew_owned(&profile, &|| Ok(open(home.path())), &|login| {
        meanwhile(home.path(), ChangeKind::Account, |db| db.profiles.clear());
        Ok(rotated(login))
    });
    assert!(result.is_err());
    assert!(open(home.path()).load().unwrap().profiles.is_empty());
}

/// The renewal has already spent the refresh token when it publishes, so an account change that
/// left this profile alone (a sign-out of another account, say) must not reject the renewed login:
/// losing it would strand the profile. The epoch deliberately does not guard this publication.
#[test]
fn an_unrelated_account_change_during_renewal_still_publishes_the_renewed_login() {
    let home = tempfile::tempdir().unwrap();
    let profile = setup(home.path(), true);
    let login = renew_owned(&profile, &|| Ok(open(home.path())), &|login| {
        meanwhile(home.path(), ChangeKind::Account, |_| {});
        Ok(rotated(login))
    })
    .unwrap();
    assert_eq!(login.auth["claudeAiOauth"]["refreshToken"], "new-refresh");
    assert_eq!(
        open(home.path()).load().unwrap().profiles[0]
            .login
            .as_ref()
            .unwrap()
            .auth["claudeAiOauth"]["refreshToken"],
        "new-refresh"
    );
}
#[test]
fn a_pending_recovery_rejects_the_renewed_login_and_keeps_the_reply_for_later() {
    let home = tempfile::tempdir().unwrap();
    let profile = setup(home.path(), true);
    let result = renew_owned(&profile, &|| Ok(open(home.path())), &|login| {
        meanwhile(home.path(), ChangeKind::Metadata, |db| {
            db.begin_recovery(super::super::transaction::Recovery {
                target_id: profile.id.clone(),
                outgoing: None,
                outgoing_identity: None,
            })
        });
        Ok(rotated(login))
    });
    assert!(result.is_err());
    let saved = open(home.path()).load().unwrap().profiles.remove(0);
    assert_eq!(
        saved.login.as_ref().unwrap().auth["claudeAiOauth"]["refreshToken"],
        "original"
    );
    assert!(
        ready(home.path(), &saved).is_err(),
        "the completed reply stays journaled, so the spent source is never activated"
    );
}

#[test]
fn a_completed_reply_recovers_after_publication_failure_without_another_grant() {
    let home = tempfile::tempdir().unwrap();
    let p = setup(home.path(), true);
    let calls = Cell::new(0);
    let opens = Cell::new(0);
    assert!(renew_owned(
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
    assert!(ready(home.path(), &p).is_err());
    let result = renew_owned(&p, &|| Ok(open(home.path())), &|login| {
        calls.set(calls.get() + 1);
        Ok(rotated(login))
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(result.auth["claudeAiOauth"]["refreshToken"], "new-refresh");
    let current = open(home.path()).load().unwrap().profiles.remove(0);
    assert!(ready(home.path(), &current).is_ok());
}
#[test]
fn changed_ownership_during_renewal_never_overwrites_the_native_shadow() {
    let home = tempfile::tempdir().unwrap();
    let p = setup(home.path(), true);
    assert!(renew_owned(&p, &|| Ok(open(home.path())), &|login| {
        meanwhile(home.path(), ChangeKind::Account, |db| {
            db.profiles[0].usage_renewal_owned = false
        });
        Ok(rotated(login))
    })
    .is_err());
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
    let result = renew_owned(&p, &|| Ok(open(home.path())), &|l| {
        renew_at(AgentId::Claude, l, &url)
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
    let claims = json!({"https://api.openai.com/auth":{"chatgpt_user_id":"user","chatgpt_account_id":"team"}});
    let jwt = format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(claims.to_string()));
    let login = Login {
        auth: json!({"tokens":{"access_token":"old","refresh_token":"original","id_token":jwt,"account_id":"team"}}),
        account: Value::Null,
    };
    let p = saved(home.path(), AgentId::Codex, login, true);
    let (url, server) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        r#"{"access_token":"fresh","refresh_token":"rotated"}"#,
    );
    let result = renew_owned(&p, &|| Ok(open(home.path())), &|l| {
        renew_at(AgentId::Codex, l, &url)
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

/// A private Codex renewal sends Codex's own refresh grant and folds the reply into the login it
/// came from: the tokens the reply carries replace their stored ones, the refresh time is stamped,
/// and every field the reply does not name is left as it was.
#[test]
fn a_private_codex_renewal_sends_codexs_grant_and_keeps_every_field_it_does_not_replace() {
    let login = Login {
        auth: json!({
            "OPENAI_API_KEY": null,
            "tokens": {"access_token": "old", "refresh_token": "original", "id_token": "id-old",
                "account_id": "team", "extra": "kept"},
            "last_refresh": "2026-09-01T00:00:00Z",
            "custom": {"kept": true}
        }),
        account: json!({"note": "kept"}),
    };
    let (url, server) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        r#"{"access_token":"fresh","refresh_token":"rotated","id_token":"id-new","expires_in":3600}"#,
    );

    let renewed = renew_at(AgentId::Codex, &login, &url).unwrap();

    let body: Value = serde_json::from_str(&server.join().unwrap().body).unwrap();
    assert_eq!(
        body,
        json!({"grant_type": "refresh_token", "refresh_token": "original",
            "client_id": "app_EMoamEEZ73f0CkXaXp7hrann"})
    );
    assert_eq!(
        renewed.auth,
        json!({
            "OPENAI_API_KEY": null,
            "tokens": {"access_token": "fresh", "refresh_token": "rotated", "id_token": "id-new",
                "account_id": "team", "extra": "kept"},
            "last_refresh": "1970-01-01T00:00:01+00:00",
            "custom": {"kept": true}
        })
    );
    assert_eq!(renewed.account, login.account);
}

/// A reply that leaves the refresh or ID token blank keeps the stored one; one without an access
/// token renews nothing.
#[test]
fn a_private_codex_renewal_keeps_the_tokens_a_reply_leaves_blank_and_refuses_one_without_access() {
    let login = Login {
        auth: json!({"tokens": {"access_token": "old", "refresh_token": "original", "id_token": "id-old"}}),
        account: Value::Null,
    };
    let (url, server) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        r#"{"access_token":"fresh","refresh_token":" ","id_token":""}"#,
    );
    let renewed = renew_at(AgentId::Codex, &login, &url).unwrap();
    server.join().unwrap();
    assert_eq!(renewed.auth["tokens"]["access_token"], "fresh");
    assert_eq!(renewed.auth["tokens"]["refresh_token"], "original");
    assert_eq!(renewed.auth["tokens"]["id_token"], "id-old");

    let (url, server) =
        crate::http::serve_once_capturing("200 OK", &[], r#"{"refresh_token":"rotated"}"#);
    assert!(renew_at(AgentId::Codex, &login, &url).is_err());
    server.join().unwrap();
}

/// A private Claude renewal redeems for the client and scopes the login was issued, and keeps
/// every field of the stored login the reply does not replace, and the account beside it.
#[test]
fn a_private_claude_renewal_redeems_for_the_logins_own_client_and_keeps_what_it_does_not_replace() {
    let login = Login {
        auth: json!({"claudeAiOauth": {"accessToken": "old", "refreshToken": "original",
            "expiresAt": 1, "scopes": ["user:profile", "user:inference"],
            "clientId": "fixture-client", "subscriptionType": "max",
            "rateLimitTier": "default_claude_max_20x", "extra": "kept"}}),
        account: json!({"accountUuid": "user", "organizationUuid": "team"}),
    };
    let (url, server) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        r#"{"access_token":"fresh","expires_in":100,"refresh_token":"rotated","refresh_token_expires_in":200,"scope":"user:profile"}"#,
    );

    let renewed = renew_at(AgentId::Claude, &login, &url).unwrap();

    let body: Value = serde_json::from_str(&server.join().unwrap().body).unwrap();
    assert_eq!(
        body,
        json!({"grant_type": "refresh_token", "refresh_token": "original",
            "client_id": "fixture-client", "scope": "user:profile user:inference"})
    );
    assert_eq!(
        renewed.auth,
        json!({"claudeAiOauth": {"accessToken": "fresh", "refreshToken": "rotated",
            "expiresAt": 101_000, "refreshTokenExpiresAt": 201_000, "scopes": ["user:profile"],
            "clientId": "fixture-client", "subscriptionType": "max",
            "rateLimitTier": "default_claude_max_20x", "extra": "kept"}})
    );
    assert_eq!(renewed.account, login.account);
}

/// A renewal journal an earlier version left, its reply complete, is adopted without another
/// grant: the journal's names are its on-disk format, and the login in it keeps the vault's shape.
#[test]
fn a_completed_renewal_journal_an_earlier_version_wrote_is_adopted_without_a_grant() {
    let home = tempfile::tempdir().unwrap();
    let profile = setup(home.path(), true);
    let renewals = Renewals::of(&open(home.path()));
    std::fs::create_dir_all(&renewals.root).unwrap();
    let journal = json!({
        "fingerprint": super::super::view(AgentId::Claude, profile.login.as_ref().unwrap())
            .unwrap()
            .fingerprint(),
        "login": {
            "auth": {"claudeAiOauth": {"accessToken": "journaled", "refreshToken": "journaled-refresh"}},
            "account": {"accountUuid": "user", "organizationUuid": "team"}
        }
    });
    std::fs::write(
        renewals.journal(&profile),
        renewals
            .sealer
            .seal(&serde_json::to_vec(&journal).unwrap())
            .unwrap(),
    )
    .unwrap();

    let renewed = renew_owned(&profile, &|| Ok(open(home.path())), &|_| {
        panic!("a completed journal is adopted, not redeemed again")
    })
    .unwrap();

    assert_eq!(renewed.auth, journal["login"]["auth"]);
    assert_eq!(
        open(home.path()).load().unwrap().profiles[0]
            .login
            .as_ref()
            .unwrap()
            .auth["claudeAiOauth"]["refreshToken"],
        "journaled-refresh"
    );
}

/// Each provider's private renewal redeems at the token endpoint its own client redeems at: a
/// refresh token is issued to one client and refused anywhere else.
#[test]
fn each_providers_private_renewal_goes_to_its_own_token_endpoint() {
    let url = |provider| super::super::adapter(provider).unwrap().token_url();
    assert_eq!(
        url(AgentId::Claude),
        "https://platform.claude.com/v1/oauth/token"
    );
    assert_eq!(url(AgentId::Codex), "https://auth.openai.com/oauth/token");
}
