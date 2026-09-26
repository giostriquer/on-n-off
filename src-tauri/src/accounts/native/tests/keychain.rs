//! The account switch's reads and writes of Claude Code's Keychain item, answered by a fake
//! `security` so no test touches the login Keychain.

use super::*;

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
    crate::accounts::with_real_keychain(rehearse_the_keyring_target);
}

#[cfg(target_os = "macos")]
fn rehearse_the_keyring_target() {
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
    /// A login whose token is empty, from either store: never an answer, since Claude Code's
    /// sign-out empties the login.
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
    use Found::{Denied, File, Keychain, Malformed, Nothing};
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
        [Nothing, File, Nothing, Malformed],
        [Keychain, Keychain, Keychain, Keychain],
        [Nothing, Nothing, Nothing, Nothing],
        [Nothing, File, Nothing, Malformed],
        [Denied, File, Nothing, Denied],
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

/// An entry whose secret is refused, or whose attributes name no account, cannot be read: the
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

    // And the write goes back under that account, never a guessed one.
    let (written, sent) = with_test_runner(
        fake_items(&[("other", r#"{"claudeAiOauth":{"accessToken":"kc-token"}}"#)]),
        || native.write(Some(&incoming())),
    );
    assert_eq!(written, Ok(()));
    assert!(
        sent.iter()
            .any(|command| command.starts_with("add-generic-password -U -a \"other\" ")),
        "{sent:?}"
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

/// A lookup that would name the item's account and fails leaves the Keychain unreadable, not
/// empty: which store Claude Code reads next is unknown, so the switch writes neither.
#[cfg(target_os = "macos")]
#[test]
fn a_failed_account_lookup_is_not_read_as_no_entry() {
    use crate::accounts::keychain::with_test_runner;
    use crate::process::CommandOutcome;
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    native.use_keychain = true;
    let file = native.config_home.join(".credentials.json");
    fs::write(&file, r#"{"claudeAiOauth":{"accessToken":"file-token"}}"#).unwrap();

    let (written, _) = with_test_runner(
        |command| {
            let own = command.contains(" -a claude-code-user ") && command.contains(" -w ");
            CommandOutcome::Exited {
                success: false,
                stdout: String::new(),
                stderr: if own {
                    "The specified item could not be found in the keychain."
                } else {
                    "User interaction is not allowed."
                }
                .to_string(),
            }
        },
        || native.write(Some(&incoming())),
    );
    assert!(written.is_err());
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        r#"{"claudeAiOauth":{"accessToken":"file-token"}}"#
    );
    assert!(!native.config_file.exists(), "the identity was not patched");
}

/// A Keychain holding one item under Claude Code's own account, whose secret the writes replace,
/// recording for each read of the secret whether `lock` was held at that moment.
#[cfg(target_os = "macos")]
fn recording_keychain(
    lock: PathBuf,
    reads: std::rc::Rc<std::cell::RefCell<Vec<bool>>>,
) -> impl Fn(&str) -> crate::process::CommandOutcome + 'static {
    use crate::process::CommandOutcome;
    let secret =
        std::cell::RefCell::new(r#"{"claudeAiOauth":{"accessToken":"kc-token"}}"#.to_string());
    move |command| {
        let exited = |stdout: String| CommandOutcome::Exited {
            success: true,
            stdout,
            stderr: String::new(),
        };
        if let Some((_, hex)) = command.rsplit_once("-X \"") {
            let hex = hex.trim_end().trim_end_matches('"');
            let bytes: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
                .collect();
            *secret.borrow_mut() = String::from_utf8(bytes).unwrap();
            return exited(String::new());
        }
        if command.contains(" -w ") {
            reads.borrow_mut().push(lock.is_dir());
            return exited(format!("{}\n", secret.borrow()));
        }
        exited("    \"acct\"<blob>=\"claude-code-user\"\n".to_string())
    }
}

/// The switch reads the published login back before it lets Claude Code's locks go, so a client
/// that writes in between is caught rather than verified.
#[cfg(target_os = "macos")]
#[test]
fn the_switch_reads_its_write_back_while_it_still_holds_the_locks() {
    use crate::accounts::keychain::with_test_runner;
    let root = tempfile::tempdir().unwrap();
    let mut native = claude(root.path());
    native.use_keychain = true;
    let reads = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let runner = recording_keychain(
        native.config_home.join(".oauth_refresh.lock"),
        std::rc::Rc::clone(&reads),
    );

    let (read_back, sent) = with_test_runner(runner, || {
        native.write_locked(Some(&incoming()), native.lock().unwrap())
    });
    let read_back = read_back.unwrap().unwrap().unwrap();
    assert_eq!(read_back.auth["claudeAiOauth"]["accessToken"], "incoming");
    let written_at = sent
        .iter()
        .position(|command| command.starts_with("add-generic-password"))
        .expect("the login is written to the Keychain");
    let reads_after = sent[written_at..]
        .iter()
        .filter(|command| command.contains(" -w "))
        .count();
    assert!(reads_after > 0, "the write is read back: {sent:?}");
    let reads = reads.borrow();
    assert!(
        reads[reads.len() - reads_after..].iter().all(|held| *held),
        "every read after the write saw the lock held: {reads:?}"
    );
}

/// Cleaning up an isolated sign-in deletes its own scoped Keychain entry and nothing else: never
/// Claude Code's unscoped entry, which holds the user's real login.
#[cfg(target_os = "macos")]
#[test]
fn cleaning_an_isolated_sign_in_deletes_only_its_scoped_entry() {
    use crate::accounts::keychain::{fake_items, with_test_runner};
    let root = tempfile::tempdir().unwrap();
    let isolated = NativeStore::isolated(AgentId::Claude, root.path()).unwrap();
    let hash = crate::sha::sha256_hex(root.path().join(".claude").to_str().unwrap().as_bytes());
    let scoped = format!("Claude Code-credentials-{}", &hash[..8]);

    let (cleaned, sent) = with_test_runner(fake_items(&[("claude-code-user", "{}")]), || {
        isolated.clean_isolated()
    });
    assert_eq!(cleaned, Ok(()));
    let deletes: Vec<&String> = sent
        .iter()
        .filter(|command| command.starts_with("delete-generic-password"))
        .collect();
    assert_eq!(
        deletes,
        [&format!(
            "delete-generic-password -a \"claude-code-user\" -s \"{scoped}\"\n"
        )]
    );
    assert!(
        sent.iter()
            .all(|command| !command.ends_with("Claude Code-credentials")
                && !command.ends_with("\"Claude Code-credentials\"\n")),
        "nothing touches the unscoped entry: {sent:?}"
    );
}

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
    let store = NativeStore::resolve(AgentId::Codex, root.path()).unwrap();
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
    let store = NativeStore::resolve(AgentId::Codex, root.path()).unwrap();
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
