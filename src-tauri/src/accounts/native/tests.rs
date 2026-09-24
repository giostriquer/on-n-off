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
