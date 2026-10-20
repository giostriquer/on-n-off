use super::super::Adapter;
use super::*;
use crate::http::serve_once;
use crate::paths::scratch_dir;
use serde_json::json;

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

fn api_key_login(mechanism: &str) -> Login {
    Login {
        auth: json!({"api_key": "k"}),
        account: json!({"mechanism": mechanism, "user_email": "you@example.com"}),
    }
}

fn you() -> Identity {
    muse_store::identity("you@example.com").unwrap()
}

fn urls(url: String) -> SavedReadUrls<'static> {
    SavedReadUrls {
        muse: Box::leak(url.into_boxed_str()),
        ..SavedReadUrls::LIVE
    }
}

fn mint(email: &str) -> String {
    json!({
        "subs_tier_name": "Muse Code Power Usage",
        "user_email": email,
        "subs_usage": {"weekly": {"used_percent": 22.0, "resets_at": 1_787_500_000.0}},
    })
    .to_string()
}

fn env_of(command: &Command) -> std::collections::HashMap<String, Option<OsString>> {
    command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(OsString::from),
            )
        })
        .collect()
}

#[test]
fn a_muse_login_is_identified_by_its_lowercased_email_and_never_by_an_api_key() {
    let saved = login("You@Example.com", "t1");
    assert_eq!(MuseView::of(&saved).identity().unwrap(), you());

    for mechanism in ["api_key", "oauth"] {
        assert_eq!(
            MuseView::of(&api_key_login(mechanism)).identity(),
            Err("API key logins cannot be saved as subscription profiles.".into()),
            "{mechanism}"
        );
    }
    let mut mislabelled = login("you@example.com", "t1");
    mislabelled.account["mechanism"] = json!("api_key");
    assert_eq!(MuseView::of(&mislabelled).identity().unwrap(), you());

    let mut nameless = login("", "t1");
    nameless
        .account
        .as_object_mut()
        .unwrap()
        .remove("user_email");
    assert!(MuseView::of(&nameless).identity().is_err());
    let mut tokenless = login("you@example.com", "");
    tokenless.auth = json!({});
    assert_eq!(
        MuseView::of(&tokenless).identity(),
        Err(model::MISSING_CREDENTIALS.into())
    );
    assert!(MuseView::of(&login("you@example.com", ""))
        .identity()
        .is_err());
}

#[test]
fn a_new_token_is_a_new_login_generation_that_is_never_renewed() {
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
    assert!(Muse
        .renew_private(&login("you@example.com", "t1"), 0, "")
        .is_err());
}

#[test]
fn a_saved_login_round_trips_through_its_home_and_leaves_with_it() {
    let dir = scratch_dir("muse-home-round-trip");
    let home = Muse.homes().expect("Muse saved logins live in homes")(&dir);
    let saved = login("you@example.com", "t1");

    let back = home.put(&saved, &()).unwrap().unwrap();

    assert_eq!(back.auth, saved.auth);
    assert_eq!(back.account, saved.account);
    let reread = home.read().unwrap().unwrap();
    assert_eq!(
        (reread.auth, reread.account),
        (saved.auth.clone(), saved.account.clone())
    );
    assert_eq!(home.identify(&back).unwrap(), you());
    let file: Value = serde_json::from_str(
        &fs::read_to_string(dir.join("config").join("muse").join("auth.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(file["providers"]["meta"]["access_token"], "t1");
    assert_eq!(file["schema_version"], 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.join("config").join("muse").join("auth.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    home.clear(&()).unwrap();
    assert!(home.read().unwrap().is_none());
    home.delete(Box::new(())).unwrap();
    assert!(!dir.exists());
    home.delete(Box::new(())).unwrap();
}

#[test]
fn the_signed_in_login_is_only_activated_from_the_default_home_without_an_api_key() {
    let home = scratch_dir("muse-native-home");
    let none = |_: &str| None;
    let native = MuseNative::resolve_from(&home, &none).unwrap();
    assert!(native.preflight_in(&none).is_ok());
    assert!(native
        .preflight_in(&|name: &str| (name == "META_API_KEY").then(|| OsString::from("k")))
        .is_err());

    for name in OVERRIDES {
        let overridden = |asked: &str| (asked == name).then(|| OsString::from("/elsewhere"));
        let custom = MuseNative::resolve_from(&home, &overridden).unwrap();
        assert_eq!(
            custom.preflight_in(&none),
            Err(CUSTOM_HOME.into()),
            "{name}"
        );
    }

    let disposable = |asked: &str| (asked == "ON_N_OFF_HOME").then(|| OsString::from("/qa"));
    let qa = MuseNative::resolve_from(&home, &disposable).unwrap();
    assert!(!qa.custom);
    assert!(!qa.keychain);
}

#[cfg(unix)]
#[test]
fn a_linked_login_file_is_refused_before_activation_starts() {
    let dir = scratch_dir("muse-linked-preflight");
    let native = MuseNative {
        custom: false,
        ..MuseNative::private(&dir)
    };
    fs::create_dir_all(&native.config).unwrap();
    let target = dir.join("elsewhere.json");
    fs::write(&target, "{}").unwrap();
    std::os::unix::fs::symlink(&target, native.auth_file()).unwrap();

    let refused = native.preflight_in(&|_: &str| None).unwrap_err();

    assert!(refused.contains("Linked"), "{refused}");
}

#[test]
fn sign_in_runs_the_official_login_in_its_own_file_backed_home() {
    let dir = scratch_dir("muse-sign-in");
    let store = MuseNative::private(&dir);
    let command = store.sign_in();
    let env = env_of(&command);

    assert_eq!(
        command
            .get_args()
            .map(|arg| arg.to_os_string())
            .collect::<Vec<_>>(),
        [OsString::from("login")]
    );
    assert_eq!(env["HOME"], Some(dir.clone().into_os_string()));
    assert_eq!(
        env["XDG_CONFIG_HOME"],
        Some(dir.join("config").into_os_string())
    );
    assert_eq!(
        env["XDG_DATA_HOME"],
        Some(dir.join("data").into_os_string())
    );
    assert_eq!(env["TBH_CREDENTIAL_BACKEND"], Some(OsString::from("file")));
    assert_eq!(env["MUSE_AUTH_PATH"], None);
}

#[test]
fn the_sign_in_page_is_the_meta_device_page_muse_prints() {
    let store = MuseNative::private(&scratch_dir("muse-sign-in-page"));
    let page = "https://auth.meta.com/oauth/device/?code=ABCD-1234";

    assert_eq!(store.sign_in_page(&format!("  {page}")), Some(page));
    for printed in [
        "Open this page to sign in:",
        "  ABCD-1234",
        "http://auth.meta.com/oauth/device/?code=ABCD-1234",
        "https://auth.meta.com.example/oauth/device/?code=ABCD-1234",
        "https://example.com/?next=https://auth.meta.com/oauth/device/",
        "https://auth.meta.com/oauth/device/ ?code=ABCD-1234",
        "Waiting for approval…",
    ] {
        assert_eq!(store.sign_in_page(printed), None, "{printed}");
    }
}

#[test]
fn sign_out_runs_the_official_logout_against_the_signed_in_home() {
    let native = MuseNative::resolve_from(&scratch_dir("muse-logout"), &|_: &str| None).unwrap();
    let command = native.logout_command();
    let env = env_of(&command);

    assert_eq!(
        command
            .get_args()
            .map(|arg| arg.to_os_string())
            .collect::<Vec<_>>(),
        [OsString::from("logout")]
    );
    for name in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "TBH_CREDENTIAL_BACKEND",
    ] {
        assert!(!env.contains_key(name), "{name}");
    }
}

#[test]
fn an_api_key_login_is_not_a_subscription() {
    let dir = scratch_dir("muse-api-key-login");
    let store = MuseNative::private(&dir);
    store.write(Some(&api_key_login("oauth"))).unwrap();

    assert_eq!(store.subscription().unwrap(), None);
    let mut mislabelled = login("You@Example.com", "t1");
    mislabelled.account["mechanism"] = json!("api_key");
    store.write(Some(&mislabelled)).unwrap();
    assert_eq!(store.subscription().unwrap(), Some(you()));
    store.write(Some(&login("You@Example.com", "t1"))).unwrap();
    assert_eq!(store.subscription().unwrap(), Some(you()));
}

fn switch_to_a_saved_login(native: &MuseNative) -> Result<(), String> {
    let mut db = super::super::store::Database::default();
    let id = db
        .save(you(), login("you@example.com", "t1"), None)
        .unwrap();
    super::super::transaction::activate(&mut db, native, &id, false, &mut |_| Ok(()))
}

#[test]
fn a_signed_in_api_key_login_blocks_a_switch_and_is_left_alone() {
    let dir = scratch_dir("muse-api-key-switch");
    let native = MuseNative {
        urls: urls("http://127.0.0.1:9/".into()),
        ..MuseNative::private(&dir)
    };
    fs::create_dir_all(&native.config).unwrap();
    let api_key = r#"{"providers":{"meta":{"mechanism":"api_key","api_key":"k","user_email":"you@example.com"}}}"#;
    fs::write(native.auth_file(), api_key).unwrap();

    assert!(native.read().unwrap().is_some());
    assert_eq!(
        switch_to_a_saved_login(&native),
        Err("API key logins cannot be saved as subscription profiles.".into())
    );
    assert_eq!(fs::read_to_string(native.auth_file()).unwrap(), api_key);
}

#[test]
fn a_malformed_saved_login_is_refused_and_the_signed_in_one_left_alone() {
    let dir = scratch_dir("muse-malformed-login");
    let store = MuseNative::private(&dir);
    store.write(Some(&login("you@example.com", "t1"))).unwrap();
    let before = fs::read_to_string(store.auth_file()).unwrap();

    for malformed in [
        Login {
            auth: json!("x"),
            account: login("you@example.com", "t2").account,
        },
        Login {
            auth: login("you@example.com", "t2").auth,
            account: Value::Null,
        },
    ] {
        assert_eq!(store.write(Some(&malformed)), Err(MALFORMED_LOGIN.into()));
    }
    assert_eq!(fs::read_to_string(store.auth_file()).unwrap(), before);
}

#[test]
fn a_homes_login_stamp_follows_its_auth_file_without_reading_it() {
    let dir = scratch_dir("muse-home-stamp");
    let home = MuseHome::at(&dir);
    let empty = home.login_stamp();
    home.put(&login("you@example.com", "t1"), &()).unwrap();
    let file = home.store.auth_file();
    let touched = |seconds: u64| {
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    };
    touched(1_800_000_000);
    let first = home.login_stamp();

    assert_eq!(empty.as_deref(), Ok(native::ABSENT), "no login yet");
    assert_ne!(first, empty);
    assert_eq!(home.login_stamp(), first);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(home.login_stamp(), first, "the stamp never opens the file");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let same_length = fs::read(&file).unwrap();
    fs::write(&file, &same_length).unwrap();
    touched(1_800_000_060);
    assert_ne!(home.login_stamp(), first, "rewritten, the same length");
}

#[cfg(unix)]
#[test]
fn a_home_whose_auth_file_cannot_be_looked_at_has_no_stamp() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch_dir("muse-home-sealed");
    let home = MuseHome::at(&dir);
    home.put(&login("you@example.com", "t1"), &()).unwrap();
    let config = home.store.auth_file().parent().unwrap().to_path_buf();

    fs::set_permissions(&config, fs::Permissions::from_mode(0o000)).unwrap();
    let stamp = home.login_stamp();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(stamp.is_err(), "{stamp:?}");
}

#[test]
fn a_home_and_a_new_sign_in_read_usage_with_their_own_token() {
    let dir = scratch_dir("muse-home-usage");
    let (home_url, home_served) = serve_once("200 OK", &mint("you@example.com"));
    let home = MuseHome {
        dir: dir.clone(),
        store: MuseNative {
            urls: urls(home_url),
            ..MuseNative::private(&dir)
        },
    };
    home.put(&login("you@example.com", "t1"), &()).unwrap();

    let card = home.read_usage(&you()).unwrap().into_card();
    let request = home_served.join().unwrap();
    let (first_url, first_served) = serve_once("200 OK", &mint("you@example.com"));
    let isolated = MuseNative {
        urls: urls(first_url),
        ..MuseNative::private(&dir)
    };
    let first = isolated.first_usage(&dir, &you()).unwrap().into_card();
    first_served.join().unwrap();

    assert!(
        request.to_lowercase().contains("authorization: bearer t1"),
        "{request}"
    );
    assert_eq!(card.account.unwrap().id, you().observation_key());
    assert_eq!(first.account.unwrap().id, you().observation_key());
}

#[test]
fn verification_accepts_its_own_account_and_refuses_another_or_a_rejected_token() {
    let dir = scratch_dir("muse-verify");
    MuseNative::private(&dir)
        .write(Some(&login("you@example.com", "t1")))
        .unwrap();
    let verdict = |status: &str, body: String| {
        let (url, served) = serve_once(status, &body);
        let store = MuseNative {
            urls: urls(url),
            ..MuseNative::private(&dir)
        };
        let verdict = store.verify();
        served.join().unwrap();
        verdict
    };

    let verified = |failure| crate::limits::outcome::verified(AgentId::Muse, Some(&failure));
    assert_eq!(verdict("200 OK", mint("You@Example.com")), Ok(()));
    assert_eq!(
        verdict("200 OK", mint("someone@example.com")),
        verified(ReadFailure::OtherAccount)
    );
    assert_eq!(
        verdict("401 Unauthorized", "{}".into()),
        verified(ReadFailure::LoginRefused)
    );
    assert!(verified(ReadFailure::LoginRefused).is_err());
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
            urls: SavedReadUrls::LIVE,
        }
    }

    #[test]
    fn a_keychain_login_reads_back_exactly_as_it_was_written() {
        let dir = scratch_dir("muse-keychain-round-trip");
        let store = native(&dir);
        fs::create_dir_all(&store.config).unwrap();
        fs::write(
            store.auth_file(),
            r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"you@example.com"}}}"#,
        )
        .unwrap();
        let saved = login("you@example.com", "t1");

        let (read, _) = with_test_runner(
            fake_items(&[(
                "meta",
                r#"{"secret_schema_version":3,"api_key":"key-1","access_token":"t1"}"#,
            )]),
            || store.read(),
        );

        assert_eq!(read.unwrap().unwrap().auth, saved.auth);
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

        let write = sent
            .iter()
            .find(|command| command.starts_with("add-generic-password"))
            .expect("a Keychain write");
        assert!(write.contains("ai.meta.dev.credentials"));
        let hex = write
            .split("-X \"")
            .nth(1)
            .unwrap()
            .trim_end()
            .trim_end_matches('"');
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
            .collect();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({"secret_schema_version": 3, "api_key": "key-1", "access_token": "t1"})
        );
        let document: Value =
            serde_json::from_slice(&fs::read(store.auth_file()).unwrap()).unwrap();
        let meta = &document["providers"]["meta"];
        assert_eq!(meta["storage"], "keychain");
        assert_eq!(meta["user_email"], "you@example.com");
        assert!(meta.get("access_token").is_none() && meta.get("api_key").is_none());
    }

    #[test]
    fn an_api_key_login_kept_in_the_keychain_blocks_a_switch_and_is_left_alone() {
        let dir = scratch_dir("muse-keychain-api-key");
        let store = native(&dir);
        fs::create_dir_all(&store.config).unwrap();
        let api_key = r#"{"providers":{"meta":{"mechanism":"api_key","storage":"keychain","user_email":"you@example.com"}}}"#;
        fs::write(store.auth_file(), api_key).unwrap();

        let ((read, switched), sent) =
            with_test_runner(fake_items(&[("meta", r#"{"api_key":"k"}"#)]), || {
                (store.read(), switch_to_a_saved_login(&store))
            });

        assert_eq!(read.unwrap().unwrap().auth, json!({"api_key": "k"}));
        assert_eq!(
            switched,
            Err("API key logins cannot be saved as subscription profiles.".into())
        );
        assert_eq!(fs::read_to_string(store.auth_file()).unwrap(), api_key);
        assert!(!sent
            .iter()
            .any(|command| command.starts_with("add-generic-password")));
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

        let (cleared, sent) = with_test_runner(
            fake_items(&[("meta", r#"{"access_token":"t1","api_key":"key-1"}"#)]),
            || store.write(None),
        );
        cleared.unwrap();

        assert!(sent.iter().any(|command| {
            command.starts_with("delete-generic-password")
                && command.contains("ai.meta.dev.credentials")
        }));
        let document: Value =
            serde_json::from_slice(&fs::read(store.auth_file()).unwrap()).unwrap();
        assert!(document["providers"].get("meta").is_none());
        assert!(document["providers"].get("anthropic").is_some());
    }

    #[test]
    fn a_malformed_keychain_item_is_refused_and_never_overwritten() {
        let dir = scratch_dir("muse-keychain-malformed");
        let store = native(&dir);
        fs::create_dir_all(&store.config).unwrap();
        fs::write(
            store.auth_file(),
            r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"you@example.com"}}}"#,
        )
        .unwrap();

        let ((read, write), sent) = with_test_runner(fake_items(&[("meta", "not json")]), || {
            (
                store.read(),
                store.write(Some(&login("you@example.com", "t1"))),
            )
        });

        assert!(read.is_err());
        assert!(write.is_err());
        assert!(!sent
            .iter()
            .any(|command| command.starts_with("add-generic-password")));
    }
}
