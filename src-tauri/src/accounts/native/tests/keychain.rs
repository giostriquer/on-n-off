//! Codex's keyring store as the account switch reads and writes it, answered by a fake `security`.

use super::*;

/// The Keychain account Codex files a home's login under: `cli|` and the first sixteen hex digits
/// of the SHA-256 of the home's canonical path.
#[cfg(target_os = "macos")]
fn codex_account(home: &Path) -> String {
    let canonical = fs::canonicalize(home).unwrap();
    let hash = crate::sha::sha256_hex(canonical.to_string_lossy().as_bytes());
    format!("cli|{}", &hash[..16])
}

/// A Keychain holding at most one Codex login, `secret`, under any account.
#[cfg(target_os = "macos")]
fn codex_item(secret: Option<&'static str>) -> impl Fn(&str) -> crate::process::CommandOutcome {
    use crate::process::CommandOutcome;
    move |command| match (secret, command.starts_with("find-generic-password")) {
        (Some(secret), true) => CommandOutcome::Exited {
            success: true,
            stdout: format!("{secret}\n"),
            stderr: String::new(),
        },
        (None, true) => CommandOutcome::Exited {
            success: false,
            stdout: String::new(),
            stderr: "The specified item could not be found in the keychain.".to_string(),
        },
        (_, false) => CommandOutcome::Exited {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        },
    }
}

/// `keyring` reads and writes Codex's own Keychain item for this home, through `security`.
#[cfg(target_os = "macos")]
#[test]
fn codex_keyring_storage_is_the_item_codex_files_for_this_home() {
    use crate::accounts::keychain::with_test_runner;
    let root = tempfile::tempdir().unwrap();
    let store = NativeStore::resolve(root.path()).unwrap();
    fs::create_dir_all(&store.config_home).unwrap();
    fs::write(&store.config_file, "cli_auth_credentials_store = 'keyring'").unwrap();
    fs::write(
        store.config_home.join("auth.json"),
        r#"{"tokens":{"access_token":"file"}}"#,
    )
    .unwrap();
    let account = codex_account(&store.config_home);

    let (read, sent) = with_test_runner(
        codex_item(Some(r#"{"tokens":{"access_token":"keyring"}}"#)),
        || store.read(),
    );
    assert_eq!(
        read.unwrap().unwrap().auth["tokens"]["access_token"],
        "keyring"
    );
    assert_eq!(
        sent,
        [format!(
            "find-generic-password -a {account} -w -s Codex Auth"
        )]
    );

    let (read, _) = with_test_runner(codex_item(None), || store.read());
    assert!(
        read.unwrap().is_none(),
        "no item is no login, whatever the file holds"
    );

    let login = Login {
        auth: json!({"tokens": {"access_token": "incoming"}}),
        account: Value::Null,
    };
    let (written, sent) = with_test_runner(codex_item(None), || store.write(Some(&login)));
    assert_eq!(written, Ok(()));
    let hex: String = serde_json::to_vec(&login.auth)
        .unwrap()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        sent,
        [
            format!("add-generic-password -U -a \"{account}\" -s \"Codex Auth\" -X \"{hex}\"\n"),
            format!("find-generic-password -a {account} -w -s Codex Auth"),
        ],
        "written, then read back"
    );
}

/// `auto` uses the Keychain item when there is one, and the file when there is none.
#[cfg(target_os = "macos")]
#[test]
fn codex_auto_storage_prefers_the_keychain_item_and_falls_back_to_the_file() {
    use crate::accounts::keychain::with_test_runner;
    let root = tempfile::tempdir().unwrap();
    let store = NativeStore::resolve(root.path()).unwrap();
    fs::create_dir_all(&store.config_home).unwrap();
    fs::write(&store.config_file, "cli_auth_credentials_store = 'auto'").unwrap();
    fs::write(
        store.config_home.join("auth.json"),
        r#"{"tokens":{"access_token":"file"}}"#,
    )
    .unwrap();

    let (read, _) = with_test_runner(
        codex_item(Some(r#"{"tokens":{"access_token":"keyring"}}"#)),
        || store.read(),
    );
    assert_eq!(
        read.unwrap().unwrap().auth["tokens"]["access_token"],
        "keyring"
    );
    let (read, _) = with_test_runner(codex_item(None), || store.read());
    assert_eq!(
        read.unwrap().unwrap().auth["tokens"]["access_token"],
        "file"
    );
}
