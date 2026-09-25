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
/// no file, a login, no token, broken.
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
            let got = cell_of(read(&ConfigDir::default_in(&home), probe.clone()), &path);
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
            read(&ConfigDir::default_in(&home), Ok(Some("null".to_string()))),
            &path
        ),
        Cell::Read(Some("file-token"), Target::File)
    );
}

/// A guessed account name would file a second Keychain item under the same service, and the read
/// matches on service alone, so it could then return either one.
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

    let writer = PreparedWrite::prepare(&ClaudeStore::File(path)).unwrap();
    assert!(temporary.exists(), "prepared up front, before the grant");
    drop(writer);
    assert!(!temporary.exists());
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
    ClaudeLocks::acquire_at(&ConfigDir::default_in(home), LockScope::Refresh, now)
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

/// The one `security` call made under the lock has to finish well inside the minute after which
/// Claude Code breaks it, or the write races whoever broke it.
#[cfg(target_os = "macos")]
#[test]
fn the_keychain_write_deadline_fits_inside_the_lock_it_is_held_under() {
    assert!(crate::accounts::keychain::DEADLINE < LOCK_STALE);
}
