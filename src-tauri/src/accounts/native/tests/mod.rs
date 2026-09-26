use super::*;

/// What account changes say about a native home the environment chose.
const CUSTOM_HOME: &str = "Account activation currently supports the default CLI home. Remove the custom home override or use the official CLI for this context.";

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
/// the test's own home: `resolve` reads the test environment, not `CODEX_HOME` or whatever a
/// sibling test did to `ON_N_OFF_HOME`.
#[test]
fn a_test_resolves_the_codex_store_inside_its_own_home_whatever_the_machine_exports() {
    let root = tempfile::tempdir().unwrap();
    let store = NativeStore::resolve(root.path()).unwrap();
    assert_eq!(store.config_home, root.path().join(".codex"));
    assert!(!store.custom);
}

#[test]
fn outside_a_disposable_home_codex_home_applies() {
    let root = tempfile::tempdir().unwrap();
    let codex_home = root.path().join("elsewhere").join("codex");
    let env = [("CODEX_HOME", codex_home.clone())];
    let store = NativeStore::resolve_from(root.path(), &environment(&env)).unwrap();
    assert_eq!(store.config_home, codex_home);
    assert_eq!(store.config_file, codex_home.join("config.toml"));
    assert!(store.custom);
}

#[test]
fn a_disposable_home_ignores_codex_home() {
    let root = tempfile::tempdir().unwrap();
    let env = [
        ("ON_N_OFF_HOME", root.path().to_path_buf()),
        ("CODEX_HOME", root.path().join("elsewhere")),
    ];
    let store = NativeStore::resolve_from(root.path(), &environment(&env)).unwrap();
    assert_eq!(store.config_home, root.path().join(".codex"));
    assert!(!store.custom);
}

#[test]
fn a_relative_provider_override_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let env = [("CODEX_HOME", Path::new("relative").join("codex"))];
    assert_eq!(
        NativeStore::resolve_from(root.path(), &environment(&env)).err(),
        Some("The provider home must be an absolute path.".to_string())
    );
}

/// A linked configuration file is the official client's to change.
#[cfg(unix)]
#[test]
fn account_changes_refuse_a_linked_codex_configuration_file() {
    let root = tempfile::tempdir().unwrap();
    let store = NativeStore::resolve(root.path()).unwrap();
    fs::create_dir_all(&store.config_home).unwrap();
    let elsewhere = root.path().join("elsewhere");
    fs::write(&elsewhere, "").unwrap();
    std::os::unix::fs::symlink(&elsewhere, &store.config_file).unwrap();
    assert_eq!(
        store.preflight().err().as_deref(),
        Some("Linked account configuration must be changed through the official CLI.")
    );
}

/// A Codex home `CODEX_HOME` chose is custom, and account changes defer to the official client for
/// it, as they do for a Claude home `CLAUDE_CONFIG_DIR` chose.
#[test]
fn account_changes_defer_to_the_official_client_for_a_codex_home_the_environment_chose() {
    let root = tempfile::tempdir().unwrap();
    let env = [("CODEX_HOME", root.path().join("work"))];
    let store = NativeStore::resolve_from(root.path(), &environment(&env)).unwrap();
    assert_eq!(store.preflight().err().as_deref(), Some(CUSTOM_HOME));
}

/// Codex settings that choose how it signs in, or which profile it runs, are a policy only its
/// official controls may change; any other setting is not.
#[test]
fn account_changes_refuse_a_codex_configuration_that_enforces_how_it_signs_in() {
    const POLICY: &str =
        "This Codex installation enforces an authentication policy. Use its official sign-in controls.";
    let root = tempfile::tempdir().unwrap();
    let store = NativeStore::resolve(root.path()).unwrap();
    fs::create_dir_all(&store.config_home).unwrap();
    for (config, refusal) in [
        ("forced_login_method = 'chatgpt'", Some(POLICY)),
        ("forced_chatgpt_workspace_id = 'team'", Some(POLICY)),
        ("auth_keyring_backend = 'secret-service'", Some(POLICY)),
        ("profile = 'work'", Some(POLICY)),
        ("model = 'fixture-model'", None),
        ("not = [toml", Some("Native configuration is malformed.")),
    ] {
        fs::write(&store.config_file, config).unwrap();
        assert_eq!(store.preflight().err().as_deref(), refusal, "{config}");
    }
}

/// A `codex` started for a store works in that store's home, from inside it.
#[test]
fn a_codex_command_works_in_its_stores_home() {
    let root = tempfile::tempdir().unwrap();
    let store = NativeStore::resolve(root.path()).unwrap();
    let command = store.command();
    assert_eq!(
        command_env(&command).get("CODEX_HOME"),
        Some(&Some(root.path().join(".codex").into_os_string()))
    );
    assert_eq!(
        command.get_current_dir(),
        Some(root.path().join(".codex").as_path())
    );
}

/// A Codex login is whatever document its store holds, read verbatim; there is no account record
/// beside it.
#[test]
fn a_codex_read_is_the_stored_document_verbatim() {
    let root = tempfile::tempdir().unwrap();
    let store = NativeStore::resolve(root.path()).unwrap();
    assert!(store.read().unwrap().is_none(), "no auth.json");
    fs::create_dir_all(&store.config_home).unwrap();
    let document =
        json!({"OPENAI_API_KEY": null, "tokens": {"access_token": "access"}, "extra": 1});
    fs::write(store.config_home.join("auth.json"), document.to_string()).unwrap();
    let login = store.read().unwrap().unwrap();
    assert_eq!(login.auth, document);
    assert_eq!(login.account, Value::Null);
}

/// Codex's config chooses its credential store: the file by default or when named, and nothing
/// on-n-off cannot follow, nor a config profile whose store it cannot verify.
#[test]
fn codex_config_selects_the_file_store_and_refuses_what_it_cannot_follow() {
    let root = tempfile::tempdir().unwrap();
    let store = NativeStore::resolve(root.path()).unwrap();
    fs::create_dir_all(&store.config_home).unwrap();
    let auth = store.config_home.join("auth.json");
    fs::write(&auth, r#"{"tokens":{"access_token":"file"}}"#).unwrap();
    for config in ["", "cli_auth_credentials_store = 'file'"] {
        fs::write(&store.config_file, config).unwrap();
        assert_eq!(
            store.read().unwrap().unwrap().auth["tokens"]["access_token"],
            "file",
            "{config:?}"
        );
    }
    for (config, refusal) in [
        (
            "cli_auth_credentials_store = 'ephemeral'",
            "This Codex credential backend cannot be activated by on-n-off. Use official sign-in.",
        ),
        (
            "profile = 'work'",
            "Codex configuration profiles must use the official account controls until their effective credential backend can be verified.",
        ),
    ] {
        fs::write(&store.config_file, config).unwrap();
        assert_eq!(store.read().err().as_deref(), Some(refusal), "{config}");
        assert_eq!(store.write(None).err().as_deref(), Some(refusal), "{config}");
    }
    assert!(auth.exists(), "a refused store is not written");
}

/// Publishing a Codex login writes its document to the file store verbatim; signing out removes
/// it; a linked file is never replaced.
#[test]
fn a_codex_publication_writes_the_file_store_verbatim() {
    let root = tempfile::tempdir().unwrap();
    let store = NativeStore::resolve(root.path()).unwrap();
    fs::create_dir_all(&store.config_home).unwrap();
    let auth = store.config_home.join("auth.json");
    let login = Login {
        auth: json!({"tokens": {"access_token": "incoming"}, "extra": {"kept": true}}),
        account: Value::Null,
    };
    store.write(Some(&login)).unwrap();
    assert_eq!(read_json(&auth).unwrap(), login.auth);
    store.write(None).unwrap();
    assert!(!auth.exists());
    store.write(None).unwrap();

    #[cfg(unix)]
    {
        let elsewhere = root.path().join("elsewhere.json");
        fs::write(&elsewhere, "{}").unwrap();
        std::os::unix::fs::symlink(&elsewhere, &auth).unwrap();
        assert_eq!(
            store.write(Some(&login)).err().as_deref(),
            Some("Refusing to replace a linked credential file.")
        );
        assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "{}");
    }
}

/// The environment a command reads, as the child will see it: `Some(None)` is a variable removed.
fn command_env(command: &Command) -> std::collections::HashMap<String, Option<std::ffi::OsString>> {
    command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(std::ffi::OsStr::to_os_string),
            )
        })
        .collect()
}

#[cfg(target_os = "macos")]
mod keychain;
