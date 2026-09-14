use super::*;
use serde_json::json;
fn now() -> DateTime<Utc> {
    "2026-09-10T12:00:00Z".parse().unwrap()
}
fn claims() -> Value {
    json!({"chatgpt_account_id":"account-a", "chatgpt_subscription_active_until":"2026-10-10T12:00:00Z", "chatgpt_subscription_last_checked":"2026-09-09T12:00:00Z"})
}
#[test]
fn billing_distinguishes_expiration_and_renewal() {
    for (renew, kind) in [(true, DateKind::Renews), (false, DateKind::Expires)] {
        let result = parse_billing(
            &json!({"active_until":"2026-10-10T12:00:00Z", "will_renew":renew}),
            now(),
        )
        .unwrap();
        assert_eq!(result.kind, Some(kind));
        assert_eq!(result.date.as_deref(), Some("2026-10-10T12:00:00+00:00"));
        assert!(!result.stale);
    }
}
#[test]
fn malformed_billing_is_unavailable_but_explicit_empty_is_authoritative() {
    for payload in [
        json!({}),
        json!({"active_until":null}),
        json!({"active_until":"bad","will_renew":true}),
        json!({"active_until":null,"will_renew":1}),
        json!({"active_until":null,"will_renew":true}),
    ] {
        assert!(parse_billing(&payload, now()).is_none());
    }
    let empty = parse_billing(&json!({"active_until":null,"will_renew":false}), now()).unwrap();
    let local = parse_claims(&claims(), "account-a", now());
    assert_eq!(choose(Some(empty.clone()), local), Some(empty));
}
#[test]
fn token_date_is_cached_paid_through_never_cancellation() {
    let result = parse_claims(&claims(), "account-a", now()).unwrap();
    assert_eq!(result.kind, Some(DateKind::PaidThrough));
    assert_eq!(result.source, DateSource::LocalToken);
    assert!(result.stale);
    assert_eq!(
        result.checked_at.as_deref(),
        Some("2026-09-09T12:00:00+00:00")
    );
}
#[test]
fn token_rejects_wrong_account_elapsed_and_invalid_dates() {
    assert!(parse_claims(&claims(), "account-b", now()).is_none());
    for value in ["2026-09-09T12:00:00Z", "bad", "2026-09-10T12:00:00Z"] {
        let mut payload = claims();
        payload["chatgpt_subscription_active_until"] = json!(value);
        assert!(parse_claims(&payload, "account-a", now()).is_none());
    }
}
#[test]
fn cached_billing_status_wins_over_newer_token_claim() {
    let mut billing = parse_billing(
        &json!({"active_until":"2026-10-10T12:00:00Z","will_renew":false}),
        now(),
    )
    .unwrap();
    billing.stale = true;
    let result = choose(Some(billing), parse_claims(&claims(), "account-a", now())).unwrap();
    assert_eq!(result.kind, Some(DateKind::Expires));
    assert!(result.stale);
}

pub(super) fn fixture_home() -> std::path::PathBuf {
    use base64::Engine;
    let home = crate::paths::scratch_dir("subscription");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    let payload = json!({"https://api.openai.com/auth":claims()});
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string());
    let auth = json!({"tokens":{"account_id":"account-a","id_token":format!("header.{encoded}.signature"),"access_token":"SECRET-ACCESS","refresh_token":"SECRET-REFRESH"}});
    std::fs::write(home.join(".codex/auth.json"), auth.to_string()).unwrap();
    home
}
#[test]
fn stored_empty_billing_survives_reload_and_suppresses_token_fallback() {
    let home = fixture_home();
    let empty = parse_billing(&json!({"active_until":null,"will_renew":false}), now()).unwrap();
    store::save(&home, "account-a", &empty).unwrap();
    let (id, claims) = store::identity(&home).unwrap();
    assert_eq!(
        choose(store::load(&home, &id), parse_claims(&claims, &id, now())),
        Some(empty)
    );
    assert!(store::load(&home, "account-b").is_none());
    store::forget(&home, "account-a").unwrap();
    assert!(store::load(&home, "account-a").is_none());
    std::fs::remove_dir_all(home).unwrap();
}
#[test]
fn reading_identity_does_not_write_credentials_and_stores_only_dates() {
    let home = fixture_home();
    let before = std::fs::read(home.join(".codex/auth.json")).unwrap();
    assert_eq!(store::identity(&home).unwrap().0, "account-a");
    let billing = parse_billing(
        &json!({"active_until":"2026-10-10T12:00:00Z","will_renew":false}),
        now(),
    )
    .unwrap();
    store::save(&home, "account-a", &billing).unwrap();
    let file = std::fs::read_dir(home.join(".on-n-off/subscriptions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let stored = std::fs::read_to_string(file).unwrap();
    assert!(!stored.contains("SECRET"));
    assert!(!stored.contains("id_token"));
    assert_eq!(
        std::fs::read(home.join(".codex/auth.json")).unwrap(),
        before
    );
    let mut auth: Value = serde_json::from_slice(&before).unwrap();
    auth["tokens"]["account_id"] = json!("account-b");
    std::fs::write(home.join(".codex/auth.json"), auth.to_string()).unwrap();
    assert!(store::identity(&home).is_none());
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn automatic_refresh_persists_its_cooldown() {
    let home = fixture_home();
    let now = "2026-09-10T12:00:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    assert!(store::claim_auto_refresh(&home, "account-a", now));
    let metadata = parse_billing(
        &serde_json::json!({"active_until":"2026-10-10T12:00:00Z","will_renew":false}),
        now,
    )
    .unwrap();
    store::save(&home, "account-a", &metadata).unwrap();
    assert!(!store::claim_auto_refresh(
        &home,
        "account-a",
        now + chrono::Duration::hours(23)
    ));
    assert!(store::claim_auto_refresh(
        &home,
        "account-a",
        now + chrono::Duration::hours(24)
    ));
    // A failed fetch leaves the old metadata. Reopening the store must retain the cooldown.
    assert!(!store::claim_auto_refresh(
        &home,
        "account-a",
        now + chrono::Duration::hours(25)
    ));
    assert!(store::claim_auto_refresh(
        &home,
        "account-b",
        now + chrono::Duration::hours(48)
    ));
    assert!(store::claim_auto_refresh(
        &home,
        "account-a",
        now + chrono::Duration::hours(48)
    ));
    assert!(!store::claim_auto_refresh(&home, "account-a", now));
    assert_eq!(store::load(&home, "account-a").unwrap(), metadata);
    store::forget(&home, "account-a").unwrap();
    assert!(store::claim_auto_refresh(
        &home,
        "account-a",
        now + chrono::Duration::days(3)
    ));
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn first_billing_attempt_is_throttled_across_restarts_without_inventing_metadata() {
    let home = fixture_home();
    assert!(store::claim_auto_refresh(&home, "account-a", now()));
    assert!(store::load(&home, "account-a").is_none());
    assert!(!store::claim_auto_refresh(
        &home,
        "account-a",
        now() + chrono::Duration::hours(1)
    ));
    assert!(store::claim_auto_refresh(
        &home,
        "account-a",
        now() + chrono::Duration::days(1)
    ));
    assert!(store::load(&home, "account-a").is_none());
}

#[test]
fn a_busy_saved_account_vault_is_retryable_not_an_absent_identity() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(".on-n-off/accounts");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("vault.enc"), "fixture-not-read-under-contention").unwrap();
    crate::accounts::vault::tests::unlock_fixture(&home);
    let lease = std::fs::File::create(root.join("operation.lock")).unwrap();
    lease.try_lock().unwrap();
    assert!(browser::read_metadata(home.path(), "profile:inactive", now()).is_err());
    let error = validate_account(home.path(), "profile:inactive").unwrap_err();
    assert!(
        error.contains("operation"),
        "must preserve the retryable vault error: {error}"
    );
}
