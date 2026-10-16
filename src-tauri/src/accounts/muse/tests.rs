use super::*;
use crate::http::serve_once;
use crate::paths::scratch_dir;

fn login(email: &str, token: &str) -> Login {
    Login {
        auth: json!({"access_token": token, "api_key": "key-1"}),
        account: json!({
            "mechanism": "oauth",
            "obtained_via": "device_code",
            "api_base_url": "https://api.meta.ai/v1",
            "subs_active": true,
            "user_full_name": "Some One",
            "user_email": email,
        }),
    }
}

fn file_store(name: &str) -> (PathBuf, MuseNative) {
    let dir = scratch_dir(name);
    (dir.clone(), MuseNative::private(&dir))
}

fn leaked(url: String) -> &'static str {
    Box::leak(url.into_boxed_str())
}

#[test]
fn a_muse_login_is_identified_by_its_lowercased_email_and_never_by_an_api_key() {
    let saved = login("You@Example.com", "t1");
    assert_eq!(
        MuseView::of(&saved).identity().unwrap(),
        Identity {
            provider: AgentId::Muse,
            user_id: "you@example.com".into(),
            workspace_id: String::new(),
        }
    );

    let mut api_key = login("you@example.com", "t1");
    api_key.account["mechanism"] = json!("api_key");
    assert!(MuseView::of(&api_key).identity().is_err());

    let mut nameless = login("", "t1");
    nameless
        .account
        .as_object_mut()
        .unwrap()
        .remove("user_email");
    assert!(MuseView::of(&nameless).identity().is_err());
    assert!(MuseView::of(&login("you@example.com", ""))
        .identity()
        .is_err());
}

#[test]
fn a_new_token_is_a_new_login_generation() {
    let first = MuseView::of(&login("you@example.com", "t1")).fingerprint();
    assert_eq!(
        first,
        MuseView::of(&login("you@example.com", "t1")).fingerprint()
    );
    assert_ne!(
        first,
        MuseView::of(&login("you@example.com", "t2")).fingerprint()
    );
    assert!(!MuseView::of(&login("you@example.com", "t1")).renewal_due(i64::MAX));
}

#[test]
fn a_home_keeps_its_login_inline_and_leaves_other_providers_alone() {
    let (dir, store) = file_store("muse-file-login");
    fs::create_dir_all(&store.config).unwrap();
    fs::write(
        store.auth_file(),
        r#"{"schema_version":1,"providers":{"anthropic":{"api_key":"other"}}}"#,
    )
    .unwrap();

    store.write(Some(&login("you@example.com", "t1"))).unwrap();

    let written: Value = serde_json::from_slice(&fs::read(store.auth_file()).unwrap()).unwrap();
    assert_eq!(written["providers"]["anthropic"]["api_key"], "other");
    assert_eq!(written["schema_version"], 1);
    assert_eq!(written["providers"]["meta"]["access_token"], "t1");
    assert_eq!(written["providers"]["meta"]["api_key"], "key-1");
    assert_eq!(
        written["providers"]["meta"]["user_email"],
        "you@example.com"
    );
    assert!(written["providers"]["meta"].get("storage").is_none());
    let back = store.read().unwrap().unwrap();
    assert_eq!(back.auth, json!({"access_token": "t1", "api_key": "key-1"}));
    assert_eq!(back.account, login("you@example.com", "t1").account);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(store.auth_file())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    store.write(None).unwrap();
    let cleared: Value = serde_json::from_slice(&fs::read(store.auth_file()).unwrap()).unwrap();
    assert!(cleared["providers"].get("meta").is_none());
    assert_eq!(cleared["providers"]["anthropic"]["api_key"], "other");
    assert!(store.read().unwrap().is_none());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_malformed_login_document_is_refused_and_left_unchanged() {
    let (dir, store) = file_store("muse-malformed-login");
    fs::create_dir_all(&store.config).unwrap();
    fs::write(store.auth_file(), "[1,2]").unwrap();

    assert!(store.write(Some(&login("you@example.com", "t1"))).is_err());
    assert!(store.read().is_err());
    assert_eq!(fs::read_to_string(store.auth_file()).unwrap(), "[1,2]");
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_linked_login_file_is_never_replaced() {
    let (dir, store) = file_store("muse-linked-login");
    fs::create_dir_all(&store.config).unwrap();
    let target = dir.join("elsewhere.json");
    fs::write(&target, "{}").unwrap();
    std::os::unix::fs::symlink(&target, store.auth_file()).unwrap();

    assert!(store.write(Some(&login("you@example.com", "t1"))).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "{}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_signed_in_login_is_only_activated_from_the_default_home_without_an_api_key() {
    let home = Path::new("/home/someone");
    let none = |_: &str| None;
    let native = MuseNative::resolve_from(home, &none).unwrap();
    assert!(native.preflight_in(&none).is_ok());
    assert!(native
        .preflight_in(&|name: &str| (name == "META_API_KEY").then(|| OsString::from("k")))
        .is_err());

    for name in OVERRIDES {
        let overridden = |asked: &str| (asked == name).then(|| OsString::from("/elsewhere"));
        let custom = MuseNative::resolve_from(home, &overridden).unwrap();
        assert_eq!(
            custom.preflight_in(&none),
            Err(CUSTOM_HOME.into()),
            "{name}"
        );
    }

    let disposable = |asked: &str| {
        matches!(asked, "ON_N_OFF_HOME" | "XDG_CONFIG_HOME").then(|| OsString::from("/qa"))
    };
    let qa = MuseNative::resolve_from(home, &disposable).unwrap();
    assert!(!qa.custom);
    assert!(!qa.keychain);
}

#[test]
fn sign_in_runs_the_official_login_in_its_own_file_backed_home_and_presses_enter() {
    let dir = Path::new("/tmp/muse-sign-in");
    let store = MuseNative::private(dir);
    let command = store.sign_in();
    let env: std::collections::HashMap<_, _> = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();

    assert_eq!(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["login"]
    );
    assert_eq!(env["HOME"].as_deref(), Some("/tmp/muse-sign-in"));
    assert_eq!(
        env["XDG_CONFIG_HOME"].as_deref(),
        Some("/tmp/muse-sign-in/config")
    );
    assert_eq!(
        env["XDG_DATA_HOME"].as_deref(),
        Some("/tmp/muse-sign-in/data")
    );
    assert_eq!(env["TBH_CREDENTIAL_BACKEND"].as_deref(), Some("file"));
    assert_eq!(env["MUSE_AUTH_PATH"], None);
    assert_eq!(store.sign_in_input(), Some(&b"\n"[..]));
}

#[test]
fn an_api_key_login_is_not_a_subscription() {
    let (dir, store) = file_store("muse-api-key-login");
    let mut api_key = login("you@example.com", "t1");
    api_key.account["mechanism"] = json!("api_key");
    store.write(Some(&api_key)).unwrap();

    assert_eq!(store.subscription().unwrap(), None);
    store.write(Some(&login("you@example.com", "t1"))).unwrap();
    assert_eq!(
        store.subscription().unwrap(),
        Some(crate::limits::muse_identity("you@example.com"))
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_home_reads_its_usage_with_its_own_token() {
    let (dir, mut store) = file_store("muse-home-usage");
    let answer = json!({
        "subs_tier_name": "Muse Code Power Usage",
        "user_email": "you@example.com",
        "subs_usage": {"weekly": {"used_percent": 22.0, "resets_at": 1_787_500_000.0}},
    });
    let (url, served) = serve_once("200 OK", &answer.to_string());
    store.key_url = leaked(url);
    store.write(Some(&login("you@example.com", "t1"))).unwrap();
    let home = MuseHome {
        dir: dir.clone(),
        store,
    };

    let card = home
        .read_usage(&crate::limits::muse_identity("you@example.com"))
        .unwrap();
    let request = served.join().unwrap();

    assert!(
        request.contains("authorization: Bearer t1")
            || request.contains("Authorization: Bearer t1")
    );
    assert_eq!(
        card.account.unwrap().id,
        crate::limits::muse_identity("you@example.com").observation_key()
    );
    home.delete(Box::new(())).unwrap();
    assert!(!dir.exists());
}

#[test]
fn verification_accepts_its_own_account_and_refuses_another() {
    let (dir, mut store) = file_store("muse-verify");
    store.write(Some(&login("you@example.com", "t1"))).unwrap();
    let mint = |email: &str| {
        json!({"user_email": email, "subs_tier_name": "Muse Code Power Usage"}).to_string()
    };

    let (own_url, own) = serve_once("200 OK", &mint("you@example.com"));
    store.key_url = leaked(own_url);
    let own_verdict = store.verify();
    own.join().unwrap();
    let (other_url, other) = serve_once("200 OK", &mint("someone@example.com"));
    store.key_url = leaked(other_url);
    let other_verdict = store.verify();
    other.join().unwrap();
    let (rejected_url, rejected) = serve_once("401 Unauthorized", "{}");
    store.key_url = leaked(rejected_url);
    let rejected_verdict = store.verify();
    rejected.join().unwrap();

    assert!(own_verdict.is_ok());
    assert!(other_verdict.is_err());
    assert!(rejected_verdict.is_err());
    let _ = fs::remove_dir_all(dir);
}

#[cfg(target_os = "macos")]
mod keychain {
    use super::*;
    use crate::accounts::keychain::{fake_items, with_test_runner};

    fn native(dir: &Path) -> MuseNative {
        MuseNative {
            config: dir.join("muse"),
            private_home: None,
            custom: false,
            keychain: true,
            key_url: SavedReadUrls::LIVE.muse,
        }
    }

    fn written_bundle(sent: &[String]) -> Value {
        let line = sent
            .iter()
            .find(|command| command.starts_with("add-generic-password"))
            .expect("a Keychain write");
        let hex = line
            .split("-X \"")
            .nth(1)
            .unwrap()
            .trim_end()
            .trim_end_matches('"');
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
            .collect();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn activation_writes_the_secret_to_the_keychain_item_and_only_metadata_to_auth_json() {
        let dir = scratch_dir("muse-keychain-write");
        let store = native(&dir);
        fs::create_dir_all(&store.config).unwrap();
        fs::write(
            store.auth_file(),
            r#"{"schema_version":1,"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"old@example.com"}}}"#,
        )
        .unwrap();

        let (written, sent) = with_test_runner(
            fake_items(&[(
                "meta",
                r#"{"secret_schema_version":3,"api_key":"old-key","access_token":"old"}"#,
            )]),
            || store.write(Some(&login("you@example.com", "t1"))),
        );
        written.unwrap();

        assert!(sent
            .iter()
            .all(|command| command.contains("ai.meta.dev.credentials")));
        assert_eq!(
            written_bundle(&sent),
            json!({"secret_schema_version": 3, "api_key": "key-1", "access_token": "t1"})
        );
        let document: Value =
            serde_json::from_slice(&fs::read(store.auth_file()).unwrap()).unwrap();
        let meta = &document["providers"]["meta"];
        assert_eq!(meta["storage"], "keychain");
        assert_eq!(meta["user_email"], "you@example.com");
        assert!(meta.get("access_token").is_none() && meta.get("api_key").is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_signed_in_login_is_read_from_the_keychain_item_its_metadata_names() {
        let dir = scratch_dir("muse-keychain-read");
        let store = native(&dir);
        fs::create_dir_all(&store.config).unwrap();
        fs::write(
            store.auth_file(),
            r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"you@example.com"}}}"#,
        )
        .unwrap();

        let (read, _) = with_test_runner(
            fake_items(&[(
                "meta",
                r#"{"secret_schema_version":3,"api_key":"key-1","access_token":"t1"}"#,
            )]),
            || store.read(),
        );
        let (missing, _) = with_test_runner(fake_items(&[]), || store.read());

        let login = read.unwrap().unwrap();
        assert_eq!(
            login.auth,
            json!({"access_token": "t1", "api_key": "key-1", "secret_schema_version": 3})
        );
        assert_eq!(login.account["user_email"], "you@example.com");
        assert!(login.account.get("storage").is_none());
        assert!(missing.unwrap().is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn clearing_the_signed_in_login_deletes_the_item_and_the_metadata() {
        let dir = scratch_dir("muse-keychain-clear");
        let store = native(&dir);
        fs::create_dir_all(&store.config).unwrap();
        fs::write(
            store.auth_file(),
            r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain"},"anthropic":{}}}"#,
        )
        .unwrap();

        let (cleared, sent) = with_test_runner(fake_items(&[]), || store.write(None));
        cleared.unwrap();

        assert!(sent
            .iter()
            .any(|command| command.starts_with("delete-generic-password")));
        let document: Value =
            serde_json::from_slice(&fs::read(store.auth_file()).unwrap()).unwrap();
        assert!(document["providers"].get("meta").is_none());
        assert!(document["providers"].get("anthropic").is_some());
        let _ = fs::remove_dir_all(dir);
    }
}
