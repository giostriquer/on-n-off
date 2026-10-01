use super::*;
use crate::paths::scratch_dir;

const CLAUDE_JSON: &str = r#"{"claudeAiOauth":{"accessToken":"kc-token","refreshToken":"r","expiresAt":1787022473402,"scopes":["user:inference"],"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}"#;

const SIGNED_OUT: &str = r#"{"claudeAiOauth":{"accessToken":"","refreshToken":"","expiresAt":0}}"#;

impl StorageDir {
    fn default_in(home: &Path) -> Self {
        Self::new(home.join(".claude"), false)
    }
}

fn write(home: &Path, rel: &str, body: &str) {
    let path = home.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Keychain,
    File,
    Refused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Read(Option<&'static str>, Target),
    KeychainError,
    FileMalformed,
    FileUnreadable,
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

const A_DIRECTORY: &str = "<a directory>";

fn file_states() -> [(&'static str, Option<String>); 5] {
    [
        ("no file", None),
        (
            "a login",
            Some(CLAUDE_JSON.replace("kc-token", "file-token")),
        ),
        ("no token", Some(SIGNED_OUT.to_string())),
        ("broken", Some("{ not json".to_string())),
        ("unreadable", Some(A_DIRECTORY.to_string())),
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
        Err(StoreError::FileUnreadable(why)) => {
            assert!(why.contains(&path.display().to_string()), "{why}");
            return Cell::FileUnreadable;
        }
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

#[test]
fn the_store_truth_table() {
    use Cell::{FileMalformed, FileUnreadable, KeychainError, Read};
    use Target::{File, Keychain, Refused};
    let from_file = [
        Read(None, File),
        Read(Some("file-token"), File),
        Read(Some(""), File),
        FileMalformed,
        FileUnreadable,
    ];
    let expected = [
        from_file,
        [Read(Some("kc-token"), Keychain); 5],
        [Read(Some(""), Keychain); 5],
        from_file,
        [
            KeychainError,
            Read(Some("file-token"), Refused),
            Read(Some(""), Refused),
            KeychainError,
            KeychainError,
        ],
    ];
    for ((keychain, probe), row) in keychain_states().into_iter().zip(expected) {
        for ((file, contents), want) in file_states().into_iter().zip(row) {
            let home = scratch_dir("store-truth");
            let path = home.join(".claude").join(".credentials.json");
            match contents.as_deref() {
                Some(A_DIRECTORY) => fs::create_dir_all(&path).unwrap(),
                Some(contents) => write(&home, ".claude/.credentials.json", contents),
                None => {}
            }
            let got = cell_of(read(&StorageDir::default_in(&home), probe.clone()), &path);
            assert_eq!(got, want, "Keychain: {keychain}; file: {file}");
        }
    }
}

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
fn an_abandoned_write_leaves_no_temporary_holding_a_token() {
    let home = scratch_dir("renew-temp");
    write(&home, ".claude/.credentials.json", CLAUDE_JSON);
    let dir = home.join(".claude");
    let files = || {
        fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .count()
    };
    let storage_write = dir.join(".storage-write.lock");
    let never_lost = || false;

    let (document, pending) = begin(
        &StorageDir::default_in(&home),
        &|_: &StorageDir| Ok(None),
        &never_lost,
    )
    .unwrap();
    assert_eq!(
        document.unwrap()["claudeAiOauth"]["accessToken"],
        "kc-token"
    );
    assert_eq!(files(), 1, "nothing is created before the write is proven");
    let writer = pending.prove().unwrap();
    assert_eq!(
        files(),
        2,
        "the temporary is created up front, before the grant"
    );
    let temporary = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .find(|name| name != ".credentials.json" && !name.ends_with(".lock"))
        .unwrap();
    assert!(
        temporary.starts_with(".credentials.json.on-n-off."),
        "a stray left by a kill between write and rename says whose it is: {temporary}"
    );
    assert!(
        storage_write.is_dir(),
        "Claude Code's credentials are locked from the read on"
    );
    drop(writer);
    assert_eq!(files(), 1, "only the credentials file is left");
    assert!(!storage_write.exists());
}

#[test]
fn the_credentials_file_is_written_private() {
    let home = scratch_dir("renew-file");
    let path = home.join(".claude").join(".credentials.json");
    let never_lost = || false;

    let (document, pending) = begin(
        &StorageDir::default_in(&home),
        &|_: &StorageDir| Ok(None),
        &never_lost,
    )
    .unwrap();
    assert_eq!(document, None);
    pending
        .prove()
        .unwrap()
        .commit(&serde_json::json!({"claudeAiOauth":{"accessToken":"new"}}))
        .unwrap();
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

#[cfg(unix)]
#[test]
fn the_legacy_lock_sits_beside_the_real_config_dir() {
    let home = scratch_dir("renew-linked");
    fs::create_dir_all(home.join("dotfiles").join("claude")).unwrap();
    std::os::unix::fs::symlink(home.join("dotfiles").join("claude"), home.join(".claude")).unwrap();

    let held = ClaudeLocks::acquire(
        &StorageDir::default_in(&home),
        LockScope::RefreshAndConfig(&home.join(".claude.json")),
    )
    .unwrap();
    assert!(home.join("dotfiles").join("claude.lock").is_dir());
    assert!(!home.join(".claude.lock").exists());
    drop(held);
    assert!(!home.join("dotfiles").join("claude.lock").exists());
}

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

#[cfg(target_os = "macos")]
#[test]
fn a_credential_write_goes_to_the_item_filed_under_claude_codes_own_account() {
    use crate::accounts::keychain::{fake_items, with_test_runner};
    const TWO_ITEMS: &[(&str, &str)] = &[("claude-code-user", "{}"), ("other", "{}")];

    let (committed, sent) = with_test_runner(fake_items(TWO_ITEMS), || {
        let dir = StorageDir::default_in(&scratch_dir("renew-keychain-account"));
        let never_lost = || false;
        let keychain = |_: &StorageDir| Ok(Some("{}".to_string()));
        let (_, pending) = begin(&dir, &keychain, &never_lost).unwrap();
        pending
            .prove()
            .unwrap()
            .commit(&serde_json::json!({"claudeAiOauth":{}}))
    });
    assert_eq!(committed, Ok(()));
    let add = sent
        .iter()
        .find(|command| command.starts_with("add-generic-password"))
        .expect("the write goes to the Keychain");
    assert!(
        add.starts_with(
            "add-generic-password -U -a \"claude-code-user\" -s \"Claude Code-credentials\""
        ),
        "{add}"
    );
}

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

fn env_os(vars: Vec<(&'static str, std::ffi::OsString)>) -> impl Fn(&str) -> Option<OsString> {
    move |name| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.clone())
    }
}

fn storage_of(dirs: &Dirs) -> StorageDir {
    StorageDir::of(&dirs.config, dirs.custom, dirs.secure_storage.as_ref())
}

fn scoped_service(path: &Path) -> String {
    let hash = crate::sha::sha256_hex(path.to_str().unwrap().as_bytes());
    format!("{CLAUDE_KEYCHAIN_SERVICE}-{}", &hash[..8])
}

fn with_suffix(path: &Path, suffix: &str) -> OsString {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    value
}

#[test]
fn the_config_dir_follows_claude_config_dir_on_every_platform() {
    let home = scratch_dir("config-dir");
    let work = home.join(".claude-work");
    for (value, config) in [
        (work.clone().into_os_string(), work.clone()),
        (
            with_suffix(&home.join("claude"), " "),
            PathBuf::from(with_suffix(&home.join("claude"), " ")),
        ),
        (
            home.join("cafe\u{301}").into_os_string(),
            home.join("caf\u{e9}"),
        ),
    ] {
        let dirs = dirs(&home, &env_os(vec![("CLAUDE_CONFIG_DIR", value.clone())])).unwrap();
        assert_eq!(dirs.config, config, "{value:?}");
        assert!(dirs.custom, "{value:?}");
        assert_eq!(
            storage_of(&dirs).credentials_file(),
            config.join(".credentials.json")
        );
        assert_eq!(
            storage_of(&dirs).service(),
            scoped_service(&config),
            "{value:?}"
        );
    }

    let default = dirs(&home, &env_os(vec![])).unwrap();
    assert_eq!(default.config, home.join(".claude"));
    assert!(!default.custom);
    assert_eq!(storage_of(&default).service(), CLAUDE_KEYCHAIN_SERVICE);
}

#[test]
fn the_secure_storage_dir_moves_the_store_on_every_platform() {
    let home = scratch_dir("secure-storage");
    let work = home.join(".claude-work").into_os_string();
    let secure = home.join("secure");
    for (vars, config, storage, service) in [
        (
            vec![(SECURE_STORAGE_VAR, secure.clone().into_os_string())],
            home.join(".claude"),
            secure.clone(),
            scoped_service(&secure),
        ),
        (
            vec![
                ("CLAUDE_CONFIG_DIR", work.clone()),
                (SECURE_STORAGE_VAR, secure.clone().into_os_string()),
            ],
            home.join(".claude-work"),
            secure.clone(),
            scoped_service(&secure),
        ),
        (
            vec![
                ("CLAUDE_CONFIG_DIR", work.clone()),
                (SECURE_STORAGE_VAR, OsString::new()),
            ],
            home.join(".claude-work"),
            home.join(".claude"),
            CLAUDE_KEYCHAIN_SERVICE.to_string(),
        ),
    ] {
        let dirs = dirs(&home, &env_os(vars.clone())).unwrap();
        assert_eq!(dirs.config, config, "{vars:?}");
        assert_eq!(
            storage_of(&dirs).credentials_file(),
            storage.join(".credentials.json"),
            "{vars:?}"
        );
        assert_eq!(storage_of(&dirs).service(), service, "{vars:?}");
    }

    assert_eq!(
        dirs(&home, &env_os(vec![(SECURE_STORAGE_VAR, "secure".into())])).err(),
        Some("The provider home must be an absolute path.".to_string())
    );
    let disposable = dirs(
        &home,
        &env_os(vec![
            ("ON_N_OFF_HOME", home.clone().into_os_string()),
            (SECURE_STORAGE_VAR, secure.into_os_string()),
        ]),
    )
    .unwrap();
    assert_eq!(storage_of(&disposable), StorageDir::default_in(&home));
}

#[cfg(unix)]
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
        assert_eq!(
            storage_of(&dirs),
            StorageDir::new(PathBuf::from(config), true)
        );
        assert_eq!(storage_of(&dirs).service(), service, "{value:?}");
    }

    let default = dirs(home, &env(&[])).unwrap();
    assert_eq!(default.config, PathBuf::from("/Users/me/.claude"));
    assert!(!default.custom);
    assert_eq!(storage_of(&default).service(), "Claude Code-credentials");
}

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
    assert_eq!(storage_of(&dirs), StorageDir::default_in(home));
}

#[cfg(unix)]
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
            storage_of(&dirs).credentials_file(),
            Path::new(storage).join(".credentials.json"),
            "{vars:?}"
        );
        assert_eq!(storage_of(&dirs).service(), service, "{vars:?}");
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
    assert_eq!(storage_of(&disposable), StorageDir::default_in(home));
}

#[test]
fn a_credential_write_knows_when_any_lock_it_relies_on_is_lost() {
    let home = scratch_dir("write-lost");
    let dir = StorageDir::default_in(&home);
    let caller_lost = std::cell::Cell::new(false);
    let held = || caller_lost.get();
    let (_, pending) = begin(&dir, &|_: &StorageDir| Ok(None), &held).unwrap();
    let write = pending.prove().unwrap();
    assert!(!write.lost());

    caller_lost.set(true);
    assert!(write.lost(), "the caller's lock");
    caller_lost.set(false);

    fs::remove_dir(home.join(".claude").join(".storage-write.lock")).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !write.lost() {
        assert!(
            std::time::Instant::now() < deadline,
            "the storage-write lock's heartbeat never noticed it gone"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn claude_codes_locks_in_its_order_and_with_its_staleness() {
    let home = scratch_dir("lock-paths");
    let config_dir = home.join(".claude");
    fs::create_dir_all(&config_dir).unwrap();
    let dir = StorageDir::default_in(&home);
    let mut legacy = fs::canonicalize(&config_dir).unwrap().into_os_string();
    legacy.push(".lock");
    let refresh = vec![
        (
            config_dir.join(".oauth_refresh.lock"),
            Duration::from_secs(60),
        ),
        (PathBuf::from(legacy), Duration::from_secs(60)),
    ];

    let config_file = home.join(".claude.json");
    let mut with_config = refresh.clone();
    with_config.push((home.join(".claude.json.lock"), Duration::from_secs(10)));
    assert_eq!(
        LockScope::RefreshAndConfig(&config_file).paths(&dir),
        with_config
    );
    assert_eq!(
        LockScope::StorageWrite.paths(&dir),
        vec![(
            config_dir.join(".storage-write.lock"),
            Duration::from_secs(15)
        )]
    );
}
