//! The account switch under `CLAUDE_SECURESTORAGE_CONFIG_DIR`, which moves Claude Code's login and
//! locks away from its config dir.

use super::*;

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
