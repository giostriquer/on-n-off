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

fn keychain<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    keychain_watching(|| {}, run)
}

#[cfg(target_os = "macos")]
fn keychain_watching<T>(
    deleting: impl Fn() + 'static,
    run: impl FnOnce() -> T,
) -> (T, Vec<String>) {
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
            deleting();
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
fn keychain_watching<T>(
    _deleting: impl Fn() + 'static,
    run: impl FnOnce() -> T,
) -> (T, Vec<String>) {
    (run(), Vec::new())
}

fn accounts_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".on-n-off/accounts")).unwrap();
    crate::accounts::vault::tests::unlock_fixture(&root);
    root
}

fn unnamed_home(root: &Path) -> (PathBuf, ClaudeHome) {
    let dir = crate::accounts::homes::dir(root, &uuid::Uuid::new_v4().to_string()).unwrap();
    let home = ClaudeHome::at(&dir);
    (dir, home)
}

fn settle(root: &Path, provider: AgentId) {
    let accounts = crate::accounts::Accounts::at(root.to_path_buf());
    let homes = accounts.account_homes(provider).unwrap();
    let open = || crate::accounts::store::Store::open_existing(root);
    crate::accounts::homes::settle(root, provider, None, &open, &homes);
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
        .unwrap_or_else(|error| panic!("{error:?}"))
        .into_card();

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

#[test]
fn a_homes_sign_in_says_when_it_ends_until_the_home_is_emptied() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());
    let dated = |access: &str, refresh: &str| {
        let mut login = login("b1");
        login.auth["claudeAiOauth"]["accessToken"] = access.into();
        login.auth["claudeAiOauth"]["refreshToken"] = refresh.into();
        login.auth["claudeAiOauth"]["refreshTokenExpiresAt"] = 1_792_081_900_000_i64.into();
        login
    };
    let ends = Some(SignInDeadline {
        identity: identity(),
        ends_at_ms: 1_792_081_900_000,
    });

    let ((live, ended, emptied), _) = keychain(|| {
        let locks = home.lock().unwrap();
        home.put(&dated("access-b1", "refresh-b1"), locks.as_ref())
            .unwrap();
        let live = home.sign_in_deadline();
        let back = home.put(&dated("", ""), locks.as_ref()).unwrap();
        assert!(back.is_none(), "Claude Code emptied the login");
        let ended = home.sign_in_deadline();
        home.clear(locks.as_ref()).unwrap();
        (live, ended, home.sign_in_deadline())
    });

    assert_eq!(live, ends);
    assert_eq!(ended, ends, "an emptied login keeps the date it ended at");
    assert_eq!(emptied, None, "on-n-off's sign-out keeps no date");
}

fn touched(file: &std::path::Path, seconds: u64) {
    std::fs::File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds))
        .unwrap();
}

#[test]
fn a_homes_login_stamp_follows_its_credentials_file_without_reading_it() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());
    let file = root.path().join(".claude/.credentials.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let (first_login, renewed_login) = (login("b1").auth.to_string(), login("b2").auth.to_string());
    assert_eq!(first_login.len(), renewed_login.len());

    let ((empty, first, again, unreadable, renewed), sent) = keychain(|| {
        let empty = home.login_stamp();
        std::fs::write(&file, &first_login).unwrap();
        touched(&file, 1_800_000_000);
        let first = home.login_stamp();
        let again = home.login_stamp();
        #[cfg(unix)]
        let unreadable = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
            let stamp = home.login_stamp();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            stamp
        };
        #[cfg(not(unix))]
        let unreadable = home.login_stamp();
        std::fs::write(&file, &renewed_login).unwrap();
        touched(&file, 1_800_000_060);
        (empty, first, again, unreadable, home.login_stamp())
    });

    assert!(empty.is_ok() && first.is_ok());
    assert_ne!(empty, first, "a login written into the home");
    assert_eq!(first, again);
    assert_eq!(unreadable, first, "the stamp never opens the file");
    assert_ne!(renewed, first, "a renewed login of the same length");
    assert!(
        sent.iter().all(|command| !command.contains(" -w")),
        "{sent:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_home_whose_store_cannot_be_looked_at_has_no_stamp() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());
    let config = root.path().join(".claude");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join(".credentials.json"),
        login("b1").auth.to_string(),
    )
    .unwrap();

    let (stamp, _) = keychain(|| {
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o000)).unwrap();
        let stamp = home.login_stamp();
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o755)).unwrap();
        stamp
    });

    assert!(stamp.is_err(), "{stamp:?}");
}

#[cfg(target_os = "macos")]
#[test]
fn a_homes_login_stamp_reads_its_keychain_items_attributes_and_never_its_secret() {
    use crate::process::CommandOutcome;
    use std::{cell::Cell, rc::Rc};
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());
    let written = Rc::new(Cell::new(1));
    let item = written.clone();
    let runner = move |command: &str| {
        if command.contains(" -a ") {
            return CommandOutcome::Exited {
                success: false,
                stdout: String::new(),
                stderr: "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain.".into(),
            };
        }
        CommandOutcome::Exited {
            success: !command.contains(" -w "),
            stdout: format!(
                "    \"acct\"<blob>=\"someone-else\"\n    \"mdat\"<timedate>=\"2026102010000{}Z\"\n",
                item.get()
            ),
            stderr: String::new(),
        }
    };

    let ((first, again, rewritten), sent) =
        crate::accounts::keychain::with_test_runner(runner, || {
            let first = home.login_stamp();
            let again = home.login_stamp();
            written.set(2);
            (first, again, home.login_stamp())
        });

    assert!(first.is_ok());
    assert_eq!(first, again);
    assert_ne!(
        rewritten, first,
        "the item filed under another account was rewritten"
    );
    assert!(
        sent.iter().any(|command| !command.contains(" -a ")),
        "{sent:?}"
    );
    assert!(
        sent.iter().all(|command| command.starts_with("find-generic-password")
            && !command.contains(" -w")),
        "{sent:?}"
    );
}

#[test]
fn a_muse_read_leaves_an_unnamed_claude_home_with_its_login() {
    let root = accounts_root();
    let (dir, home) = unnamed_home(root.path());

    let (left, sent) = keychain(|| {
        let locks = home.lock().unwrap();
        home.put(&login("b1"), locks.as_ref()).unwrap();
        drop(locks);
        settle(root.path(), AgentId::Muse);
        home.read().unwrap()
    });

    assert!(
        dir.join(".claude").is_dir(),
        "a Muse read removed a Claude home"
    );
    assert_eq!(token(left), Some("access-b1".into()));
    assert!(
        !sent
            .iter()
            .any(|command| command.starts_with("delete-generic-password")),
        "{sent:?}"
    );
}

#[test]
fn a_claude_read_tears_down_an_unnamed_home_its_login_before_its_directory() {
    let root = accounts_root();
    let (dir, home) = unnamed_home(root.path());
    let existed: std::rc::Rc<std::cell::RefCell<Vec<bool>>> = std::rc::Rc::default();
    let watched = (existed.clone(), dir.clone());

    let (left, sent) = keychain_watching(
        move || watched.0.borrow_mut().push(watched.1.exists()),
        || {
            let locks = home.lock().unwrap();
            home.put(&login("b1"), locks.as_ref()).unwrap();
            drop(locks);
            settle(root.path(), AgentId::Claude);
            home.read().unwrap()
        },
    );

    assert!(!dir.exists(), "the home stayed");
    assert_eq!(token(left), None, "its login outlived its home");
    #[cfg(target_os = "macos")]
    {
        let service = StorageDir::new(dir.join(".claude"), true).service();
        let deletes: Vec<_> = sent
            .iter()
            .filter(|command| command.starts_with("delete-generic-password"))
            .collect();
        assert_eq!(deletes.len(), 1, "{sent:?}");
        assert!(
            deletes[0].contains(&format!("-s \"{service}\"")),
            "{}",
            deletes[0]
        );
        assert_eq!(
            *existed.borrow(),
            [true],
            "the directory went before its Keychain entry"
        );
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (sent, existed);
}

#[test]
fn a_home_is_known_by_its_claude_config_dir_and_never_by_its_login() {
    let root = tempfile::tempdir().unwrap();
    let home = ClaudeHome::at(root.path());
    assert!(!home.recognizes(), "an empty home");
    fs::create_dir_all(root.path().join("config/muse")).unwrap();
    assert!(!home.recognizes(), "a Muse home");

    let (known, sent) = keychain(|| {
        let _locks = home.lock().unwrap();
        home.recognizes()
    });

    assert!(known);
    assert!(sent.is_empty(), "{sent:?}");
}

#[cfg(unix)]
#[test]
fn a_linked_claude_config_dir_is_not_a_homes_layout() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), root.path().join(".claude")).unwrap();

    assert!(!ClaudeHome::at(root.path()).recognizes());
}

#[test]
fn a_claude_read_leaves_an_unnamed_muse_home_to_muse() {
    let root = accounts_root();
    let (dir, claude) = unnamed_home(root.path());
    let muse = crate::accounts::muse::MuseHome::at(&dir);
    let login = Login {
        auth: json!({"access_token": "t1", "api_key": "key-1"}),
        account: json!({"mechanism": "oauth", "user_email": "you@example.com"}),
    };
    muse.put(&login, &()).unwrap();

    let ((), sent) = keychain(|| settle(root.path(), AgentId::Claude));

    assert!(sent.is_empty(), "{sent:?}");
    assert!(
        !dir.join(".claude").exists(),
        "a Claude read laid its config dir into a Muse home"
    );
    assert!(muse.recognizes() && !claude.recognizes());
    settle(root.path(), AgentId::Muse);
    assert!(!dir.exists(), "its owner, Muse, did not tear it down");
}
