use std::cell::Cell;
use std::path::PathBuf;

use super::*;

const NOW: i64 = 1_800_000_000;

fn config_with_auth(body: &str) -> PathBuf {
    let config = crate::paths::scratch_dir("muse-store-auth");
    std::fs::create_dir_all(&config).expect("config dir");
    std::fs::write(config.join("auth.json"), body).expect("auth.json");
    config
}

fn no_keychain() -> Option<String> {
    None
}

fn bearer(login: &MuseLogin) -> String {
    login.token.authorization()
}

const KEYCHAIN_AUTH: &str = r#"{"schema_version":2,"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"you@example.com"}}}"#;

#[test]
fn an_inline_device_token_is_loaded_with_its_email() {
    let config = config_with_auth(
        r#"{"schema_version":2,"providers":{"meta":{"mechanism":"oauth","access_token":"device-token","user_email":" You@Example.com "}}}"#,
    );
    let login = LoginCache::new()
        .login(&config, NOW, &no_keychain)
        .expect("login");
    assert_eq!(bearer(&login), "Bearer device-token");
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
    let login = LoginCache::new()
        .login(&config, NOW, &|| Some(bundle.to_string()))
        .expect("login");
    assert_eq!(bearer(&login), "Bearer bundle-access-token");
    assert!(LoginCache::new()
        .login(&config, NOW, &no_keychain)
        .is_none());
}

#[test]
fn a_keychain_value_without_an_access_token_yields_no_login() {
    let config = config_with_auth(KEYCHAIN_AUTH);
    for value in [
        r#"{"secret_schema_version":1,"refresh_token":"bundle-refresh-secret"}"#,
        "bare-token-of-an-unknown-shape",
        "",
    ] {
        assert!(
            LoginCache::new()
                .login(&config, NOW, &|| Some(value.to_string()))
                .is_none(),
            "{value}"
        );
    }
}

#[test]
fn credentials_the_mint_cannot_use_yield_no_login() {
    let missing = crate::paths::scratch_dir("muse-store-missing");
    assert!(LoginCache::new()
        .login(&missing, NOW, &|| Some(
            r#"{"access_token":"unused"}"#.to_string()
        ))
        .is_none());
    let api_key = config_with_auth(r#"{"providers":{"meta":{"mechanism":"apiKey"}}}"#);
    assert!(LoginCache::new()
        .login(&api_key, NOW, &|| Some(
            r#"{"access_token":"unused"}"#.to_string()
        ))
        .is_none());
    let corrupt = config_with_auth("{not json");
    assert!(LoginCache::new()
        .login(&corrupt, NOW, &no_keychain)
        .is_none());
}

#[test]
fn an_expired_login_is_not_used_and_skips_the_keychain() {
    let inline = config_with_auth(
        r#"{"providers":{"meta":{"mechanism":"oauth","access_token":"device-token","expires_at":1800000000}}}"#,
    );
    assert!(LoginCache::new()
        .login(&inline, NOW, &no_keychain)
        .is_none());
    assert!(LoginCache::new()
        .login(&inline, NOW - 1, &no_keychain)
        .is_some());

    let keychain = config_with_auth(
        r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain","expires_at":1000}}}"#,
    );
    let calls = Cell::new(0);
    let lookup = || {
        calls.set(calls.get() + 1);
        Some(r#"{"access_token":"keychain-token"}"#.to_string())
    };
    assert!(LoginCache::new().login(&keychain, NOW, &lookup).is_none());
    assert_eq!(calls.get(), 0);
}

#[test]
fn a_cached_login_expires_without_a_new_lookup() {
    let config = config_with_auth(
        r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain","expires_at":1800000100}}}"#,
    );
    let calls = Cell::new(0);
    let lookup = || {
        calls.set(calls.get() + 1);
        Some(r#"{"access_token":"keychain-token"}"#.to_string())
    };
    let cache = LoginCache::new();
    assert!(cache.login(&config, NOW, &lookup).is_some());
    assert!(cache.login(&config, NOW + 100, &lookup).is_none());
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
        Some(r#"{"access_token":"keychain-token"}"#.to_string())
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
    assert!(cache.login(&config, NOW, &denied).is_none());
    assert!(cache.login(&config, NOW, &denied).is_none());
    assert_eq!(calls.get(), 1);
}
