use super::*;

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

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    Keychain,
    File,
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
        native::read_json(&file).unwrap()["claudeAiOauth"]["accessToken"],
        "incoming"
    );
    assert!(
        sent.iter()
            .all(|command| command.starts_with("find-generic-password")),
        "the Keychain is only read: {sent:?}"
    );
}

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
        native::read_json(&file).unwrap()["claudeAiOauth"]["accessToken"],
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

#[cfg(target_os = "macos")]
#[test]
fn cleaning_an_isolated_sign_in_deletes_only_its_scoped_entry() {
    use crate::accounts::keychain::{fake_items, with_test_runner};
    let root = tempfile::tempdir().unwrap();
    let isolated = ClaudeNative::isolated(root.path()).unwrap();
    let hash = crate::sha::sha256_hex(root.path().join(".claude").to_str().unwrap().as_bytes());
    let scoped = format!("Claude Code-credentials-{}", &hash[..8]);

    let (cleaned, sent) = with_test_runner(fake_items(&[("claude-code-user", "{}")]), || {
        isolated.clean()
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
