use std::cell::Cell;
use std::path::PathBuf;

use super::*;
use serde_json::json;

const NOW: i64 = 1_800_000_000;

const KEYCHAIN_AUTH: &str = r#"{"schema_version":2,"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"you@example.com"}}}"#;

fn config_with_auth(body: &str) -> PathBuf {
    let config = crate::paths::scratch_dir("muse-store-auth");
    std::fs::create_dir_all(&config).expect("config dir");
    std::fs::write(config.join("auth.json"), body).expect("auth.json");
    config
}

fn no_keychain() -> Lookup {
    Ok(None)
}

fn denied() -> Lookup {
    Err("denied".into())
}

fn keychain_token() -> Lookup {
    Ok(Some(r#"{"access_token":"keychain-token"}"#.to_string()))
}

fn bearer(login: &MuseLogin) -> Option<String> {
    login.token.as_ref().map(AccessToken::authorization)
}

fn login_in(config: &Path, now_secs: i64, keychain: &dyn Fn() -> Lookup) -> MuseLogin {
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
    let login = login_in(&config, NOW, &|| Ok(Some(bundle.to_string())));
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
        let login = login_in(&config, NOW, &|| Ok(Some(value.to_string())));
        assert!(login.token.is_none(), "{value}");
        assert_eq!(login.email.as_deref(), Some("you@example.com"), "{value}");
    }
}

#[test]
fn a_denied_or_expired_login_keeps_its_email() {
    let denied = login_in(&config_with_auth(KEYCHAIN_AUTH), NOW, &denied);
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
    for _ in 0..2 {
        let login = cache.login(&config, NOW, &lookup).expect("login");
        assert_eq!(bearer(&login).as_deref(), Some("Bearer keychain-token"));
    }
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
        Err("denied".to_string())
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

#[derive(Default)]
struct FakeKeychain {
    item: std::cell::RefCell<Option<String>>,
    writes: Cell<u32>,
    fail_reads: bool,
}

impl FakeKeychain {
    fn holding(raw: &str) -> Self {
        Self {
            item: std::cell::RefCell::new(Some(raw.to_string())),
            ..Self::default()
        }
    }

    fn item(&self) -> Option<Value> {
        self.item
            .borrow()
            .as_deref()
            .map(|raw| serde_json::from_str(raw).unwrap())
    }
}

impl Keychain for FakeKeychain {
    fn read(&self) -> Result<Option<String>, String> {
        if self.fail_reads {
            return Err("denied".into());
        }
        Ok(self.item.borrow().clone())
    }

    fn write(&self, raw: &[u8]) -> Result<(), String> {
        self.writes.set(self.writes.get() + 1);
        *self.item.borrow_mut() = Some(String::from_utf8(raw.to_vec()).unwrap());
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        *self.item.borrow_mut() = None;
        Ok(())
    }
}

fn read_native(config: &Path, keychain: &FakeKeychain) -> Result<Option<NativeLogin>, String> {
    LoginCache::new().native(config, &|| keychain.read())
}

fn metadata(email: &str) -> Map<String, Value> {
    json!({"mechanism": "oauth", "obtained_via": "device_code", "user_email": email})
        .as_object()
        .unwrap()
        .clone()
}

fn secret(token: &str) -> Map<String, Value> {
    json!({"access_token": token, "api_key": "key-1"})
        .as_object()
        .unwrap()
        .clone()
}

fn written(config: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(config.join("auth.json")).unwrap()).unwrap()
}

#[test]
fn every_store_reads_the_same_secret_shape() {
    let keychain = FakeKeychain::holding(
        r#"{"secret_schema_version":3,"api_key":"key-1","access_token":"t1"}"#,
    );
    let in_keychain = read_native(&config_with_auth(KEYCHAIN_AUTH), &keychain)
        .unwrap()
        .unwrap();
    let inline = read_native(
        &config_with_auth(r#"{"providers":{"meta":{"mechanism":"oauth","access_token":"t1","api_key":"key-1","user_email":"you@example.com"}}}"#),
        &FakeKeychain::default(),
    )
    .unwrap()
    .unwrap();

    assert_eq!(in_keychain.secret, secret("t1"));
    assert_eq!(inline.secret, secret("t1"));
    assert_eq!(in_keychain.storage, Storage::Keychain);
    assert_eq!(inline.storage, Storage::Inline);
    assert!(!in_keychain.metadata.contains_key("storage"));
    assert!(!inline.metadata.contains_key("access_token"));
}

#[test]
fn an_oauth_login_without_storage_or_token_still_reads_the_keychain() {
    let config = config_with_auth(
        r#"{"providers":{"meta":{"mechanism":"oauth","user_email":"you@example.com"}}}"#,
    );
    let keychain = FakeKeychain::holding(r#"{"access_token":"t1","api_key":"key-1"}"#);

    let login = read_native(&config, &keychain).unwrap().unwrap();

    assert_eq!(login.storage, Storage::Keychain);
    assert_eq!(login.secret, secret("t1"));
}

#[test]
fn metadata_without_a_credential_is_no_login() {
    for body in [
        r#"{"providers":{"meta":{"mechanism":"apiKey","user_email":"you@example.com"}}}"#,
        r#"{"providers":{"meta":{"user_email":"you@example.com"}}}"#,
        r#"{"providers":{"openai":{"access_token":"other"}}}"#,
    ] {
        assert_eq!(
            read_native(&config_with_auth(body), &FakeKeychain::default()),
            Ok(None),
            "{body}"
        );
    }
    assert_eq!(
        read_native(&config_with_auth(KEYCHAIN_AUTH), &FakeKeychain::default()),
        Ok(None)
    );
}

#[test]
fn a_denied_lookup_or_a_malformed_item_is_an_error_never_no_login() {
    let config = config_with_auth(KEYCHAIN_AUTH);
    let refused = FakeKeychain {
        fail_reads: true,
        ..FakeKeychain::default()
    };

    assert!(read_native(&config, &refused).is_err());
    assert!(read_native(&config, &FakeKeychain::holding("not json")).is_err());
    assert!(read_native(&config_with_auth("[1,2]"), &FakeKeychain::default()).is_err());
}

#[test]
fn a_write_keeps_the_login_in_the_store_muse_already_uses() {
    let inline = config_with_auth(
        r#"{"schema_version":2,"providers":{"meta":{"mechanism":"oauth","access_token":"old"},"anthropic":{"api_key":"other"}}}"#,
    );
    let keychain = FakeKeychain::holding(
        r#"{"secret_schema_version":3,"access_token":"old","api_key":"old"}"#,
    );

    LoginCache::new()
        .write(
            &inline,
            Some(&keychain),
            Some((&metadata("you@example.com"), &secret("t1"))),
        )
        .unwrap();

    let document = written(&inline);
    assert_eq!(document["providers"]["meta"]["access_token"], "t1");
    assert_eq!(document["providers"]["meta"]["api_key"], "key-1");
    assert!(document["providers"]["meta"].get("storage").is_none());
    assert_eq!(document["providers"]["anthropic"]["api_key"], "other");
    assert_eq!(document["schema_version"], 2);
    assert_eq!(keychain.writes.get(), 0, "an inline login stays inline");
}

#[test]
fn a_keychain_login_is_written_back_to_the_item_keeping_its_schema_version() {
    let config = config_with_auth(KEYCHAIN_AUTH);
    let keychain = FakeKeychain::holding(
        r#"{"secret_schema_version":3,"access_token":"old","api_key":"old"}"#,
    );

    LoginCache::new()
        .write(
            &config,
            Some(&keychain),
            Some((&metadata("new@example.com"), &secret("t1"))),
        )
        .unwrap();

    assert_eq!(
        keychain.item(),
        Some(json!({"secret_schema_version": 3, "access_token": "t1", "api_key": "key-1"}))
    );
    let meta = &written(&config)["providers"]["meta"];
    assert_eq!(meta["storage"], "keychain");
    assert_eq!(meta["user_email"], "new@example.com");
    assert!(meta.get("access_token").is_none() && meta.get("api_key").is_none());
    assert_eq!(
        read_native(&config, &keychain).unwrap().unwrap().secret,
        secret("t1")
    );
}

#[test]
fn a_first_login_goes_where_the_platform_keeps_it_with_a_fresh_document() {
    let to_keychain = crate::paths::scratch_dir("muse-store-first-keychain");
    let keychain = FakeKeychain::default();
    LoginCache::new()
        .write(
            &to_keychain,
            Some(&keychain),
            Some((&metadata("you@example.com"), &secret("t1"))),
        )
        .unwrap();
    let inline = crate::paths::scratch_dir("muse-store-first-inline");
    LoginCache::new()
        .write(
            &inline,
            None,
            Some((&metadata("you@example.com"), &secret("t1"))),
        )
        .unwrap();

    assert_eq!(
        keychain.item().unwrap()["secret_schema_version"],
        DEFAULT_SCHEMA_VERSION
    );
    assert_eq!(written(&to_keychain)["schema_version"], 1);
    assert_eq!(
        written(&to_keychain)["providers"]["meta"]["storage"],
        "keychain"
    );
    assert_eq!(written(&inline)["schema_version"], 1);
    assert_eq!(written(&inline)["providers"]["meta"]["access_token"], "t1");
}

#[test]
fn a_malformed_item_or_document_is_refused_before_anything_is_written() {
    let config = config_with_auth(KEYCHAIN_AUTH);
    let malformed = FakeKeychain::holding("not json");
    assert!(LoginCache::new()
        .write(
            &config,
            Some(&malformed),
            Some((&metadata("you@example.com"), &secret("t1")))
        )
        .is_err());
    assert_eq!(malformed.writes.get(), 0);
    assert_eq!(
        std::fs::read_to_string(config.join("auth.json")).unwrap(),
        KEYCHAIN_AUTH
    );

    let broken = config_with_auth("[1,2]");
    let keychain = FakeKeychain::default();
    assert!(LoginCache::new()
        .write(
            &broken,
            Some(&keychain),
            Some((&metadata("you@example.com"), &secret("t1")))
        )
        .is_err());
    assert_eq!(keychain.writes.get(), 0);
    assert_eq!(
        std::fs::read_to_string(broken.join("auth.json")).unwrap(),
        "[1,2]"
    );
}

#[test]
fn a_keychain_login_cannot_be_written_where_the_keychain_is_out_of_reach() {
    let config = config_with_auth(KEYCHAIN_AUTH);

    assert!(LoginCache::new()
        .write(
            &config,
            None,
            Some((&metadata("you@example.com"), &secret("t1")))
        )
        .is_err());
    assert_eq!(
        std::fs::read_to_string(config.join("auth.json")).unwrap(),
        KEYCHAIN_AUTH
    );
}

#[cfg(unix)]
#[test]
fn a_linked_login_file_is_refused_before_the_keychain_is_touched() {
    let config = crate::paths::scratch_dir("muse-store-linked");
    std::fs::create_dir_all(&config).unwrap();
    let target = config.join("elsewhere.json");
    std::fs::write(&target, KEYCHAIN_AUTH).unwrap();
    std::os::unix::fs::symlink(&target, config.join("auth.json")).unwrap();
    let keychain = FakeKeychain::holding(r#"{"access_token":"old"}"#);

    assert!(LoginCache::new()
        .write(
            &config,
            Some(&keychain),
            Some((&metadata("you@example.com"), &secret("t1")))
        )
        .is_err());
    assert_eq!(keychain.writes.get(), 0);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), KEYCHAIN_AUTH);
}

#[test]
fn clearing_removes_the_login_from_its_store_and_keeps_other_providers() {
    let config = config_with_auth(
        r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain"},"anthropic":{}}}"#,
    );
    let keychain = FakeKeychain::holding(r#"{"access_token":"t1"}"#);

    LoginCache::new()
        .write(&config, Some(&keychain), None)
        .unwrap();

    assert_eq!(keychain.item(), None);
    assert!(written(&config)["providers"].get("meta").is_none());
    assert!(written(&config)["providers"].get("anthropic").is_some());
    assert_eq!(read_native(&config, &keychain), Ok(None));
}

#[test]
fn a_write_forgets_the_cached_lookup_so_the_next_read_sees_the_new_login() {
    let config = config_with_auth(KEYCHAIN_AUTH);
    let keychain = FakeKeychain::holding(r#"{"access_token":"old","api_key":"old"}"#);
    let cache = LoginCache::new();
    let lookup = || keychain.read();
    assert_eq!(
        cache.native(&config, &lookup).unwrap().unwrap().secret["access_token"],
        "old"
    );

    cache
        .write(
            &config,
            Some(&keychain),
            Some((&metadata("you@example.com"), &secret("t1"))),
        )
        .unwrap();

    assert_eq!(
        cache.native(&config, &lookup).unwrap().unwrap().secret["access_token"],
        "t1"
    );
}

#[test]
fn a_muse_identity_needs_an_email_and_lowercases_it() {
    assert_eq!(
        identity(" You@Example.com "),
        Some(Identity {
            provider: AgentId::Muse,
            user_id: "you@example.com".into(),
            workspace_id: String::new(),
        })
    );
    assert_eq!(identity("default"), None);
    assert_eq!(identity(""), None);
}
