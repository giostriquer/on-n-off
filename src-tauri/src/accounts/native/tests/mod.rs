use super::*;
fn claude(root: &Path) -> NativeStore {
    let home = root.join(".claude");
    fs::create_dir_all(&home).unwrap();
    NativeStore {
        provider: AgentId::Claude,
        config_home: home,
        config_file: root.join(".claude.json"),
        custom: false,
        use_keychain: false,
        secure_storage: None,
    }
}
#[test]
fn switching_claude_preserves_current_mcp_oauth_and_unrelated_configuration() {
    let root = tempfile::tempdir().unwrap();
    let native = claude(root.path());
    fs::write(native.config_home.join(".credentials.json"),r#"{"claudeAiOauth":{"accessToken":"old","refreshToken":"a"},"mcpOAuth":{"server":"current-server-token"}}"#).unwrap();
    fs::write(&native.config_file,r#"{"oauthAccount":{"accountUuid":"a","organizationUuid":"org-a"},"mcpServers":{"custom":{"command":"existing"}},"theme":"dark"}"#).unwrap();
    let incoming = Login {
        auth: json!({"claudeAiOauth":{"accessToken":"incoming","refreshToken":"b"},"mcpOAuth":{"server":"stale-snapshot-token"}}),
        account: json!({"accountUuid":"b","organizationUuid":"org-b"}),
    };
    native.write(Some(&incoming)).unwrap();
    let current = native.read().unwrap().unwrap();
    assert_eq!(current.auth["claudeAiOauth"]["refreshToken"], "b");
    assert_eq!(
        read_json(&native.config_home.join(".credentials.json")).unwrap()["mcpOAuth"]["server"],
        "current-server-token"
    );
    assert!(current.auth.get("mcpOAuth").is_none());
    assert_eq!(current.account["organizationUuid"], "org-b");
    let config = read_json(&native.config_file).unwrap();
    assert_eq!(config["theme"], "dark");
    assert_eq!(config["mcpServers"]["custom"]["command"], "existing");
    assert!(!native.config_home.join(".oauth_refresh.lock").exists());
}
#[test]
fn native_refresh_lock_prevents_either_half_of_claude_activation() {
    let root = tempfile::tempdir().unwrap();
    let native = claude(root.path());
    fs::create_dir(native.config_home.join(".oauth_refresh.lock")).unwrap();
    let incoming = Login {
        auth: json!({"claudeAiOauth":{"accessToken":"incoming","refreshToken":"b"}}),
        account: json!({"accountUuid":"b","organizationUuid":"org-b"}),
    };
    assert!(native.write(Some(&incoming)).is_err());
    assert!(!native.config_file.exists());
    assert!(!native.config_home.join(".credentials.json").exists());
}
#[test]
fn scoped_codex_metadata_keeps_same_workspace_users_separate() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let root = tempfile::tempdir().unwrap();
    let save = |user: &str| {
        let claims = json!({"https://api.openai.com/auth":{"chatgpt_account_id":"team","chatgpt_user_id":user}});
        fs::write(root.path().join("auth.json"),json!({"tokens":{"account_id":"team","id_token":format!("e30.{}.s",URL_SAFE_NO_PAD.encode(claims.to_string()))}}).to_string()).unwrap();
    };
    save("a");
    let a = codex_metadata(root.path()).unwrap().unwrap().0;
    save("b");
    let b = codex_metadata(root.path()).unwrap().unwrap().0;
    assert_ne!(a, b);
    assert!(a.starts_with("profile:"));
    fs::write(
        root.path().join("config.toml"),
        "cli_auth_credentials_store = 'ephemeral'",
    )
    .unwrap();
    assert!(codex_metadata(root.path()).is_err());
}

#[test]
fn disposable_claude_commands_never_use_the_default_native_home() {
    let root = tempfile::tempdir().unwrap();
    let native = claude(root.path());
    let command = native.command();
    let env: std::collections::HashMap<_, _> = command.get_envs().collect();
    assert_eq!(
        env.get(std::ffi::OsStr::new("CLAUDE_CONFIG_DIR")),
        Some(&Some(native.config_home.as_os_str()))
    );
    assert_eq!(
        env.get(std::ffi::OsStr::new("HOME")),
        Some(&Some(root.path().as_os_str()))
    );
}

#[cfg(target_os = "macos")]
#[test]
fn isolated_claude_sign_in_keeps_the_os_home_for_keychain_lookup() {
    let root = tempfile::tempdir().unwrap();
    let native = NativeStore::isolated(AgentId::Claude, root.path()).unwrap();
    let command = native.command();
    let env: std::collections::HashMap<_, _> = command.get_envs().collect();
    assert_eq!(
        env.get(std::ffi::OsStr::new("CLAUDE_CONFIG_DIR")),
        Some(&Some(native.config_home.as_os_str()))
    );
    assert!(
        !env.contains_key(std::ffi::OsStr::new("HOME")),
        "Redirecting HOME makes macOS lose the login Keychain"
    );
    assert!(!env.contains_key(std::ffi::OsStr::new("USERPROFILE")));
    assert_eq!(native.config_file, native.config_home.join(".claude.json"));
    assert_ne!(native.claude_service(), "Claude Code-credentials");
}

#[test]
fn legacy_identity_is_canonical_even_when_the_other_config_disagrees() {
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    native.config_file = native.config_home.join(".config.json");
    fs::write(
        &native.config_file,
        r#"{"oauthAccount":{"accountUuid":"legacy-user","organizationUuid":"team"}}"#,
    )
    .unwrap();
    fs::write(
        native.config_home.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"test-token","refreshToken":"test-refresh"}}"#,
    )
    .unwrap();
    for alternate in [
        None,
        Some(r#"{"oauthAccount":{"accountUuid":"stale-other-user","organizationUuid":"other"}}"#),
    ] {
        if let Some(text) = alternate {
            fs::write(root.path().join(".claude.json"), text).unwrap();
        }
        let identity_file = claude_store::native_dirs(root.path())
            .unwrap()
            .config_file(root.path());
        let identity = crate::limits::credentials::read_claude_identity(&identity_file).unwrap();
        assert_eq!(identity.account.id, "legacy-user");
        let (url, request) = crate::http::serve_once(
            "200 OK",
            r#"{"account":{"uuid":"legacy-user"},"organization":{"uuid":"team"}}"#,
        );
        native.verify_claude(&url).unwrap();
        request.join().unwrap();
        let (url, request) = crate::http::serve_once(
            "200 OK",
            r#"{"account":{"uuid":"wrong-user"},"organization":{"uuid":"team"}}"#,
        );
        assert!(native.verify_claude(&url).is_err());
        request.join().unwrap();
    }
}

/// An environment holding exactly `vars`, for `resolve_from`.
fn environment<'a>(
    vars: &'a [(&'a str, PathBuf)],
) -> impl Fn(&str) -> Option<std::ffi::OsString> + 'a {
    move |name| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.clone().into_os_string())
    }
}

/// Whatever the developer running the suite has exported, a store resolved in a test lives in
/// the test's own home and never chooses the login Keychain: `resolve` reads the test
/// environment, not `CLAUDE_CONFIG_DIR`, `CODEX_HOME` or whatever a sibling test did to
/// `ON_N_OFF_HOME`.
#[test]
fn a_test_resolves_native_stores_inside_its_own_home_whatever_the_machine_exports() {
    let root = tempfile::tempdir().unwrap();
    for (provider, folder) in [(AgentId::Claude, ".claude"), (AgentId::Codex, ".codex")] {
        let store = NativeStore::resolve(provider, root.path()).unwrap();
        assert_eq!(store.config_home, root.path().join(folder), "{provider:?}");
        assert!(!store.custom, "{provider:?}");
        assert!(
            !store.use_keychain,
            "{provider:?} would reach the login Keychain"
        );
    }
}

#[test]
fn outside_a_disposable_home_the_provider_override_and_the_keychain_apply() {
    let root = tempfile::tempdir().unwrap();
    let claude_home = root.path().join("elsewhere").join("claude");
    let env = [("CLAUDE_CONFIG_DIR", claude_home.clone())];
    let store =
        NativeStore::resolve_from(AgentId::Claude, root.path(), &environment(&env)).unwrap();
    assert_eq!(store.config_home, claude_home);
    assert_eq!(store.config_file, claude_home.join(".claude.json"));
    assert!(store.custom);
    assert!(store.use_keychain);

    let codex_home = root.path().join("elsewhere").join("codex");
    let env = [("CODEX_HOME", codex_home.clone())];
    let store = NativeStore::resolve_from(AgentId::Codex, root.path(), &environment(&env)).unwrap();
    assert_eq!(store.config_home, codex_home);
    assert_eq!(store.config_file, codex_home.join("config.toml"));
    assert!(store.custom);
    assert!(store.use_keychain);

    let store = NativeStore::resolve_from(AgentId::Claude, root.path(), &environment(&[])).unwrap();
    assert_eq!(store.config_home, root.path().join(".claude"));
    assert_eq!(store.config_file, root.path().join(".claude.json"));
    assert!(!store.custom);
    assert!(store.use_keychain);
}

/// Claude's config home is `CLAUDE_CONFIG_DIR` as Claude Code reads it: NFC-normalized, never
/// trimmed.
#[test]
fn the_claude_config_home_is_claude_config_dir_as_claude_code_reads_it() {
    let root = tempfile::tempdir().unwrap();
    let mut padded = root.path().join("claude").into_os_string();
    padded.push(" ");
    for (value, config) in [
        (
            root.path().join("cafe\u{301}"),
            root.path().join("caf\u{e9}"),
        ),
        (PathBuf::from(&padded), PathBuf::from(&padded)),
    ] {
        let env = [("CLAUDE_CONFIG_DIR", value.clone())];
        let store =
            NativeStore::resolve_from(AgentId::Claude, root.path(), &environment(&env)).unwrap();
        assert_eq!(store.config_home, config, "{value:?}");
        assert_eq!(store.config_file, config.join(".claude.json"));
    }
}

#[test]
fn a_disposable_home_ignores_provider_overrides_and_the_keychain() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = root.path().join("elsewhere");
    let env = [
        ("ON_N_OFF_HOME", root.path().to_path_buf()),
        ("CLAUDE_CONFIG_DIR", elsewhere.clone()),
        ("CODEX_HOME", elsewhere),
    ];
    for (provider, folder) in [(AgentId::Claude, ".claude"), (AgentId::Codex, ".codex")] {
        let store = NativeStore::resolve_from(provider, root.path(), &environment(&env)).unwrap();
        assert_eq!(store.config_home, root.path().join(folder), "{provider:?}");
        assert!(!store.custom, "{provider:?}");
        assert!(!store.use_keychain, "{provider:?}");
    }
}

#[test]
fn a_relative_provider_override_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let env = [("CODEX_HOME", Path::new("relative").join("codex"))];
    assert_eq!(
        NativeStore::resolve_from(AgentId::Codex, root.path(), &environment(&env)).err(),
        Some("The provider home must be an absolute path.".to_string())
    );
}

/// A Codex login on disk, as the CLI writes it: its tokens under `tokens`, the workspace's claims in
/// the id token.
fn codex_login(root: &Path, access_token: Option<&str>) {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let claims = json!({"https://api.openai.com/auth":{"chatgpt_account_id":"team","chatgpt_user_id":"user"}});
    let mut tokens = json!({
        "account_id": "team",
        "refresh_token": "fixture-refresh",
        "id_token": format!("e30.{}.s", URL_SAFE_NO_PAD.encode(claims.to_string())),
    });
    if let Some(token) = access_token {
        tokens["access_token"] = json!(token);
    }
    fs::write(
        root.join("auth.json"),
        json!({"tokens": tokens}).to_string(),
    )
    .unwrap();
}

/// Only the access token leaves accounts, beside the key the card is known by and its workspace.
#[test]
fn the_access_projection_carries_only_the_access_token_with_the_cards_identity() {
    let root = tempfile::tempdir().unwrap();
    codex_login(root.path(), Some("fixture-access"));

    let (metadata, access) = codex_metadata_and_access(root.path()).unwrap().unwrap();
    let access = access.unwrap();

    assert_eq!(
        Some(&metadata),
        codex_metadata(root.path()).unwrap().as_ref()
    );
    assert_eq!(access.observation_key, metadata.0);
    assert_eq!(access.workspace_id, "team");
    assert_eq!(access.token.authorization(), "Bearer fixture-access");
}

#[test]
fn a_codex_login_without_an_access_token_gives_its_identity_and_no_access() {
    let root = tempfile::tempdir().unwrap();
    codex_login(root.path(), None);

    let (metadata, access) = codex_metadata_and_access(root.path()).unwrap().unwrap();
    assert_eq!(Some(metadata), codex_metadata(root.path()).unwrap());
    assert!(access.is_none());
    assert!(
        codex_metadata_and_access(tempfile::tempdir().unwrap().path())
            .unwrap()
            .is_none()
    );
}

/// Reading the token must not loosen the identity rules `codex_metadata` enforces.
#[test]
fn the_access_projection_refuses_a_login_whose_claims_name_another_workspace() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let root = tempfile::tempdir().unwrap();
    let claims = json!({"https://api.openai.com/auth":{"chatgpt_account_id":"other","chatgpt_user_id":"user"}});
    fs::write(
        root.path().join("auth.json"),
        json!({"tokens":{"account_id":"team","access_token":"fixture-access",
            "id_token":format!("e30.{}.s",URL_SAFE_NO_PAD.encode(claims.to_string()))}})
        .to_string(),
    )
    .unwrap();

    assert!(codex_metadata_and_access(root.path()).is_err());
}

/// Claude Code's own sign-out leaves this behind: valid JSON, no token.
const SIGNED_OUT: &str = r#"{"claudeAiOauth":{"accessToken":"","refreshToken":"","expiresAt":0}}"#;

/// The account switch's read of a store that has no Keychain to consult, for each thing the
/// credentials file can hold. A login needs an access token: Claude Code signs out by emptying
/// `claudeAiOauth`, so an emptied one is no login, as it is for Limits.
#[test]
fn the_native_claude_read_of_the_credentials_file() {
    let root = tempfile::tempdir().unwrap();
    let native = claude(root.path());
    let file = native.config_home.join(".credentials.json");
    assert!(native.read().unwrap().is_none(), "no file");

    fs::write(
        &file,
        r#"{"claudeAiOauth":{"accessToken":"file-token"},"mcpOAuth":{"s":"t"}}"#,
    )
    .unwrap();
    let login = native.read().unwrap().unwrap();
    assert_eq!(
        login.auth,
        json!({"claudeAiOauth":{"accessToken":"file-token"}}),
        "only the login is read, never the MCP tokens beside it"
    );

    for signed_out in [
        SIGNED_OUT,
        r#"{"claudeAiOauth":{"accessToken":"  ","refreshToken":"r"}}"#,
        r#"{"claudeAiOauth":{"refreshToken":"r"}}"#,
    ] {
        fs::write(&file, signed_out).unwrap();
        assert!(
            native.read().unwrap().is_none(),
            "an emptied login is no login: {signed_out}"
        );
    }

    fs::write(&file, r#"{"mcpOAuth":{"s":"t"}}"#).unwrap();
    assert!(native.read().unwrap().is_none(), "no claudeAiOauth");

    fs::write(&file, "{ not json").unwrap();
    assert_eq!(
        native.read().err().as_deref(),
        Some("The native credential document is malformed. It has not been changed.")
    );
}

/// Claude Code's refresh lock, its legacy lock beside the config home, and the config file's lock,
/// in the order they are taken.
fn native_lock_paths(native: &NativeStore) -> [PathBuf; 3] {
    let mut legacy = native.config_home.as_os_str().to_owned();
    legacy.push(".lock");
    let mut config = native.config_file.as_os_str().to_owned();
    config.push(".lock");
    [
        native.config_home.join(".oauth_refresh.lock"),
        legacy.into(),
        config.into(),
    ]
}

fn backdate(path: &Path, seconds: u64) {
    let then = std::time::SystemTime::now() - Duration::from_secs(seconds);
    filetime::set_file_mtime(path, filetime::FileTime::from_system_time(then)).unwrap();
}

/// Polls `done` for up to ten seconds, well past two heartbeats.
fn eventually(what: &str, done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

const BUSY: &str = "Claude is updating its login or configuration. Retry after it finishes.";

#[test]
fn any_one_held_native_lock_refuses_and_leaves_none_of_the_others_taken() {
    for held in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let native = claude(root.path());
        let paths = native_lock_paths(&native);
        fs::create_dir(&paths[held]).unwrap();

        assert_eq!(native.lock().err().as_deref(), Some(BUSY), "lock {held}");
        for (at, path) in paths.iter().enumerate() {
            assert_eq!(path.exists(), at == held, "lock {held} held: {path:?}");
        }
    }
}

/// Claude Code abandons its refresh locks after a minute and the config file's after ten seconds.
#[test]
fn a_native_lock_left_behind_is_broken_only_once_stale_for_its_kind() {
    for (held, age, broken) in [
        (0, 30, false),
        (0, 61, true),
        (1, 30, false),
        (1, 61, true),
        (2, 5, false),
        (2, 11, true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let native = claude(root.path());
        let paths = native_lock_paths(&native);
        fs::create_dir(&paths[held]).unwrap();
        backdate(&paths[held], age);

        let lock = native.lock();
        assert_eq!(lock.is_ok(), broken, "lock {held}, {age}s old");
    }
}

/// While held, every lock is touched often enough that Claude Code never judges it abandoned.
#[test]
fn held_native_locks_are_kept_fresh_and_released_on_drop() {
    let root = tempfile::tempdir().unwrap();
    let native = claude(root.path());
    let guard = native.lock().unwrap();
    let paths = native_lock_paths(&native);
    for path in &paths {
        backdate(path, 3600);
    }
    let fresh = |path: &Path| {
        fs::metadata(path)
            .and_then(|meta| meta.modified())
            .is_ok_and(|at| at.elapsed().unwrap_or_default() < Duration::from_secs(60))
    };
    eventually("the heartbeat refreshes every held lock", || {
        paths.iter().all(|path| fresh(path))
    });

    drop(guard);
    assert!(paths.iter().all(|path| !path.exists()));
}

/// A lock another process broke means the write is no longer coordinated with Claude Code, so it
/// must not happen.
#[test]
fn a_native_lock_broken_under_the_holder_stops_the_write() {
    let root = tempfile::tempdir().unwrap();
    let native = claude(root.path());
    let credentials = native.config_home.join(".credentials.json");
    fs::write(&credentials, r#"{"claudeAiOauth":{"accessToken":"old"}}"#).unwrap();
    let guard = native.lock().unwrap();
    fs::remove_dir(&native_lock_paths(&native)[0]).unwrap();
    eventually("the heartbeat notices the lost lock", || {
        guard.ensure().is_err()
    });

    let incoming = Login {
        auth: json!({"claudeAiOauth":{"accessToken":"incoming"}}),
        account: json!({"accountUuid":"b","organizationUuid":"org-b"}),
    };
    assert_eq!(
        native.write_locked(Some(&incoming), guard).err().as_deref(),
        Some("Native credential coordination was lost. Protected recovery has been retained.")
    );
    assert_eq!(
        fs::read_to_string(&credentials).unwrap(),
        r#"{"claudeAiOauth":{"accessToken":"old"}}"#
    );
    assert!(!native.config_file.exists());
}

fn incoming() -> Login {
    Login {
        auth: json!({"claudeAiOauth":{"accessToken":"incoming"}}),
        account: json!({"accountUuid":"b","organizationUuid":"org-b"}),
    }
}

/// Claude Code takes `.storage-write.lock` around every change to its credentials. While another
/// process holds it, the switch writes neither half.
#[test]
fn a_switch_yields_while_claude_code_writes_its_credentials() {
    let root = tempfile::tempdir().unwrap();
    let native = claude(root.path());
    let credentials = native.config_home.join(".credentials.json");
    fs::write(&credentials, r#"{"claudeAiOauth":{"accessToken":"old"}}"#).unwrap();
    let lock = native.config_home.join(".storage-write.lock");
    fs::create_dir(&lock).unwrap();

    assert_eq!(native.write(Some(&incoming())).err().as_deref(), Some(BUSY));
    assert_eq!(
        fs::read_to_string(&credentials).unwrap(),
        r#"{"claudeAiOauth":{"accessToken":"old"}}"#
    );
    assert!(!native.config_file.exists());
    assert!(lock.is_dir());

    fs::remove_dir(&lock).unwrap();
    native.write(Some(&incoming())).unwrap();
    assert!(!lock.exists(), "released once the write is done");
}

/// A credentials file that is a link is refused before either half of the switch is written.
#[cfg(unix)]
#[test]
fn a_switch_refuses_a_linked_credentials_file_before_writing_anything() {
    let root = tempfile::tempdir().unwrap();
    let native = claude(root.path());
    let elsewhere = root.path().join("elsewhere.json");
    fs::write(&elsewhere, r#"{"claudeAiOauth":{"accessToken":"old"}}"#).unwrap();
    let file = native.config_home.join(".credentials.json");
    std::os::unix::fs::symlink(&elsewhere, &file).unwrap();

    assert_eq!(
        native.write(Some(&incoming())).err().as_deref(),
        Some("Refusing to replace a linked credential file.")
    );
    assert!(fs::symlink_metadata(&file)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!native.config_file.exists(), "the identity was not patched");
}

/// A credentials file that cannot be read is an error, never a missing login, and the switch
/// writes nothing over it.
#[test]
fn an_unreadable_credentials_file_is_an_error_and_nothing_is_written() {
    let root = tempfile::tempdir().unwrap();
    let native = claude(root.path());
    fs::create_dir(native.config_home.join(".credentials.json")).unwrap();

    assert_eq!(
        native.read().err().as_deref(),
        Some("Cannot read native credentials.")
    );
    assert_eq!(
        native.write(Some(&incoming())).err().as_deref(),
        Some("Cannot read native credentials.")
    );
    assert!(!native.config_file.exists(), "the identity was not patched");
}

/// A Claude login past its expiry that cannot renew itself fails verification before anything is
/// asked of the network.
#[test]
fn verification_refuses_an_expired_login_that_cannot_renew() {
    let root = tempfile::tempdir().unwrap();
    let native = claude(root.path());
    fs::write(
        native.config_home.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"old","expiresAt":1}}"#,
    )
    .unwrap();
    fs::write(
        &native.config_file,
        r#"{"oauthAccount":{"accountUuid":"a","organizationUuid":"org-a"}}"#,
    )
    .unwrap();

    assert_eq!(
        native
            .verify_claude(&crate::http::refused_url())
            .err()
            .as_deref(),
        Some("Could not renew the native Claude login. Sign in again if it has expired.")
    );
}

/// Only a lock another process holds is worth waiting on. One that cannot be created at all is
/// reported for what it is, not as Claude being busy.
#[test]
fn a_native_lock_that_cannot_be_created_says_why_instead_of_busy() {
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    let blocker = root.path().join("not-a-directory");
    fs::write(&blocker, "").unwrap();
    native.config_home = blocker.join(".claude");

    let refused = native.lock().err().unwrap();
    assert!(
        refused.starts_with("Cannot take Claude Code's locks: "),
        "{refused}"
    );
    assert_ne!(refused, BUSY);
}

/// A storage-write lock that cannot be created is reported as that lock, not as a bare I/O error.
#[test]
fn a_storage_lock_that_cannot_be_created_is_named_in_the_error() {
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    let blocker = root.path().join("not-a-directory");
    fs::write(&blocker, "").unwrap();
    native.config_home = blocker.join(".claude");

    let refused = native
        .write_locked(Some(&incoming()), Box::new(()))
        .err()
        .unwrap();
    assert!(
        refused.starts_with("Cannot take Claude Code's storage lock: "),
        "{refused}"
    );
}

#[cfg(target_os = "macos")]
mod keychain;
mod secure_storage;
