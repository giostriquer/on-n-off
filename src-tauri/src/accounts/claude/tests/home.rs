//! A saved Claude account's home (`ClaudeHome`): where its login is filed, how it is emptied and
//! removed, and that its usage is Claude Code's own report there. On macOS a fake `security`
//! stands in for the Keychain, so no test touches the login Keychain.
use super::super::home::ClaudeHome;
use super::*;
use crate::accounts::Home;
use crate::cli_stub::CliStub;

fn login(generation: &str) -> Login {
    Login {
        auth: json!({"claudeAiOauth": {
            "accessToken": format!("access-{generation}"),
            "refreshToken": format!("refresh-{generation}"),
            "expiresAt": 1
        }}),
        account: json!({
            "accountUuid": "user",
            "organizationUuid": "team",
            "emailAddress": "you@example.com"
        }),
    }
}

fn identity() -> Identity {
    Identity {
        provider: AgentId::Claude,
        user_id: "user".into(),
        workspace_id: "team".into(),
    }
}

/// The login's access token, or `None` for none.
fn token(login: Option<Login>) -> Option<String> {
    login?.auth["claudeAiOauth"]["accessToken"]
        .as_str()
        .map(str::to_string)
}

/// Runs `run` against a Keychain that keeps what is written to it: on macOS a fake `security`
/// holding at most one item, whose commands come back with the result; elsewhere, where Claude
/// Code keeps its login in a file, `run` alone.
#[cfg(target_os = "macos")]
fn keychain<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    use crate::process::CommandOutcome;
    use std::{cell::RefCell, rc::Rc};
    let item: Rc<RefCell<Option<String>>> = Rc::default();
    let held = item.clone();
    let runner = move |command: &str| {
        let exited = |success: bool, stdout: String, stderr: &str| CommandOutcome::Exited {
            success,
            stdout,
            stderr: stderr.to_string(),
        };
        if command.starts_with("add-generic-password") {
            let hex = command
                .split("-X \"")
                .nth(1)
                .unwrap()
                .trim_end()
                .trim_end_matches('"');
            let bytes: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            *held.borrow_mut() = Some(String::from_utf8(bytes).unwrap());
            return exited(true, String::new(), "");
        }
        if command.starts_with("delete-generic-password") {
            *held.borrow_mut() = None;
            return exited(true, String::new(), "");
        }
        match (held.borrow().clone(), command.contains(" -w ")) {
            (None, _) => exited(
                false,
                String::new(),
                "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain.",
            ),
            (Some(secret), true) => exited(true, format!("{secret}\n"), ""),
            (Some(_), false) => exited(true, "    \"acct\"<blob>=\"claude-code-user\"\n".into(), ""),
        }
    };
    let result = crate::accounts::keychain::with_test_runner(runner, run);
    drop(item);
    result
}

#[cfg(not(target_os = "macos"))]
fn keychain<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    (run(), Vec::new())
}

#[test]
fn a_login_put_in_a_home_reads_back_with_its_account() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());

    let (back, _) = keychain(|| {
        let locks = home.lock().unwrap();
        home.put(&login("b1"), locks.as_ref()).unwrap()
    });

    let back = back.expect("the login it was given");
    assert_eq!(back.auth, login("b1").auth);
    assert_eq!(home.identify(&back).unwrap(), identity());
}

/// Claude Code reads a credentials file as the login as readily as the Keychain, so a home's login
/// never goes to one on macOS: it goes to the home's own scoped entry, filed under Claude Code's
/// own account name.
#[cfg(target_os = "macos")]
#[test]
fn on_macos_a_homes_login_goes_to_its_own_keychain_entry_never_a_file() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());

    let (_, sent) = keychain(|| {
        let locks = home.lock().unwrap();
        home.put(&login("b1"), locks.as_ref()).unwrap()
    });

    let service = ClaudeNative::home(root.path()).storage_dir().service();
    assert!(service.starts_with("Claude Code-credentials-"), "{service}");
    let writes: Vec<_> = sent
        .iter()
        .filter(|command| command.starts_with("add-generic-password"))
        .collect();
    assert_eq!(writes.len(), 1, "{sent:?}");
    assert!(
        writes[0].contains(&format!("-a \"claude-code-user\" -s \"{service}\"")),
        "{}",
        writes[0]
    );
    assert!(!root.path().join(".claude/.credentials.json").exists());
}

/// A home that keeps a login in a file was not made by on-n-off; writing beside it would leave two.
#[cfg(target_os = "macos")]
#[test]
fn on_macos_a_home_whose_login_is_in_a_file_takes_no_new_one() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());
    fs::create_dir_all(root.path().join(".claude")).unwrap();
    fs::write(
        root.path().join(".claude/.credentials.json"),
        login("file").auth.to_string(),
    )
    .unwrap();

    let (result, sent) = keychain(|| {
        let locks = home.lock().unwrap();
        home.put(&login("b1"), locks.as_ref())
    });

    assert!(result.is_err());
    assert!(!sent.iter().any(|c| c.starts_with("add-generic-password")));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn elsewhere_a_homes_login_goes_to_its_credentials_file_as_claude_code_keeps_it() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());

    let locks = home.lock().unwrap();
    home.put(&login("b1"), locks.as_ref()).unwrap();

    let file = fs::read_to_string(root.path().join(".claude/.credentials.json")).unwrap();
    let stored: Value = serde_json::from_str(&file).unwrap();
    assert_eq!(stored["claudeAiOauth"], login("b1").auth["claudeAiOauth"]);
}

#[test]
fn emptying_a_home_signs_it_out_as_claude_code_does_and_keeps_its_account_record() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());

    let ((emptied, config), _) = keychain(|| {
        let locks = home.lock().unwrap();
        home.put(&login("b1"), locks.as_ref()).unwrap();
        home.clear(locks.as_ref()).unwrap();
        drop(locks);
        let config = native::read_json(&root.path().join(".claude/.claude.json")).unwrap();
        (home.read().unwrap(), config)
    });

    assert!(emptied.is_none(), "{:?}", token(emptied));
    // The record says whose home it is; Claude Code's own sign-out keeps it too.
    assert_eq!(config["oauthAccount"], login("b1").account);
}

#[test]
fn emptying_a_home_that_holds_nothing_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());

    let (result, sent) = keychain(|| {
        let locks = home.lock().unwrap();
        home.clear(locks.as_ref())
    });

    result.unwrap();
    assert!(!sent.iter().any(|c| c.starts_with("add-generic-password")));
    assert!(!root.path().join(".claude/.credentials.json").exists());
}

#[test]
fn deleting_a_home_removes_its_login_and_its_directory() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("home");
    let home = ClaudeHome::at(&dir);

    let (read, sent) = keychain(|| {
        let locks = home.lock().unwrap();
        home.put(&login("b1"), locks.as_ref()).unwrap();
        home.delete(locks).unwrap();
        home.read().unwrap()
    });

    assert!(read.is_none());
    assert!(!dir.exists());
    if cfg!(target_os = "macos") {
        assert!(sent
            .iter()
            .any(|c| c.starts_with("delete-generic-password")));
    }
}

/// A home's usage is Claude Code's own report, asked in the home's config dir: the one whose
/// account record names the account.
#[test]
fn a_homes_usage_is_claude_codes_report_asked_in_that_home() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());
    keychain(|| {
        let locks = home.lock().unwrap();
        home.put(&login("b1"), locks.as_ref()).unwrap();
    });
    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        bin.join("report.jsonl"),
        r#"{"type":"assistant","usage_report":{"rate_limits":{"limits":[{"kind":"weekly_all","group":"weekly","percent":34}]}}}"#,
    )
    .unwrap();
    let stub = CliStub::new("claude")
        .log_env("CLAUDE_CONFIG_DIR", "config-dir.txt")
        .stdout_file("report.jsonl")
        .write(&bin);

    let card = crate::accounts::native::with_test_cli(&stub, || home.read_usage(&identity()))
        .unwrap_or_else(|error| panic!("{error:?}"));

    assert_eq!(card.account.unwrap().id, identity().observation_key());
    let config_dir = fs::read_to_string(bin.join("config-dir.txt")).unwrap();
    assert_eq!(Path::new(config_dir.trim()), root.path().join(".claude"));
}

/// A home read back after a write, by a store opened afresh on the same directory, holds the
/// login and says whose it is, as a later read or switch finds it.
#[test]
fn a_homes_login_is_read_back_by_a_later_look_at_the_same_home() {
    let root = tempfile::tempdir().unwrap();

    let (read, _) = keychain(|| {
        let home = ClaudeHome::at(root.path());
        let locks = home.lock().unwrap();
        home.put(&login("b1"), locks.as_ref()).unwrap();
        drop(locks);
        ClaudeHome::at(root.path()).read().unwrap()
    });

    let read = read.expect("the login the home holds");
    assert_eq!(read.auth, login("b1").auth);
    assert_eq!(
        ClaudeHome::at(root.path()).identify(&read).unwrap(),
        identity()
    );
}

#[test]
fn deleting_a_home_that_was_never_made_is_done_at_once() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(&root.path().join("never-made"));

    let (result, _) = keychain(|| home.delete(Box::new(())));

    result.unwrap();
}

/// A home that could not be removed is reported, so it is not forgotten while it still holds a
/// login.
#[cfg(unix)]
#[test]
fn deleting_a_home_that_cannot_be_removed_says_so() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("home");
    let home = ClaudeHome::at(&dir);
    keychain(|| {
        let locks = home.lock().unwrap();
        home.put(&login("b1"), locks.as_ref()).unwrap();
    });
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();

    let (result, _) = keychain(|| home.delete(Box::new(())));

    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert!(dir.exists());
}
