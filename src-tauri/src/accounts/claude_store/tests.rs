use super::*;
use crate::paths::scratch_dir;

const CLAUDE_JSON: &str = r#"{"claudeAiOauth":{"accessToken":"kc-token","refreshToken":"r","expiresAt":1787022473402,"scopes":["user:inference"],"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}"#;

/// Claude Code's own sign-out leaves this behind: valid JSON, no token.
const SIGNED_OUT: &str = r#"{"claudeAiOauth":{"accessToken":"","refreshToken":"","expiresAt":0}}"#;

fn write(home: &Path, rel: &str, body: &str) {
    let path = home.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// Where a write would go, as `Stored::target` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Keychain,
    File,
    /// The Keychain could not be read, so a write refuses.
    Refused,
}

/// One cell of the store truth table: the token read (`""` for a signed-out login, `None` for no
/// document) and where a write would go, or which store failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Read(Option<&'static str>, Target),
    KeychainError,
    FileMalformed,
}

fn keychain_states() -> [(&'static str, KeychainProbe); 5] {
    [
        ("no item", Ok(None)),
        ("a login", Ok(Some(CLAUDE_JSON.to_string()))),
        ("no token", Ok(Some(SIGNED_OUT.to_string()))),
        ("invalid JSON", Ok(Some("{ not json".to_string()))),
        (
            "unreadable",
            Err("Keychain lookup failed (User canceled the operation.)".to_string()),
        ),
    ]
}

fn file_states() -> [(&'static str, Option<String>); 4] {
    [
        ("no file", None),
        (
            "a login",
            Some(CLAUDE_JSON.replace("kc-token", "file-token")),
        ),
        ("no token", Some(SIGNED_OUT.to_string())),
        ("broken", Some("{ not json".to_string())),
    ]
}

fn cell_of(result: Result<Stored, StoreError>, path: &Path) -> Cell {
    let stored = match result {
        Ok(stored) => stored,
        Err(StoreError::Keychain(why)) => {
            assert!(why.contains("User canceled"), "{why}");
            return Cell::KeychainError;
        }
        Err(StoreError::FileMalformed(why)) => {
            assert!(why.contains(&path.display().to_string()), "{why}");
            return Cell::FileMalformed;
        }
        Err(other) => panic!("unexpected {other:?}"),
    };
    let token = stored.document.as_ref().map(|document| {
        match document["claudeAiOauth"]["accessToken"].as_str() {
            Some("kc-token") => "kc-token",
            Some("file-token") => "file-token",
            Some("") => "",
            other => panic!("unexpected token {other:?}"),
        }
    });
    let target = match stored.target {
        Ok(ClaudeStore::Keychain) => Target::Keychain,
        Ok(ClaudeStore::File(from)) => {
            assert_eq!(from, path);
            Target::File
        }
        Err(why) => {
            assert!(why.contains("User canceled"), "{why}");
            Target::Refused
        }
    };
    Cell::Read(token, target)
}

/// What Claude Code's next read would find, and where a write would go, for every combination of
/// what the Keychain entry and the credentials file hold. Rows are the Keychain, columns the file:
/// no file, a login, no token, broken. The two `KeychainError` cells are the intended difference
/// from Claude Code, which reads them as signed out.
#[test]
fn the_store_truth_table() {
    use Cell::{FileMalformed, KeychainError, Read};
    use Target::{File, Keychain, Refused};
    let from_file = [
        Read(None, File),
        Read(Some("file-token"), File),
        Read(Some(""), File),
        FileMalformed,
    ];
    let expected = [
        from_file,
        [Read(Some("kc-token"), Keychain); 4],
        [Read(Some(""), Keychain); 4],
        from_file,
        [
            KeychainError,
            Read(Some("file-token"), Refused),
            Read(Some(""), Refused),
            KeychainError,
        ],
    ];
    for ((keychain, probe), row) in keychain_states().into_iter().zip(expected) {
        for ((file, contents), want) in file_states().into_iter().zip(row) {
            let home = scratch_dir("store-truth");
            let path = home.join(".claude").join(".credentials.json");
            if let Some(contents) = contents {
                write(&home, ".claude/.credentials.json", &contents);
            }
            let got = cell_of(read(&StorageDir::default_in(&home), probe.clone()), &path);
            assert_eq!(got, want, "Keychain: {keychain}; file: {file}");
        }
    }
}

/// A Keychain entry holding JSON `null` is one Claude Code reads past, as if there were none.
#[test]
fn a_keychain_entry_of_null_leaves_the_read_to_the_file() {
    let home = scratch_dir("store-null");
    write(
        &home,
        ".claude/.credentials.json",
        &CLAUDE_JSON.replace("kc-token", "file-token"),
    );
    let path = home.join(".claude").join(".credentials.json");
    assert_eq!(
        cell_of(
            read(&StorageDir::default_in(&home), Ok(Some("null".to_string()))),
            &path
        ),
        Cell::Read(Some("file-token"), Target::File)
    );
}

/// When Claude Code's own account name finds no item, the one the service holds is addressed by
/// the account its attributes name, never by a guess that would file a second item beside it.
#[test]
fn the_keychain_account_is_read_off_the_entry_rather_than_guessed() {
    let dump = "keychain: \"/Users/me/Library/Keychains/login.keychain-db\"\n\
        attributes:\n    \
        \"acct\"<blob>=\"me.example\"\n    \
        \"svce\"<blob>=\"Claude Code-credentials\"\n";
    assert_eq!(parse_keychain_account(dump).as_deref(), Some("me.example"));
    assert_eq!(parse_keychain_account("attributes:\n"), None);
    assert_eq!(parse_keychain_account("    \"acct\"<blob>=\"\"\n"), None);
}

#[test]
fn disposable_home_does_not_read_or_renew_the_real_keychain_login() {
    let calls = std::cell::Cell::new(0);
    let result = isolated_keychain(true, || {
        calls.set(calls.get() + 1);
        Ok(Some("real-native-credential".into()))
    });
    assert_eq!(result, Ok(None));
    assert_eq!(calls.get(), 0);
}

/// A prepared write that is never committed takes its temporary with it. Leaving one behind would
/// park a live refresh token in a file Claude Code neither knows about nor rotates, which is the
/// same objection that keeps `ConfigIo` out of this module.
#[test]
fn an_abandoned_write_leaves_no_temporary_holding_a_token() {
    let home = scratch_dir("renew-temp");
    write(&home, ".claude/.credentials.json", CLAUDE_JSON);
    let path = home.join(".claude").join(".credentials.json");
    let temporary = path.with_extension("json.on-n-off");

    let storage_write = home.join(".claude").join(".storage-write.lock");

    let writer =
        PreparedWrite::prepare(&StorageDir::default_in(&home), &ClaudeStore::File(path)).unwrap();
    assert!(temporary.exists(), "prepared up front, before the grant");
    assert!(
        storage_write.is_dir(),
        "Claude Code's credentials are locked from the preparation on"
    );
    drop(writer);
    assert!(!temporary.exists());
    assert!(!storage_write.exists());
}

/// The renewed login lands in a file only this user can read.
#[test]
fn the_credentials_file_is_written_private() {
    let home = scratch_dir("renew-file");
    let path = home.join(".claude").join(".credentials.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    write_private(&path, r#"{"claudeAiOauth":{"accessToken":"new"}}"#).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        r#"{"claudeAiOauth":{"accessToken":"new"}}"#
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the file holds a refresh token");
    }
}

fn refresh_lock(home: &Path, now: SystemTime) -> Result<ClaudeLocks, LockError> {
    ClaudeLocks::acquire_at(&StorageDir::default_in(home), LockScope::Refresh, now)
}

#[test]
fn the_refresh_lock_admits_one_holder_and_frees_both_paths_on_drop() {
    let home = scratch_dir("renew-lock");
    let paths = [
        home.join(".claude").join(".oauth_refresh.lock"),
        home.join(".claude.lock"),
    ];

    let held = refresh_lock(&home, SystemTime::now()).unwrap();
    assert!(paths.iter().all(|path| path.is_dir()));
    assert_eq!(
        refresh_lock(&home, SystemTime::now()).unwrap_err(),
        LockError::Busy
    );

    drop(held);
    assert!(
        paths.iter().all(|path| !path.exists()),
        "a released lock leaves nothing behind for the next renewal to break"
    );
    refresh_lock(&home, SystemTime::now()).unwrap();
}

/// A process killed mid-renewal leaves its lock directory behind. Claude Code breaks one older
/// than a minute rather than never refreshing again, and so must this.
#[test]
fn a_lock_left_behind_by_a_dead_process_is_broken_once_it_goes_stale() {
    let home = scratch_dir("renew-stale");
    let abandoned = home.join(".claude").join(".oauth_refresh.lock");
    fs::create_dir_all(&abandoned).unwrap();
    assert_eq!(
        refresh_lock(&home, SystemTime::now()).unwrap_err(),
        LockError::Busy
    );

    let past_stale = SystemTime::now() + LOCK_STALE + Duration::from_secs(5);
    assert!(refresh_lock(&home, past_stale).is_ok());
}

/// The refresh lock is taken first. When the legacy lock beside the config home is held, the
/// renewal yields and gives back the one it had already taken.
#[test]
fn a_held_legacy_lock_yields_and_releases_the_refresh_lock_already_taken() {
    let home = scratch_dir("renew-legacy-held");
    fs::create_dir_all(home.join(".claude.lock")).unwrap();

    assert_eq!(
        refresh_lock(&home, SystemTime::now()).unwrap_err(),
        LockError::Busy
    );
    assert!(!home.join(".claude").join(".oauth_refresh.lock").exists());
    assert!(
        home.join(".claude.lock").is_dir(),
        "another holder's lock is theirs to release"
    );
}

fn backdate(path: &Path, seconds: u64) {
    let then = SystemTime::now() - Duration::from_secs(seconds);
    filetime::set_file_mtime(path, filetime::FileTime::from_system_time(then)).unwrap();
}

fn fresh(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .is_ok_and(|at| at.elapsed().unwrap_or_default() < Duration::from_secs(60))
}

/// A renewal can hold the refresh locks past Claude Code's minute: the Keychain prompt alone may
/// take ninety seconds. Kept fresh while held, the locks are never judged abandoned under it.
#[test]
fn the_refresh_locks_are_kept_fresh_while_held() {
    let home = scratch_dir("renew-heartbeat");
    let held = refresh_lock(&home, SystemTime::now()).unwrap();
    let paths = [
        home.join(".claude").join(".oauth_refresh.lock"),
        home.join(".claude.lock"),
    ];
    for path in &paths {
        backdate(path, 3600);
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !paths.iter().all(|path| fresh(path)) {
        assert!(
            std::time::Instant::now() < deadline,
            "the refresh locks were not touched while held"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(held);
}

/// Claude Code takes its legacy lock beside the config dir's real path, so a config dir reached
/// through a link locks the same directory Claude Code does.
#[cfg(unix)]
#[test]
fn the_legacy_lock_sits_beside_the_real_config_dir() {
    let home = scratch_dir("renew-linked");
    fs::create_dir_all(home.join("dotfiles").join("claude")).unwrap();
    std::os::unix::fs::symlink(home.join("dotfiles").join("claude"), home.join(".claude")).unwrap();

    let held = refresh_lock(&home, SystemTime::now()).unwrap();
    assert!(home.join("dotfiles").join("claude.lock").is_dir());
    assert!(!home.join(".claude.lock").exists());
    drop(held);
    assert!(!home.join("dotfiles").join("claude.lock").exists());
}

/// The lock taken first is the last one given back, so a process waiting on it never finds the
/// others still held behind it.
#[test]
fn locks_are_released_innermost_first() {
    let held = [
        PathBuf::from("refresh"),
        PathBuf::from("legacy"),
        PathBuf::from("config"),
    ];
    let mut order = Vec::new();
    release(&held, |path| order.push(path.to_path_buf()));
    assert_eq!(
        order,
        [
            PathBuf::from("config"),
            PathBuf::from("legacy"),
            PathBuf::from("refresh")
        ]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_keychain_write_deadline_fits_inside_the_lock_it_is_held_under() {
    assert!(crate::accounts::keychain::DEADLINE < LOCK_STALE);
}

/// The account name Claude Code files its Keychain entry under: `$USER`, else the login name, and
/// a fixed name when that is not one `security` can take as-is.
#[test]
fn claude_codes_own_account_name() {
    let named = |vars: &[(&str, &str)]| {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        claude_code_account(&move |name: &str| {
            vars.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| std::ffi::OsString::from(value))
        })
    };
    assert_eq!(named(&[("USER", "me.example")]), "me.example");
    assert_eq!(named(&[("USER", "me"), ("LOGNAME", "other")]), "me");
    assert_eq!(named(&[("LOGNAME", "me")]), "me");
    assert_eq!(
        named(&[("USER", ""), ("LOGNAME", "me")]),
        "me",
        "an empty $USER is no name"
    );
    for unusable in ["me@example.com", "two words", "quo\"te"] {
        assert_eq!(
            named(&[("USER", unusable)]),
            "claude-code-user",
            "{unusable}"
        );
    }
    assert_eq!(named(&[]), "claude-code-user");
}

/// The renewal writes back under the account of the item Claude Code reads: its own, when there
/// is one, not whichever item `security` returns for the service.
#[cfg(target_os = "macos")]
#[test]
fn the_renewal_writes_the_item_filed_under_claude_codes_own_account() {
    use crate::accounts::keychain::{fake_items, with_test_runner};
    const TWO_ITEMS: &[(&str, &str)] = &[("claude-code-user", "{}"), ("other", "{}")];

    let (committed, sent) = with_test_runner(fake_items(TWO_ITEMS), || {
        let dir = StorageDir::default_in(&scratch_dir("renew-keychain-account"));
        let write = PreparedWrite::prepare(&dir, &ClaudeStore::Keychain).unwrap();
        write.commit(r#"{"claudeAiOauth":{}}"#)
    });
    assert_eq!(committed, Ok(()));
    let add = sent
        .iter()
        .find(|command| command.starts_with("add-generic-password"))
        .expect("the renewal is written to the Keychain");
    assert!(
        add.starts_with(
            "add-generic-password -U -a \"claude-code-user\" -s \"Claude Code-credentials\""
        ),
        "{add}"
    );
}

/// An environment holding exactly `vars`.
fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<std::ffi::OsString> {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect();
    move |name| {
        vars.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| std::ffi::OsString::from(value))
    }
}

/// Claude Code 2.1.282's config dir is `CLAUDE_CONFIG_DIR` exactly as set, NFC-normalized, else
/// `~/.claude`; set, it scopes the Keychain entry by a hash of that path. The hashes here were
/// worked out by hand from the literal paths.
#[test]
fn the_config_dir_is_claude_config_dir_as_claude_code_reads_it() {
    let home = Path::new("/Users/me");
    for (value, config, service) in [
        (
            "/Users/me/.claude-work",
            "/Users/me/.claude-work",
            "Claude Code-credentials-1e91dd84",
        ),
        (
            "/Users/me/claude ",
            "/Users/me/claude ",
            "Claude Code-credentials-355aaf04",
        ),
        (
            "/Users/me/cafe\u{301}",
            "/Users/me/caf\u{e9}",
            "Claude Code-credentials-12e72a48",
        ),
    ] {
        let dirs = dirs(home, &env(&[("CLAUDE_CONFIG_DIR", value)])).unwrap();
        assert_eq!(dirs.config, PathBuf::from(config), "{value:?}");
        assert!(dirs.custom, "{value:?}");
        assert_eq!(dirs.storage(), StorageDir::new(PathBuf::from(config), true));
        assert_eq!(dirs.storage().service(), service, "{value:?}");
    }

    let default = dirs(home, &env(&[])).unwrap();
    assert_eq!(default.config, PathBuf::from("/Users/me/.claude"));
    assert!(!default.custom);
    assert_eq!(default.storage().service(), "Claude Code-credentials");
}

/// Set but empty is still set: Claude Code would use the empty path, relative to wherever it runs,
/// which on-n-off cannot know, so it refuses rather than read some other store.
#[test]
fn an_empty_or_relative_config_dir_is_refused() {
    let home = Path::new("/Users/me");
    for value in ["", "claude-work", " /Users/me/.claude"] {
        assert_eq!(
            dirs(home, &env(&[("CLAUDE_CONFIG_DIR", value)])).err(),
            Some("The provider home must be an absolute path.".to_string()),
            "{value:?}"
        );
    }
}

/// A disposable home never follows the environment to a real store.
#[test]
fn a_disposable_home_keeps_the_default_dirs_whatever_the_environment_says() {
    let home = Path::new("/Users/me");
    let dirs = dirs(
        home,
        &env(&[
            ("ON_N_OFF_HOME", "/Users/me"),
            ("CLAUDE_CONFIG_DIR", "/Users/me/.claude-work"),
        ]),
    )
    .unwrap();
    assert_eq!(dirs.config, PathBuf::from("/Users/me/.claude"));
    assert!(!dirs.custom);
    assert_eq!(dirs.storage(), StorageDir::default_in(home));
}

/// `CLAUDE_SECURESTORAGE_CONFIG_DIR` moves Claude Code's storage — the credentials file, the lock
/// directories and the path that names the Keychain entry — and leaves the config dir where it
/// was. Set but empty, it puts the storage back in `~/.claude` under the unscoped entry.
#[test]
fn the_secure_storage_dir_moves_the_store_and_leaves_the_config_dir() {
    let home = Path::new("/Users/me");
    for (vars, config, storage, service) in [
        (
            vec![("CLAUDE_SECURESTORAGE_CONFIG_DIR", "/Users/me/secure")],
            "/Users/me/.claude",
            "/Users/me/secure",
            "Claude Code-credentials-8fb5187b",
        ),
        (
            vec![
                ("CLAUDE_CONFIG_DIR", "/Users/me/.claude-work"),
                ("CLAUDE_SECURESTORAGE_CONFIG_DIR", "/Users/me/secure"),
            ],
            "/Users/me/.claude-work",
            "/Users/me/secure",
            "Claude Code-credentials-8fb5187b",
        ),
        (
            vec![
                ("CLAUDE_CONFIG_DIR", "/Users/me/.claude-work"),
                ("CLAUDE_SECURESTORAGE_CONFIG_DIR", ""),
            ],
            "/Users/me/.claude-work",
            "/Users/me/.claude",
            "Claude Code-credentials",
        ),
        (
            vec![(
                "CLAUDE_SECURESTORAGE_CONFIG_DIR",
                "/Users/me/secure-cafe\u{301}",
            )],
            "/Users/me/.claude",
            "/Users/me/secure-caf\u{e9}",
            "Claude Code-credentials-d5d5ac9f",
        ),
    ] {
        let dirs = dirs(home, &env(&vars)).unwrap();
        assert_eq!(dirs.config, PathBuf::from(config), "{vars:?}");
        assert_eq!(
            dirs.storage().credentials_file(),
            Path::new(storage).join(".credentials.json"),
            "{vars:?}"
        );
        assert_eq!(dirs.storage().service(), service, "{vars:?}");
    }

    assert_eq!(
        dirs(home, &env(&[("CLAUDE_SECURESTORAGE_CONFIG_DIR", "secure")])).err(),
        Some("The provider home must be an absolute path.".to_string())
    );
    let disposable = dirs(
        home,
        &env(&[
            ("ON_N_OFF_HOME", "/Users/me"),
            ("CLAUDE_SECURESTORAGE_CONFIG_DIR", "/Users/me/secure"),
        ]),
    )
    .unwrap();
    assert_eq!(disposable.storage(), StorageDir::default_in(home));
}
