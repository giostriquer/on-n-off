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
        let identity = crate::limits::credentials::read_claude_identity(root.path()).unwrap();
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

/// Activation's Keychain path on macOS sends `security` exactly the commands the shared writer
/// builds, and nothing else: the login as one `add-generic-password -U` with the secret
/// hex-encoded, a removal as one `delete-generic-password` naming account and service. A write
/// back through the `keyring` crate — this process's own identity, which is what prompted on
/// every switch — would send nothing here and fail this test.
#[cfg(target_os = "macos")]
#[test]
fn the_keyring_target_writes_and_deletes_through_security() {
    use crate::accounts::keychain::{with_test_runner, Runner};
    use crate::process::CommandOutcome;

    let ok: Runner = |_| CommandOutcome::Exited {
        success: true,
        stdout: String::new(),
        stderr: String::new(),
    };
    // Synthetic names on purpose: anyone re-checking this guard by putting the `keyring` crate
    // back would otherwise file a second item under Claude Code's own service, which the
    // service-only read could then return instead of the real login.
    let target = Target::Keyring {
        service: "on-n-off seam rehearsal".into(),
        account: "on-n-off-test".into(),
    };
    let login = json!({"claudeAiOauth": {"accessToken": "one", "note": "a \"quoted\" word"}});
    let hex: String = serde_json::to_vec(&login)
        .unwrap()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();

    let (result, sent) = with_test_runner(ok, || target.write(Some(&login)));
    assert_eq!(result, Ok(()));
    assert_eq!(
        sent,
        vec![format!(
            "add-generic-password -U -a \"on-n-off-test\" -s \"on-n-off seam rehearsal\" -X \"{hex}\"\n"
        )]
    );

    let (result, sent) = with_test_runner(ok, || target.write(None));
    assert_eq!(result, Ok(()));
    assert_eq!(
        sent,
        vec![
            "delete-generic-password -a \"on-n-off-test\" -s \"on-n-off seam rehearsal\"\n"
                .to_string()
        ]
    );

    let refused: Runner = |_| CommandOutcome::Exited {
        success: false,
        stdout: String::new(),
        stderr: "User interaction is not allowed.".to_string(),
    };
    let (result, _) = with_test_runner(refused, || target.write(Some(&login)));
    assert_eq!(
        result,
        Err("Keychain write failed (User interaction is not allowed).".to_string()),
        "the refusal reads as a sentence, for the transaction to prefix its own"
    );
}

/// The same path against the real tool, on a throwaway entry: publish, read back, remove, remove
/// again. What it proves beyond the test above is the wiring to `security` itself.
///
/// `cargo test --manifest-path src-tauri/Cargo.toml rehearse_the_keyring_target -- --ignored`
#[cfg(target_os = "macos")]
#[test]
#[ignore = "writes a throwaway Keychain entry; not part of CI"]
fn rehearse_the_keyring_target_through_security() {
    let entry = crate::accounts::keychain::ThrowawayEntry {
        service: "on-n-off native keychain rehearsal",
        account: "on-n-off-test",
    };
    let target = Target::Keyring {
        service: entry.service.into(),
        account: entry.account.into(),
    };
    let login = json!({"claudeAiOauth": {"accessToken": "one", "note": "a \"quoted\" word"}});
    target.write(Some(&login)).unwrap();
    assert_eq!(target.read().unwrap(), Some(login));
    target.write(None).unwrap();
    assert_eq!(target.read().unwrap(), None);
    target.write(None).unwrap();
    assert_eq!(
        target.read().unwrap(),
        None,
        "removing an entry already gone is not an error"
    );
    drop(entry);
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
    for (value, config) in [
        ("/Users/me/cafe\u{301}", "/Users/me/caf\u{e9}"),
        ("/Users/me/claude ", "/Users/me/claude "),
    ] {
        let env = [("CLAUDE_CONFIG_DIR", PathBuf::from(value))];
        let store =
            NativeStore::resolve_from(AgentId::Claude, root.path(), &environment(&env)).unwrap();
        assert_eq!(store.config_home, PathBuf::from(config), "{value:?}");
        assert_eq!(
            store.config_file,
            PathBuf::from(config).join(".claude.json")
        );
    }
}

const SECURE_STORAGE: &str = "CLAUDE_SECURESTORAGE_CONFIG_DIR";

/// Under `CLAUDE_SECURESTORAGE_CONFIG_DIR` the switch reads Claude Code's login from that dir and
/// takes the refresh locks there, while the identity file and its lock stay with the config dir.
/// The `claude` it starts is handed the same variable, so it works in the same store.
#[test]
fn the_switch_works_in_the_secure_storage_dir() {
    let root = tempfile::tempdir().unwrap();
    let secure = root.path().join("secure");
    fs::create_dir_all(&secure).unwrap();
    fs::create_dir_all(root.path().join(".claude")).unwrap();
    fs::write(
        secure.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"secure-token"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join(".claude").join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"config-token"}}"#,
    )
    .unwrap();
    let env = [(SECURE_STORAGE, secure.clone())];
    let mut native =
        NativeStore::resolve_from(AgentId::Claude, root.path(), &environment(&env)).unwrap();
    native.use_keychain = false;

    assert_eq!(native.config_home, root.path().join(".claude"));
    assert_eq!(
        native.read().unwrap().unwrap().auth["claudeAiOauth"]["accessToken"],
        "secure-token"
    );
    let guard = native.lock().unwrap();
    assert!(secure.join(".oauth_refresh.lock").is_dir());
    assert!(!root
        .path()
        .join(".claude")
        .join(".oauth_refresh.lock")
        .exists());
    assert!(root.path().join(".claude.json.lock").is_dir());
    drop(guard);

    let command = native.command();
    let envs: std::collections::HashMap<_, _> = command.get_envs().collect();
    assert_eq!(
        envs.get(std::ffi::OsStr::new(SECURE_STORAGE)),
        Some(&Some(secure.as_os_str()))
    );
}

/// An isolated sign-in stores its login in its own home. A `CLAUDE_SECURESTORAGE_CONFIG_DIR`
/// inherited from on-n-off's environment would send it to the user's real store instead.
#[test]
fn an_isolated_claude_sign_in_never_inherits_a_secure_storage_dir() {
    let root = tempfile::tempdir().unwrap();
    let native = NativeStore::isolated(AgentId::Claude, root.path()).unwrap();
    let command = native.command();
    let envs: std::collections::HashMap<_, _> = command.get_envs().collect();
    assert_eq!(
        envs.get(std::ffi::OsStr::new(SECURE_STORAGE)),
        Some(&None),
        "removed from the child's environment"
    );
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
/// credentials file can hold. Only the presence of `claudeAiOauth` decides, so an emptied login
/// still reads as one.
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

    fs::write(&file, SIGNED_OUT).unwrap();
    let emptied = native.read().unwrap().unwrap();
    assert_eq!(emptied.auth["claudeAiOauth"]["accessToken"], "");

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

/// Answers `security` as a Keychain holding at most one Claude Code item, filed under `me`: its
/// attributes, and its secret or the reason the secret could not be read.
#[cfg(target_os = "macos")]
fn keychain_item(
    secret: Option<Result<&'static str, &'static str>>,
) -> impl Fn(&str) -> crate::process::CommandOutcome + 'static {
    use crate::process::CommandOutcome;
    move |command| {
        let exited = |success: bool, stdout: &str, stderr: &str| CommandOutcome::Exited {
            success,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        };
        match (secret, command.contains(" -w ")) {
            (None, _) => exited(
                false,
                "",
                "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain.",
            ),
            (Some(_), false) => exited(true, "    \"acct\"<blob>=\"me\"\n", ""),
            (Some(Ok(secret)), true) => exited(true, &format!("{secret}\n"), ""),
            (Some(Err(why)), true) => exited(false, "", why),
        }
    }
}

/// What the account switch's read found, told apart by the fixture each store holds.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    Keychain,
    File,
    /// A login whose token is empty, from either store.
    Emptied,
    Nothing,
    Malformed,
    Denied,
}

#[cfg(target_os = "macos")]
fn found(read: Result<Option<Login>, String>) -> Found {
    match read {
        Ok(Some(login)) => match login.auth["claudeAiOauth"]["accessToken"].as_str() {
            Some("kc-token") => Found::Keychain,
            Some("file-token") => Found::File,
            Some("") => Found::Emptied,
            other => panic!("unexpected token {other:?}"),
        },
        Ok(None) => Found::Nothing,
        Err(why)
            if why == "The native credential document is malformed. It has not been changed." =>
        {
            Found::Malformed
        }
        Err(why) if why.contains("User canceled the operation") => Found::Denied,
        Err(why) => panic!("unexpected error {why}"),
    }
}

/// The account switch's read for every combination of what the Keychain entry and the
/// credentials file hold, taken from the store Claude Code would read: a Keychain entry that
/// parses, token or not; otherwise the file. Rows are the Keychain, columns the file: no file, a
/// login, no token, broken.
#[cfg(target_os = "macos")]
#[test]
fn the_native_claude_read_truth_table() {
    use crate::accounts::keychain::with_test_runner;
    use Found::{Denied, Emptied, File, Keychain, Malformed, Nothing};
    let keychain = [
        ("no item", None),
        (
            "a login",
            Some(Ok(r#"{"claudeAiOauth":{"accessToken":"kc-token"}}"#)),
        ),
        ("no token", Some(Ok(SIGNED_OUT))),
        ("invalid JSON", Some(Ok("{ not json"))),
        (
            "unreadable",
            Some(Err(
                "security: SecKeychainItemCopyContent: User canceled the operation.",
            )),
        ),
    ];
    let files = [
        ("no file", None),
        (
            "a login",
            Some(r#"{"claudeAiOauth":{"accessToken":"file-token"}}"#),
        ),
        ("no token", Some(SIGNED_OUT)),
        ("broken", Some("{ not json")),
    ];
    let expected = [
        [Nothing, File, Emptied, Malformed],
        [Keychain, Keychain, Keychain, Keychain],
        [Emptied, Emptied, Emptied, Emptied],
        [Nothing, File, Emptied, Malformed],
        [Denied, File, Emptied, Denied],
    ];
    for ((item, secret), row) in keychain.into_iter().zip(expected) {
        for ((file, contents), want) in files.into_iter().zip(row) {
            let root = tempfile::tempdir().unwrap();
            let mut native = claude(root.path());
            native.use_keychain = true;
            if let Some(contents) = contents {
                fs::write(native.config_home.join(".credentials.json"), contents).unwrap();
            }
            let (read, _) = with_test_runner(keychain_item(secret), || native.read());
            assert_eq!(found(read), want, "Keychain: {item}; file: {file}");
        }
    }
}

/// An entry whose attributes cannot be read, or name no account, cannot be read either: the
/// credentials file answers in its place, as it would for Claude Code.
#[cfg(target_os = "macos")]
#[test]
fn a_keychain_entry_the_switch_cannot_identify_leaves_the_read_to_the_file() {
    use crate::accounts::keychain::with_test_runner;
    use crate::process::CommandOutcome;
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    native.use_keychain = true;
    fs::write(
        native.config_home.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"file-token"}}"#,
    )
    .unwrap();

    let (read, sent) = with_test_runner(
        |_| CommandOutcome::Exited {
            success: false,
            stdout: String::new(),
            stderr: "User interaction is not allowed.".to_string(),
        },
        || native.read(),
    );
    assert_eq!(found(read), Found::File);
    assert_eq!(
        sent,
        ["find-generic-password -a claude-code-user -w -s Claude Code-credentials"],
        "a refused read of Claude Code's own item is not looked past"
    );

    let (read, _) = with_test_runner(
        |command| CommandOutcome::Exited {
            success: !command.contains(" -w "),
            stdout: "    \"svce\"<blob>=\"Claude Code-credentials\"\n".to_string(),
            stderr: "The specified item could not be found in the keychain.".to_string(),
        },
        || native.read(),
    );
    assert_eq!(found(read), Found::File);
}

/// Claude Code's own item, filed under the name it derives (in a test binary, which sees no
/// `$USER`, the fallback `claude-code-user`), and an older item under another account.
#[cfg(target_os = "macos")]
const TWO_ITEMS: &[(&str, &str)] = &[
    (
        "claude-code-user",
        r#"{"claudeAiOauth":{"accessToken":"kc-token"}}"#,
    ),
    (
        "other",
        r#"{"claudeAiOauth":{"accessToken":"other-token"}}"#,
    ),
];

/// Claude Code reads the item filed under its own account name, so the switch does too, rather
/// than whichever item `security` happens to return for the service.
#[cfg(target_os = "macos")]
#[test]
fn the_switch_reads_the_item_filed_under_claude_codes_own_account() {
    use crate::accounts::keychain::{fake_items, with_test_runner};
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    native.use_keychain = true;

    let (read, sent) = with_test_runner(fake_items(TWO_ITEMS), || native.read());
    assert_eq!(found(read), Found::Keychain);
    assert_eq!(
        sent,
        ["find-generic-password -a claude-code-user -w -s Claude Code-credentials"]
    );
}

/// An item an older Claude Code filed under another account is still Claude Code's login when it is
/// the only one: the service-only lookup names its account.
#[cfg(target_os = "macos")]
#[test]
fn an_item_filed_under_another_account_is_still_found() {
    use crate::accounts::keychain::{fake_items, with_test_runner};
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    native.use_keychain = true;

    let (read, sent) = with_test_runner(
        fake_items(&[("other", r#"{"claudeAiOauth":{"accessToken":"kc-token"}}"#)]),
        || native.read(),
    );
    assert_eq!(found(read), Found::Keychain);
    assert_eq!(
        sent,
        [
            "find-generic-password -a claude-code-user -w -s Claude Code-credentials",
            "find-generic-password -s Claude Code-credentials",
            "find-generic-password -a other -w -s Claude Code-credentials",
        ]
    );
}

/// `add-generic-password -U` replaces the item matching service and account, so the write names
/// the account of the item it read; any other would file a second item beside it.
#[cfg(target_os = "macos")]
#[test]
fn the_switch_writes_back_under_the_account_of_the_item_it_read() {
    use crate::accounts::keychain::{fake_items, with_test_runner};
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    native.use_keychain = true;

    let (written, sent) =
        with_test_runner(fake_items(TWO_ITEMS), || native.write(Some(&incoming())));
    assert_eq!(written, Ok(()));
    let add = sent
        .iter()
        .find(|command| command.starts_with("add-generic-password"))
        .expect("the login is written to the Keychain");
    assert!(
        add.starts_with(
            "add-generic-password -U -a \"claude-code-user\" -s \"Claude Code-credentials\""
        ),
        "{add}"
    );
}

fn incoming() -> Login {
    Login {
        auth: json!({"claudeAiOauth":{"accessToken":"incoming"}}),
        account: json!({"accountUuid":"b","organizationUuid":"org-b"}),
    }
}

/// A write goes to the store Claude Code's next read uses. A Keychain entry that is not JSON is one
/// that read skips, so the login goes to the credentials file and the entry is left alone.
#[cfg(target_os = "macos")]
#[test]
fn a_switch_past_a_malformed_keychain_entry_writes_the_credentials_file() {
    use crate::accounts::keychain::with_test_runner;
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    native.use_keychain = true;
    let file = native.config_home.join(".credentials.json");
    fs::write(&file, r#"{"claudeAiOauth":{"accessToken":"file-token"}}"#).unwrap();

    let (written, sent) = with_test_runner(keychain_item(Some(Ok("{ not json"))), || {
        native.write(Some(&incoming()))
    });
    assert_eq!(written, Ok(()));
    assert_eq!(
        read_json(&file).unwrap()["claudeAiOauth"]["accessToken"],
        "incoming"
    );
    assert!(
        sent.iter()
            .all(|command| command.starts_with("find-generic-password")),
        "the Keychain is only read: {sent:?}"
    );
}

/// When the Keychain cannot be read, which store Claude Code reads next is unknown, so the switch
/// writes neither.
#[cfg(target_os = "macos")]
#[test]
fn a_switch_refuses_to_write_when_the_keychain_cannot_be_read() {
    use crate::accounts::keychain::with_test_runner;
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    native.use_keychain = true;
    let file = native.config_home.join(".credentials.json");
    fs::write(&file, r#"{"claudeAiOauth":{"accessToken":"file-token"}}"#).unwrap();

    let (written, sent) = with_test_runner(
        keychain_item(Some(Err(
            "security: SecKeychainItemCopyContent: User canceled the operation.",
        ))),
        || native.write(Some(&incoming())),
    );
    assert!(written.is_err());
    assert_eq!(
        read_json(&file).unwrap()["claudeAiOauth"]["accessToken"],
        "file-token"
    );
    assert!(
        !native.config_file.exists(),
        "no half of the switch is written"
    );
    assert!(sent
        .iter()
        .all(|command| command.starts_with("find-generic-password")));
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
