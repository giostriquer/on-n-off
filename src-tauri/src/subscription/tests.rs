use super::*;
use crate::accounts::model::Identity;
use crate::dto::AgentId;
use serde_json::json;

fn now() -> DateTime<Utc> {
    "2026-09-10T12:00:00Z".parse().unwrap()
}
fn claims() -> Value {
    json!({
        "chatgpt_account_id": "account-a",
        "chatgpt_subscription_active_until": "2026-10-10T12:00:00Z",
        "chatgpt_subscription_last_checked": "2026-09-09T12:00:00Z"
    })
}
/// A Codex `auth.json` whose ID token carries these `https://api.openai.com/auth` claims.
fn auth(claims: &Value) -> Value {
    json!({"tokens": {
        "account_id": claims["chatgpt_account_id"],
        "id_token": crate::accounts::codex::tests::id_token(claims),
        "access_token": "SECRET-ACCESS",
        "refresh_token": "SECRET-REFRESH"
    }})
}

#[test]
fn token_date_is_the_paid_through_date_with_its_check_time() {
    let result = parse_claims(&claims(), "account-a", now()).unwrap();
    assert_eq!(result.date, "2026-10-10T12:00:00+00:00");
    assert_eq!(
        result.checked_at.as_deref(),
        Some("2026-09-09T12:00:00+00:00")
    );
    let mut unchecked = claims();
    unchecked["chatgpt_subscription_last_checked"] = json!("2026-09-11T12:00:00Z");
    assert_eq!(
        parse_claims(&unchecked, "account-a", now())
            .unwrap()
            .checked_at,
        None,
        "a check time in the future is not a check"
    );
}

#[test]
fn token_rejects_wrong_account_elapsed_missing_and_invalid_dates() {
    assert!(parse_claims(&claims(), "account-b", now()).is_none());
    for value in ["2026-09-09T12:00:00Z", "bad", "2026-09-10T12:00:00Z"] {
        let mut payload = claims();
        payload["chatgpt_subscription_active_until"] = json!(value);
        assert!(parse_claims(&payload, "account-a", now()).is_none());
    }
    let mut missing = claims();
    missing
        .as_object_mut()
        .unwrap()
        .remove("chatgpt_subscription_active_until");
    assert!(parse_claims(&missing, "account-a", now()).is_none());
}

#[test]
fn the_signed_in_login_gives_its_own_date_and_is_only_read() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    let path = home.path().join(".codex/auth.json");
    std::fs::write(&path, auth(&claims()).to_string()).unwrap();
    let before = std::fs::read(&path).unwrap();

    let date = read_at(home.path(), "account-a", now()).unwrap().unwrap();
    assert_eq!(date.date, "2026-10-10T12:00:00+00:00");
    assert_eq!(read_at(home.path(), "account-b", now()).unwrap(), None);
    assert_eq!(
        read_at(home.path(), "profile:unknown", now()).unwrap(),
        None,
        "no vault means no saved profile, not an error"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(
        !home.path().join(".on-n-off").exists(),
        "a token read persists nothing"
    );
}

#[test]
fn a_saved_profile_gives_its_date_from_the_vault_without_a_native_login() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".on-n-off/accounts")).unwrap();
    crate::accounts::vault::tests::unlock_fixture(&home);
    let identity = Identity {
        provider: AgentId::Codex,
        user_id: "user-saved".into(),
        workspace_id: "workspace-saved".into(),
    };
    let mut saved = claims();
    saved["chatgpt_account_id"] = json!("workspace-saved");
    saved["chatgpt_user_id"] = json!("user-saved");
    let key = crate::accounts::saved_codex_fixture(home.path(), identity, auth(&saved));

    let date = read_at(home.path(), &key, now()).unwrap().unwrap();
    assert_eq!(date.date, "2026-10-10T12:00:00+00:00");
    assert_eq!(
        read_at(home.path(), "profile:someone-else", now()).unwrap(),
        None
    );
}

#[test]
fn the_signed_in_login_answers_for_itself_and_the_vault_for_everyone_else() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".on-n-off/accounts")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    crate::accounts::vault::tests::unlock_fixture(&home);
    let identity = |user: &str| Identity {
        provider: AgentId::Codex,
        user_id: user.into(),
        workspace_id: "workspace".into(),
    };
    let dated = |user: &str, until: &str| {
        let mut claims = claims();
        claims["chatgpt_account_id"] = json!("workspace");
        claims["chatgpt_user_id"] = json!(user);
        claims["chatgpt_subscription_active_until"] = json!(until);
        claims
    };
    // A is signed in with the live date and also saved with an older one; B is only saved.
    let auth_path = home.path().join(".codex/auth.json");
    std::fs::write(
        &auth_path,
        auth(&dated("user-a", "2026-11-01T12:00:00Z")).to_string(),
    )
    .unwrap();
    let a = crate::accounts::saved_codex_fixture(
        home.path(),
        identity("user-a"),
        auth(&dated("user-a", "2026-09-20T12:00:00Z")),
    );
    let b = crate::accounts::saved_codex_fixture(
        home.path(),
        identity("user-b"),
        auth(&dated("user-b", "2026-12-01T12:00:00Z")),
    );
    let date = |key: &str| read_at(home.path(), key, now()).unwrap().unwrap().date;
    assert_eq!(
        date(&a),
        "2026-11-01T12:00:00+00:00",
        "the live login beats its saved copy"
    );
    assert_eq!(date(&b), "2026-12-01T12:00:00+00:00");

    // A native login nothing can parse costs the saved profiles nothing, and A's saved copy
    // stands in for it.
    std::fs::write(&auth_path, "not json").unwrap();
    assert_eq!(date(&b), "2026-12-01T12:00:00+00:00");
    assert_eq!(date(&a), "2026-09-20T12:00:00+00:00");
}

#[test]
fn the_reading_crosses_ipc_as_date_and_checked_at_or_null() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    let mut far = claims();
    far["chatgpt_subscription_active_until"] = json!("2999-01-01T00:00:00Z");
    std::fs::write(home.path().join(".codex/auth.json"), auth(&far).to_string()).unwrap();
    assert_eq!(
        serde_json::to_value(read(home.path(), "account-a").unwrap()).unwrap(),
        json!({"date": "2999-01-01T00:00:00+00:00", "checkedAt": "2026-09-09T12:00:00+00:00"}),
        "the UI reads these two names"
    );
    assert_eq!(
        serde_json::to_value(read(home.path(), "account-b").unwrap()).unwrap(),
        Value::Null
    );
}

#[test]
fn the_legacy_import_cache_is_discarded_and_nothing_beside_it() {
    let home = tempfile::tempdir().unwrap();
    let legacy = home.path().join(".on-n-off/subscriptions");
    let kept = home.path().join(".on-n-off/limits");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::create_dir_all(&kept).unwrap();
    std::fs::write(legacy.join("codex-abc.json"), "{}").unwrap();
    std::fs::write(kept.join("codex-abc.json"), "{}").unwrap();
    discard_legacy_cache(home.path());
    assert!(!legacy.exists());
    assert!(kept.join("codex-abc.json").exists());
    discard_legacy_cache(home.path());
    assert!(!legacy.exists(), "a second run on a clean home is a no-op");
}

#[test]
fn a_busy_saved_account_vault_is_retryable_not_an_absent_date() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(".on-n-off/accounts");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("vault.enc"), "fixture-not-read-under-contention").unwrap();
    crate::accounts::vault::tests::unlock_fixture(&home);
    let lease = std::fs::File::create(root.join("operation.lock")).unwrap();
    lease.try_lock().unwrap();
    // Production's ten seconds would add ten to the suite.
    let _short = crate::accounts::override_lease_timeout(std::time::Duration::from_millis(100));
    let error = read_at(home.path(), "profile:inactive", now()).unwrap_err();
    assert!(
        error.contains("operation"),
        "must preserve the retryable vault error: {error}"
    );
    assert_eq!(
        read_at(home.path(), "account-b", now()),
        Ok(None),
        "a legacy workspace key never opens the vault"
    );
}
