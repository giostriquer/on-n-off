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

fn token(login: Option<Login>) -> Option<String> {
    login?.auth["claudeAiOauth"]["accessToken"]
        .as_str()
        .map(str::to_string)
}

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
