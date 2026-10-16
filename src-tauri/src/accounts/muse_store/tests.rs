use std::cell::Cell;
use std::path::PathBuf;

use super::*;

const NOW: i64 = 1_800_000_000;

const KEYCHAIN_AUTH: &str = r#"{"schema_version":2,"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"you@example.com"}}}"#;

fn config_with_auth(body: &str) -> PathBuf {
    let config = crate::paths::scratch_dir("muse-store-auth");
    std::fs::create_dir_all(&config).expect("config dir");
    std::fs::write(config.join("auth.json"), body).expect("auth.json");
    config
}

fn no_keychain() -> Option<String> {
    None
}

fn keychain_token() -> Option<String> {
    Some(r#"{"access_token":"keychain-token"}"#.to_string())
}

fn bearer(login: &MuseLogin) -> Option<String> {
    login.token.as_ref().map(AccessToken::authorization)
}

fn login_in(config: &Path, now_secs: i64, keychain: &dyn Fn() -> Option<String>) -> MuseLogin {
    LoginCache::new()
        .login(config, now_secs, keychain)
        .expect("login")
}

#[test]
fn an_inline_device_token_is_loaded_with_its_email() {
    let config = config_with_auth(
        r#"{"schema_version":2,"providers":{"meta":{"mechanism":"oauth","access_token":"device-token","user_email":" You@Example.com "}}}"#,
    );
    let login = login_in(&config, NOW, &|| {
        panic!("an inline token never asks the Keychain")
    });
    assert_eq!(bearer(&login).as_deref(), Some("Bearer device-token"));
    assert_eq!(login.email.as_deref(), Some("You@Example.com"));
}

#[test]
fn a_keychain_login_reads_only_the_bundle_access_token() {
    let config = config_with_auth(KEYCHAIN_AUTH);
    let bundle = r#"{
        "secret_schema_version": 1,
        "access_token": "bundle-access-token",
        "refresh_token": "bundle-refresh-secret",
        "api_key": "LLM|bundle-minted-secret"
    }"#;
    let login = login_in(&config, NOW, &|| Some(bundle.to_string()));
    assert_eq!(
        bearer(&login).as_deref(),
        Some("Bearer bundle-access-token")
    );
}

#[test]
fn a_keychain_value_without_an_access_token_gives_no_token() {
    let config = config_with_auth(KEYCHAIN_AUTH);
    for value in [
        r#"{"secret_schema_version":1,"refresh_token":"bundle-refresh-secret"}"#,
        "bare-token-of-an-unknown-shape",
        "",
    ] {
        let login = login_in(&config, NOW, &|| Some(value.to_string()));
        assert!(login.token.is_none(), "{value}");
        assert_eq!(login.email.as_deref(), Some("you@example.com"), "{value}");
    }
}

#[test]
fn a_denied_or_expired_login_keeps_its_email() {
    let denied = login_in(&config_with_auth(KEYCHAIN_AUTH), NOW, &no_keychain);
    assert!(denied.token.is_none());
    assert_eq!(denied.email.as_deref(), Some("you@example.com"));

    let expired = config_with_auth(
        r#"{"providers":{"meta":{"mechanism":"oauth","access_token":"device-token","expires_at":1800000000,"user_email":"you@example.com"}}}"#,
    );
    let login = login_in(&expired, NOW, &no_keychain);
    assert!(login.token.is_none());
    assert_eq!(login.email.as_deref(), Some("you@example.com"));
    assert!(login_in(&expired, NOW - 1, &no_keychain).token.is_some());
}

#[test]
fn an_auth_file_without_a_meta_login_is_no_login() {
    let missing = crate::paths::scratch_dir("muse-store-missing");
    assert!(LoginCache::new()
        .login(&missing, NOW, &keychain_token)
        .is_none());
    let corrupt = config_with_auth("{not json");
    assert!(LoginCache::new()
        .login(&corrupt, NOW, &keychain_token)
        .is_none());
    let other = config_with_auth(r#"{"providers":{"openai":{"access_token":"other"}}}"#);
    assert!(LoginCache::new()
        .login(&other, NOW, &keychain_token)
        .is_none());
}

#[test]
fn an_api_key_login_has_no_token_and_never_asks_the_keychain() {
    let config = config_with_auth(
        r#"{"providers":{"meta":{"mechanism":"apiKey","user_email":"you@example.com"}}}"#,
    );
    let login = login_in(&config, NOW, &|| {
        panic!("an API-key login never asks the Keychain")
    });
    assert!(login.token.is_none());
    assert_eq!(login.email.as_deref(), Some("you@example.com"));
}

#[test]
fn an_expired_login_does_not_ask_the_keychain() {
    let config = config_with_auth(
        r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain","expires_at":1000}}}"#,
    );
    let login = login_in(&config, NOW, &|| {
        panic!("an expired login never asks the Keychain")
    });
    assert!(login.token.is_none());
}

#[test]
fn a_cached_login_expires_without_a_new_lookup() {
    let config = config_with_auth(
        r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain","expires_at":1800000100}}}"#,
    );
    let calls = Cell::new(0);
    let lookup = || {
        calls.set(calls.get() + 1);
        keychain_token()
    };
    let cache = LoginCache::new();
    let fresh = cache.login(&config, NOW, &lookup).expect("login");
    assert!(fresh.token.is_some());
    let later = cache.login(&config, NOW + 100, &lookup).expect("login");
    assert!(later.token.is_none());
    assert_eq!(calls.get(), 1);
}

#[test]
fn the_keychain_is_read_again_only_when_auth_json_changes() {
    let config = config_with_auth(KEYCHAIN_AUTH);
    let path = config.join("auth.json");
    let stamp = std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .expect("mtime");
    let calls = Cell::new(0);
    let lookup = || {
        calls.set(calls.get() + 1);
        keychain_token()
    };
    let cache = LoginCache::new();
    assert!(cache.login(&config, NOW, &lookup).is_some());
    assert!(cache.login(&config, NOW, &lookup).is_some());
    assert_eq!(calls.get(), 1, "one Keychain lookup while auth.json stands");

    std::fs::write(
        &path,
        r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"other@example.com"}}}"#,
    )
    .expect("rewrite auth.json");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_modified(stamp))
        .expect("keep the old mtime");
    let login = cache.login(&config, NOW, &lookup).expect("login");
    assert_eq!(
        calls.get(),
        2,
        "new auth.json content reads the login again"
    );
    assert_eq!(login.email.as_deref(), Some("other@example.com"));
}

#[test]
fn a_denied_lookup_is_remembered_until_auth_json_changes() {
    let config = config_with_auth(KEYCHAIN_AUTH);
    let calls = Cell::new(0);
    let denied = || {
        calls.set(calls.get() + 1);
        None
    };
    let cache = LoginCache::new();
    assert!(cache
        .login(&config, NOW, &denied)
        .expect("login")
        .token
        .is_none());
    assert!(cache
        .login(&config, NOW, &denied)
        .expect("login")
        .token
        .is_none());
    assert_eq!(calls.get(), 1);
}

#[test]
fn an_expiry_of_another_shape_keeps_the_token() {
    for expires_at in [r#""2030-01-01""#, "1.5e9", "null"] {
        let config = config_with_auth(&format!(
            r#"{{"providers":{{"meta":{{"mechanism":"oauth","access_token":"device-token","expires_at":{expires_at}}}}}}}"#
        ));
        assert!(
            login_in(&config, NOW, &no_keychain).token.is_some(),
            "{expires_at}"
        );
    }
}
