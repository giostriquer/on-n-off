use super::*;

const SECURE_STORAGE: &str = "CLAUDE_SECURESTORAGE_CONFIG_DIR";

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
    let mut native = ClaudeNative::resolve_from(root.path(), &environment(&env)).unwrap();
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

#[test]
fn an_isolated_claude_sign_in_never_inherits_a_secure_storage_dir() {
    let root = tempfile::tempdir().unwrap();
    let native = ClaudeNative::isolated(root.path()).unwrap();
    let command = native.command();
    let envs: std::collections::HashMap<_, _> = command.get_envs().collect();
    assert_eq!(
        envs.get(std::ffi::OsStr::new(SECURE_STORAGE)),
        Some(&None),
        "removed from the child's environment"
    );
}

#[test]
fn account_changes_defer_to_the_official_client_for_a_moved_store() {
    let root = tempfile::tempdir().unwrap();
    let preflight = |variable: &str, value: PathBuf| {
        let env = [(variable, value)];
        let store = ClaudeNative::resolve_from(root.path(), &environment(&env)).unwrap();
        store.preflight()
    };

    for moved in [root.path().join("secure"), root.path().join(".claude")] {
        assert_eq!(
            preflight(SECURE_STORAGE, moved.clone()).err().as_deref(),
            Some(CUSTOM_HOME),
            "{moved:?}"
        );
    }
    assert_eq!(
        preflight("CLAUDE_CONFIG_DIR", root.path().join("work"))
            .err()
            .as_deref(),
        Some(CUSTOM_HOME),
        "the same refusal as for a CLAUDE_CONFIG_DIR home"
    );
    assert_ne!(
        preflight(SECURE_STORAGE, PathBuf::new()).err().as_deref(),
        Some(CUSTOM_HOME),
        "set but empty, the store is the default one"
    );
}

#[test]
fn the_usage_read_works_in_its_own_config_dir_and_signs_in_from_the_users_store() {
    let root = tempfile::tempdir().unwrap();
    let own = root.path().join("usage");
    let custom = root.path().join("custom");
    let secure = root.path().join("secure");
    let empty = std::ffi::OsString::new();
    for (variable, value, storage) in [
        (None, PathBuf::new(), empty.clone()),
        (
            Some("CLAUDE_CONFIG_DIR"),
            custom.clone(),
            custom.clone().into(),
        ),
        (Some(SECURE_STORAGE), secure.clone(), secure.clone().into()),
        (Some(SECURE_STORAGE), PathBuf::new(), empty.clone()),
    ] {
        let env: Vec<_> = variable
            .map(|name| (name, value.clone()))
            .into_iter()
            .collect();
        let store = ClaudeNative::resolve_from(root.path(), &environment(&env)).unwrap();

        let command = SignedIn(store).command_in(&own);

        let env = command_env(&command);
        assert_eq!(
            env["CLAUDE_CONFIG_DIR"],
            Some(own.clone().into()),
            "{variable:?}"
        );
        assert_eq!(env[SECURE_STORAGE], Some(storage), "{variable:?}");
        assert_eq!(
            command.get_current_dir(),
            Some(std::env::temp_dir().as_path())
        );
    }

    fs::create_dir_all(&own).unwrap();
    let store = ClaudeNative::resolve_from(root.path(), &environment(&[])).unwrap();
    assert_eq!(
        SignedIn(store).command_in(&own).get_current_dir(),
        Some(own.as_path())
    );
}

#[test]
fn a_disposable_homes_usage_read_signs_in_from_that_home_never_the_users() {
    let root = tempfile::tempdir().unwrap();
    let own = root.path().join("usage");
    let disposable = [("ON_N_OFF_HOME", PathBuf::from("disposable"))];
    let store = ClaudeNative::resolve_from(root.path(), &environment(&disposable)).unwrap();

    let env = command_env(&SignedIn(store).command_in(&own));

    assert_eq!(env[SECURE_STORAGE], Some(std::ffi::OsString::new()));
    assert_eq!(env["HOME"], Some(root.path().into()));
    assert_eq!(env["USERPROFILE"], Some(root.path().into()));
}
