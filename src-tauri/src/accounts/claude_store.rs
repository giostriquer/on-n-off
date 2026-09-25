//! Claude Code's native store: where its dirs are ([`dirs`], honouring `CLAUDE_CONFIG_DIR` and
//! `CLAUDE_SECURESTORAGE_CONFIG_DIR`); which Keychain item or credentials file holds its signed-in
//! login, read with Claude Code's own precedence ([`read`]); how that item is found; the lock
//! directories Claude Code takes around changing it; and the writes of the `claudeAiOauth`
//! document itself.
//!
//! Everything here follows Claude Code 2.1.282, with one intended difference: when the Keychain
//! cannot be read and the credentials file holds no document, Claude Code reads a signed-out
//! user, while this reports the Keychain's failure, because a login may well be behind it and
//! "sign in again" would be the wrong advice.
//!
//! Every on-n-off path that reads or writes Claude's login comes through here — the Limits read,
//! the renewal in [`super::claude_renew`] and the account switch in [`super::native`] — so the
//! three cannot disagree about which store they are talking about or about the locks around it.
//! A second copy of either is how a renewal redeems the file's refresh token and writes it over a
//! Keychain entry the next read prefers, or takes a lock another path does not know to wait for.
//!
//! The grant that redeems a refresh token is not here: Claude grants live only in `claude_renew`.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use serde_json::Value;

/// Outcome of probing the macOS Keychain: `Ok(Some(json))` entry found, `Ok(None)` no entry,
/// `Err(why)` the entry could not be read (access denied, tool failure).
pub(crate) type KeychainProbe = Result<Option<String>, String>;

/// The name of Claude Code's Keychain entry for its default config dir. Read and write share it: a
/// second copy that drifted would mean writing a renewed login to an entry nothing reads.
///
/// Not gated to macOS: [`StorageDir::service`] names the entry on every platform, and on Windows
/// a write to it is refused before anything is attempted, by `write_account`'s stub.
pub(crate) const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// Claude Code's storage dir: where its credentials file and lock directories live, and whose path
/// names its Keychain entry. It is the config dir unless `CLAUDE_SECURESTORAGE_CONFIG_DIR` moves it
/// (see [`dirs`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageDir {
    path: PathBuf,
    /// Whether the Keychain entry's name carries a hash of this path, as it does when the
    /// environment chose the dir rather than the default.
    scoped: bool,
}

#[cfg(test)]
impl StorageDir {
    /// `<home>/.claude`, the default, whose Keychain entry is Claude Code's unscoped one: what
    /// [`dirs`] resolves for a disposable home.
    pub(crate) fn default_in(home: &Path) -> Self {
        Self {
            path: home.join(".claude"),
            scoped: false,
        }
    }
}

impl StorageDir {
    pub(crate) fn new(path: PathBuf, scoped: bool) -> Self {
        Self { path, scoped }
    }

    /// Where Claude Code keeps the login for the config dir `config`: that dir, scoped when
    /// `CLAUDE_CONFIG_DIR` chose it (`custom`), unless `CLAUDE_SECURESTORAGE_CONFIG_DIR` moved it.
    pub(crate) fn of(config: &Path, custom: bool, secure_storage: Option<&SecureStorage>) -> Self {
        secure_storage.map_or_else(
            || Self::new(config.to_path_buf(), custom),
            |secure| secure.dir.clone(),
        )
    }

    pub(crate) fn credentials_file(&self) -> PathBuf {
        self.path.join(".credentials.json")
    }

    /// The Keychain entry Claude Code files this dir's login under: its own name, suffixed with a
    /// hash of the NFC-normalized path when the dir is scoped.
    pub(crate) fn service(&self) -> String {
        if !self.scoped {
            return CLAUDE_KEYCHAIN_SERVICE.into();
        }
        format!(
            "{CLAUDE_KEYCHAIN_SERVICE}-{}",
            &crate::sha::sha256_hex(
                unicode_normalization::UnicodeNormalization::nfc(
                    self.path.to_string_lossy().as_ref()
                )
                .collect::<String>()
                .as_bytes()
            )[..8]
        )
    }
}

/// Claude Code's dirs for one environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dirs {
    /// The config dir: `CLAUDE_CONFIG_DIR`, else `<home>/.claude`.
    pub(crate) config: PathBuf,
    /// `CLAUDE_CONFIG_DIR` chose the config dir.
    pub(crate) custom: bool,
    pub(crate) secure_storage: Option<SecureStorage>,
}

impl Dirs {
    /// The storage dir: the config dir, unless `secure_storage` moved it.
    pub(crate) fn storage(&self) -> StorageDir {
        StorageDir::of(&self.config, self.custom, self.secure_storage.as_ref())
    }

    /// The file Claude Code keeps its signed-in account in: `.config.json` in the config dir when
    /// an older Claude Code left one, else `.claude.json` in the config dir `CLAUDE_CONFIG_DIR`
    /// chose, or in `home`.
    pub(crate) fn config_file(&self, home: &Path) -> PathBuf {
        let legacy = self.config.join(".config.json");
        if legacy.exists() {
            legacy
        } else if self.custom {
            self.config.join(".claude.json")
        } else {
            home.join(".claude.json")
        }
    }
}

/// Claude Code's dirs under `home`, for this process's environment.
pub(crate) fn native_dirs(home: &Path) -> Result<Dirs, String> {
    dirs(home, &crate::paths::process_env)
}

/// The name of the variable that moves Claude Code's storage away from its config dir.
pub(crate) const SECURE_STORAGE_VAR: &str = "CLAUDE_SECURESTORAGE_CONFIG_DIR";

/// `CLAUDE_SECURESTORAGE_CONFIG_DIR` as it was set, and the storage dir it chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecureStorage {
    /// The value exactly as set, empty included, for a `claude` started for this store to be
    /// handed the same, as Claude Code hands it to the processes it starts.
    pub(crate) var: OsString,
    pub(crate) dir: StorageDir,
}

/// Claude Code's dirs under `home` for the environment `env` reads, resolved as Claude Code
/// 2.1.282 resolves them:
///
/// - The config dir is `CLAUDE_CONFIG_DIR` exactly as set — never trimmed, and set even when
///   empty — else `<home>/.claude`, NFC-normalized either way.
/// - The storage dir, which holds the credentials file and the lock directories, is the config
///   dir; a set `CLAUDE_CONFIG_DIR` scopes its Keychain entry by a hash of that path.
/// - `CLAUDE_SECURESTORAGE_CONFIG_DIR`, when set, moves the storage dir to it (NFC-normalized)
///   and scopes the entry by its hash instead, leaving the config dir alone. Set but empty, it
///   puts the storage back in `<home>/.claude` under the unscoped entry, whatever
///   `CLAUDE_CONFIG_DIR` says.
///
/// A dir that is not absolute is refused. Claude Code would resolve it against whatever directory
/// it happens to run in, which on-n-off cannot know. A disposable `ON_N_OFF_HOME` keeps the
/// default dirs whatever the environment says, so no test or development run follows it to a
/// real store.
pub(crate) fn dirs(home: &Path, env: &dyn Fn(&str) -> Option<OsString>) -> Result<Dirs, String> {
    if env("ON_N_OFF_HOME").is_some() {
        return Ok(Dirs {
            config: home.join(".claude"),
            custom: false,
            secure_storage: None,
        });
    }
    let default = || home.join(".claude").into_os_string();
    let chosen = env("CLAUDE_CONFIG_DIR");
    let config = absolute(nfc(chosen.clone().unwrap_or_else(default)))?;
    let secure_storage = env(SECURE_STORAGE_VAR)
        .map(|var| {
            let scoped = !var.is_empty();
            let path = if scoped { var.clone() } else { default() };
            absolute(nfc(path)).map(|path| SecureStorage {
                dir: StorageDir::new(path, scoped),
                var,
            })
        })
        .transpose()?;
    Ok(Dirs {
        config,
        custom: chosen.is_some(),
        secure_storage,
    })
}

fn absolute(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("The provider home must be an absolute path.".into())
    }
}

/// `path` in Unicode normalization form C, as Claude Code normalizes its dirs. A path that is not
/// Unicode is left as it is.
fn nfc(path: OsString) -> PathBuf {
    use unicode_normalization::UnicodeNormalization;
    match path.into_string() {
        Ok(text) => PathBuf::from(text.nfc().collect::<String>()),
        Err(raw) => PathBuf::from(raw),
    }
}

/// Where a stored login lives, so a write goes back to the entry the next read will consult.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClaudeStore {
    /// The `Claude Code-credentials` Keychain entry, which only macOS has.
    Keychain,
    /// `<storage dir>/.credentials.json`: the only store on Windows, and the macOS fallback.
    File(PathBuf),
}

/// What Claude Code's next read of its login would find, and where a write has to go.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Stored {
    /// The store that read uses. `Err` when the Keychain could not be read: the credentials file
    /// stood in for it, but whether Claude Code will read the Keychain or the file next is
    /// unknown, so a write refuses rather than guess.
    pub(crate) target: Result<ClaudeStore, String>,
    /// What that store holds, token or not; `None` when nothing is stored there.
    pub(crate) document: Option<Value>,
}

/// Why no store could be read. Each caller words these its own way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoreError {
    /// The Keychain could not be read, and the credentials file held nothing to stand in for it.
    Keychain(String),
    /// The credentials file could not be read.
    FileUnreadable(String),
    /// The credentials file is not JSON.
    FileMalformed(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keychain(why) | Self::FileUnreadable(why) | Self::FileMalformed(why) => {
                f.write_str(why)
            }
        }
    }
}

/// Claude Code's login, from the store Claude Code itself would read it from (2.1.282):
///
/// - A Keychain entry that parses as JSON is the login, token or not. Claude Code signs out by
///   emptying it, so an entry without a token is a signed-out user, never a reason to look past
///   it to an older login in the file.
/// - An entry that is not JSON, or no entry, leaves it to `<storage dir>/.credentials.json`.
/// - A Keychain that cannot be read (denied, not answered, locked) leaves the read to the file
///   too, but only a file that holds a document can answer for it: with nothing there the
///   Keychain's failure is the answer, because a login may be behind it. Claude Code reads that
///   case as signed out; the difference is intended. And a write refuses.
pub(crate) fn read(dir: &StorageDir, keychain: KeychainProbe) -> Result<Stored, StoreError> {
    let unread = match keychain {
        Ok(Some(secret)) => match serde_json::from_str::<Value>(&secret) {
            Ok(Value::Null) | Err(_) => None,
            Ok(document) => {
                return Ok(Stored {
                    target: Ok(ClaudeStore::Keychain),
                    document: Some(document),
                })
            }
        },
        Ok(None) => None,
        Err(why) => Some(why),
    };
    let path = dir.credentials_file();
    match (unread, read_document(&path)) {
        (None, document) => Ok(Stored {
            target: Ok(ClaudeStore::File(path)),
            document: document?,
        }),
        (Some(why), Ok(Some(document))) => Ok(Stored {
            target: Err(why),
            document: Some(document),
        }),
        (Some(why), _) => Err(StoreError::Keychain(why)),
    }
}

/// `Ok(None)` when the file does not exist.
fn read_document(path: &Path) -> Result<Option<Value>, StoreError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(StoreError::FileUnreadable(format!(
                "{}: {error}",
                path.display()
            )))
        }
    };
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|error| StoreError::FileMalformed(format!("{}: {error}", path.display())))
}

/// Probe the macOS Keychain for the login Claude Code keeps for `dir`, the way Limits reads it. A
/// disposable `ON_N_OFF_HOME` never reads the real login.
#[cfg(target_os = "macos")]
pub(crate) fn keychain_probe(dir: &StorageDir) -> KeychainProbe {
    isolated_keychain(crate::paths::process_env("ON_N_OFF_HOME").is_some(), || {
        keychain_secret(&dir.service())
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn keychain_probe(_dir: &StorageDir) -> KeychainProbe {
    Ok(None)
}

#[cfg(any(target_os = "macos", test))]
fn isolated_keychain(isolated: bool, probe: impl FnOnce() -> KeychainProbe) -> KeychainProbe {
    if isolated {
        Ok(None)
    } else {
        probe()
    }
}

/// The account name Claude Code (2.1.282) files its Keychain entry under: `$USER`, else the login
/// name, and `claude-code-user` when that is not a plain name `security` takes as it is. Claude
/// Code asks the OS for the login name; `$LOGNAME`, which a login session and launchd both set,
/// stands in for it here.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn claude_code_account(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> String {
    ["USER", "LOGNAME"]
        .into_iter()
        .filter_map(env)
        .map(|name| name.to_string_lossy().into_owned())
        .find(|name| !name.is_empty())
        .filter(|name| {
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
        .unwrap_or_else(|| "claude-code-user".into())
}

/// This process's name for Claude Code's Keychain entry. A test binary resolves the fallback name,
/// never a developer's own: [`crate::paths::process_env`] shows it no `$USER`.
#[cfg(target_os = "macos")]
fn own_account() -> String {
    claude_code_account(&crate::paths::process_env)
}

/// Deadline for an attribute lookup, which prints no secret and raises no prompt.
#[cfg(target_os = "macos")]
const ACCOUNT_DEADLINE: Duration = Duration::from_secs(30);

/// The secret of Claude Code's item under `service`. First the item filed under Claude Code's own
/// account name, which is the one Claude Code reads; failing that, whichever item the service
/// holds, through the account its attributes name, so an item an older Claude Code filed under
/// another account still resolves. A refused read of Claude Code's own item is not looked past.
#[cfg(target_os = "macos")]
pub(crate) fn keychain_secret(service: &str) -> KeychainProbe {
    let own = own_account();
    if let Some(secret) = super::keychain::find_password(service, Some(&own))? {
        return Ok(Some(secret));
    }
    match attributes_account(service, None)? {
        Some(filed) if filed != own => super::keychain::find_password(service, Some(&filed)),
        _ => Ok(None),
    }
}

/// The account of the item [`keychain_secret`] reads, found the same way but from attributes
/// alone, so a write replaces that item.
///
/// `security add-generic-password -U` matches on service **and** account. Any other account would
/// file a second item for the same service, and a service-only lookup then returns one of the two
/// in no defined order — on-n-off and Claude Code reading different logins, with nothing on
/// screen to say so.
#[cfg(target_os = "macos")]
pub(crate) fn keychain_account(service: &str) -> Result<Option<String>, String> {
    let own = own_account();
    if attributes_account(service, Some(&own))?.is_some() {
        return Ok(Some(own));
    }
    attributes_account(service, None)
}

/// The account of the item under `service` (and `account`, when named), read from its attributes.
#[cfg(target_os = "macos")]
fn attributes_account(service: &str, account: Option<&str>) -> Result<Option<String>, String> {
    use crate::process::CommandOutcome;
    match super::keychain::find_attributes(service, account, ACCOUNT_DEADLINE)? {
        CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        } => parse_keychain_account(&stdout)
            .map(Some)
            .ok_or_else(|| "Cannot identify the Claude Code Keychain entry.".to_string()),
        CommandOutcome::Exited { stderr, .. } if stderr.contains("could not be found") => Ok(None),
        CommandOutcome::Exited { stderr, .. } => {
            Err(format!("Keychain lookup failed ({}).", stderr.trim()))
        }
        CommandOutcome::TimedOut => Err("Keychain lookup was not answered in time".to_string()),
    }
}

/// The account a write to the Keychain entry under `service` goes to.
#[cfg(target_os = "macos")]
fn write_account(service: &str) -> Result<String, String> {
    keychain_account(service)?.ok_or_else(|| "The Claude Code Keychain entry disappeared.".into())
}

/// Mirrors `keychain_probe`'s stub: these platforms have no such entry, and the file store is the
/// one their read will have chosen.
#[cfg(not(target_os = "macos"))]
fn write_account(_service: &str) -> Result<String, String> {
    Err("this platform has no Claude Code Keychain entry".to_string())
}

/// Pull `acct` out of `security find-generic-password`'s attribute dump. Kept pure so the parsing
/// is testable on every platform; only the lookups above are macOS-specific.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn parse_keychain_account(attributes: &str) -> Option<String> {
    attributes
        .lines()
        .filter_map(|line| line.trim().strip_prefix("\"acct\"<blob>=\""))
        .find_map(|rest| rest.strip_suffix('"'))
        .filter(|account| !account.is_empty())
        .map(str::to_string)
}

/// Why a change to Claude Code's credentials could not begin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BeginError {
    /// Claude Code is writing its credentials: its change goes first.
    Busy,
    /// The storage-write lock could not be created, for a reason of its own.
    Lock(String),
    /// No store could be read.
    Store(StoreError),
    /// The write could not be proven: a Keychain that could not be read, the Keychain entry's
    /// account, a linked credentials file, or a temporary the directory will not take.
    Unavailable(String),
}

impl From<LockError> for BeginError {
    fn from(error: LockError) -> Self {
        match error {
            LockError::Busy => Self::Busy,
            LockError::Unavailable(why) => Self::Lock(why),
        }
    }
}

/// One change to Claude Code's credentials: the only way on-n-off writes them.
///
/// [`begin`] takes Claude Code's `.storage-write.lock` and reads the store under it, and
/// [`PendingWrite::prove`] then proves the write: the Keychain entry's account, or a private
/// temporary beside the credentials file. What is left for [`CredentialWrite::commit`] is one
/// `security -U` or one rename, and the document it writes is a change to the one read under the
/// lock, so no credential write of Claude Code's can land in between. The lock is held until the
/// write drops.
pub(crate) struct PendingWrite<'a> {
    dir: StorageDir,
    target: Result<ClaudeStore, String>,
    source: ClaudeStore,
    storage: ClaudeLocks,
    held: &'a dyn Fn() -> bool,
}

/// A [`PendingWrite`] proven: nothing left that can fail on its own account but the write itself.
pub(crate) struct CredentialWrite<'a> {
    target: WriteTarget,
    storage: ClaudeLocks,
    held: &'a dyn Fn() -> bool,
}

enum WriteTarget {
    Keychain {
        service: String,
        account: String,
    },
    File {
        path: PathBuf,
        temporary: tempfile::NamedTempFile,
    },
}

/// Begin a change to the credentials in `dir`: take Claude Code's storage-write lock and read the
/// store under it with `keychain`. `held` says whether a lock the caller holds around the change
/// was taken away. Returns what the store holds, `None` for nothing, and the write, still to be
/// proven: a caller that finds it has nothing to write never pays for the proof.
pub(crate) fn begin<'a>(
    dir: &StorageDir,
    keychain: &dyn Fn(&StorageDir) -> KeychainProbe,
    held: &'a dyn Fn() -> bool,
) -> Result<(Option<Value>, PendingWrite<'a>), BeginError> {
    let storage = ClaudeLocks::acquire(dir, LockScope::StorageWrite)?;
    let stored = read(dir, keychain(dir)).map_err(BeginError::Store)?;
    let source = stored
        .target
        .clone()
        .unwrap_or_else(|_| ClaudeStore::File(dir.credentials_file()));
    Ok((
        stored.document,
        PendingWrite {
            dir: dir.clone(),
            target: stored.target,
            source,
            storage,
            held,
        },
    ))
}

impl<'a> PendingWrite<'a> {
    /// The store the document came from: the Keychain entry, or the credentials file.
    pub(crate) fn source(&self) -> &ClaudeStore {
        &self.source
    }

    /// Prove the write, before anything is written or redeemed. Refused when the Keychain could
    /// not be read, since which store Claude Code reads next is then unknown, and when the
    /// credentials file is a link: replacing it would cut the link and leave the login where
    /// Claude Code no longer looks.
    pub(crate) fn prove(self) -> Result<CredentialWrite<'a>, BeginError> {
        let store = self.target.map_err(BeginError::Unavailable)?;
        let target = match store {
            ClaudeStore::Keychain => {
                let service = self.dir.service();
                WriteTarget::Keychain {
                    account: write_account(&service).map_err(BeginError::Unavailable)?,
                    service,
                }
            }
            ClaudeStore::File(path) => {
                if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
                    return Err(BeginError::Unavailable(
                        "Refusing to replace a linked credential file.".into(),
                    ));
                }
                // Created now, private and beside the file, so a directory that will not take it
                // says so before anything is written, or redeemed. Named for what it is, so one
                // left behind by a kill between write and rename can be told apart.
                let temporary = tempfile::Builder::new()
                    .prefix(".credentials.json.on-n-off.")
                    .tempfile_in(&self.dir.path)
                    .map_err(|error| {
                        BeginError::Unavailable(format!("{}: {error}", self.dir.path.display()))
                    })?;
                WriteTarget::File { path, temporary }
            }
        };
        Ok(CredentialWrite {
            target,
            storage: self.storage,
            held: self.held,
        })
    }
}

impl CredentialWrite<'_> {
    /// Whether a lock this change relies on was taken away while held: the caller's own, or the
    /// storage-write lock. Whatever is written after that is uncoordinated.
    pub(crate) fn lost(&self) -> bool {
        (self.held)() || self.storage.lost()
    }

    /// Write `document`: one `security -U`, or the temporary synced and renamed over the
    /// credentials file, and the directory synced.
    pub(crate) fn commit(self, document: &Value) -> Result<(), String> {
        let bytes = serde_json::to_vec(document).map_err(|error| error.to_string())?;
        match self.target {
            WriteTarget::Keychain { service, account } => {
                super::keychain::write(&service, &account, &bytes)
            }
            WriteTarget::File {
                path,
                mut temporary,
            } => {
                use std::io::Write;
                temporary
                    .write_all(&bytes)
                    .and_then(|()| temporary.as_file().sync_all())
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                temporary
                    .persist(&path)
                    .map_err(|error| format!("{}: {}", path.display(), error.error))?;
                #[cfg(unix)]
                if let Some(parent) = path.parent() {
                    fs::File::open(parent)
                        .and_then(|directory| directory.sync_all())
                        .map_err(|error| format!("{}: {error}", parent.display()))?;
                }
                Ok(())
            }
        }
    }
}

/// Claude Code treats a refresh lock older than a minute as abandoned. Matching that is what makes
/// the two implementations take turns instead of both deciding the other is stuck.
const LOCK_STALE: Duration = Duration::from_secs(60);

/// Claude Code abandons a config file's lock after ten seconds: the smallest staleness of any lock
/// taken here.
const CONFIG_LOCK_STALE: Duration = Duration::from_secs(10);

/// Claude Code abandons its credentials' write lock after fifteen seconds.
const STORAGE_WRITE_STALE: Duration = Duration::from_secs(15);

/// How long Claude Code 2.1.282 watches a refresh lock whose time has not moved before it starts
/// asking whether its holder is still alive.
const CLAUDE_CODE_LIVENESS: Duration = Duration::from_millis(7500);

/// How often every held lock is touched.
const HEARTBEAT: Duration = Duration::from_secs(2);

// A held lock must never look abandoned: the heartbeat has to land several times over within the
// smallest staleness limit and within Claude Code's liveness watch, or a slow tick breaks a lock
// this process still holds, or has Claude Code ask whether its holder is alive.
const _: () = assert!(HEARTBEAT.as_millis() * 3 <= CONFIG_LOCK_STALE.as_millis());
const _: () = assert!(HEARTBEAT.as_millis() * 3 <= CLAUDE_CODE_LIVENESS.as_millis());

/// Which of Claude Code's locks to take, in the storage dir.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LockScope<'a> {
    /// The two refresh locks, around one renewal.
    Refresh,
    /// The refresh locks and this config file's lock, around an account change that writes both.
    RefreshAndConfig(&'a Path),
    /// `.storage-write.lock`, which Claude Code takes around every change to its credentials, in
    /// the Keychain or the file. Taken inside the refresh locks, as Claude Code takes it.
    StorageWrite,
}

impl LockScope<'_> {
    /// The lock directories in the order Claude Code takes them — two processes that disagree
    /// about the order deadlock — each with the age after which it counts as abandoned. The legacy
    /// lock sits beside the storage dir's real path, as Claude Code resolves it, so a storage dir
    /// reached through a link locks the directory Claude Code locks.
    fn paths(self, dir: &StorageDir) -> Vec<(PathBuf, Duration)> {
        if let Self::StorageWrite = self {
            return vec![(dir.path.join(".storage-write.lock"), STORAGE_WRITE_STALE)];
        }
        let mut legacy = fs::canonicalize(&dir.path)
            .unwrap_or_else(|_| dir.path.clone())
            .into_os_string();
        legacy.push(".lock");
        let mut paths = vec![
            (dir.path.join(".oauth_refresh.lock"), LOCK_STALE),
            (PathBuf::from(legacy), LOCK_STALE),
        ];
        if let Self::RefreshAndConfig(config_file) = self {
            let mut config = config_file.as_os_str().to_owned();
            config.push(".lock");
            paths.push((PathBuf::from(config), CONFIG_LOCK_STALE));
        }
        paths
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LockError {
    /// Another process holds one of the locks and it is not stale: whoever holds it goes first.
    Busy,
    /// A lock could not be created for a reason of its own.
    Unavailable(String),
}

/// Claude Code's lock directories, held until this value drops and released in reverse order.
///
/// They are directories, taken with `mkdir` because that is atomic on every filesystem either
/// implementation runs on. One already there yields at once unless it is stale, in which case its
/// holder is gone and it is broken, as Claude Code breaks one.
///
/// While held, every lock is touched every [`HEARTBEAT`], as Claude Code touches its own. Nothing
/// done under them is bounded by the minute after which Claude Code breaks a lock that has gone
/// quiet — the Keychain probe allows ninety seconds for its prompt, an account lookup thirty and a
/// Keychain write twenty — so without the heartbeat Claude Code would break a lock this process
/// still holds. A lock taken away all the same shows as [`ClaudeLocks::lost`].
#[derive(Debug)]
pub(crate) struct ClaudeLocks {
    held: Vec<PathBuf>,
    heartbeat: Option<Heartbeat>,
    lost: Arc<AtomicBool>,
}

#[derive(Debug)]
struct Heartbeat {
    stop: mpsc::Sender<()>,
    worker: JoinHandle<()>,
}

impl ClaudeLocks {
    pub(crate) fn acquire(dir: &StorageDir, scope: LockScope<'_>) -> Result<Self, LockError> {
        Self::acquire_at(dir, scope, SystemTime::now())
    }

    /// `now` is a parameter so the staleness branch is reachable from a test without waiting a
    /// minute or backdating a directory the filesystem may not let us touch.
    pub(crate) fn acquire_at(
        dir: &StorageDir,
        scope: LockScope<'_>,
        now: SystemTime,
    ) -> Result<Self, LockError> {
        // Claude Code creates its config dir before locking in it, and so does this.
        fs::create_dir_all(&dir.path).map_err(|error| LockError::Unavailable(error.to_string()))?;
        let mut locks = Self {
            held: Vec::new(),
            heartbeat: None,
            lost: Arc::new(AtomicBool::new(false)),
        };
        for (path, stale) in scope.paths(dir) {
            // `?` drops `locks`, which releases whatever it had already taken.
            take(&path, stale, now)?;
            locks.held.push(path);
        }
        locks.heartbeat = Some(Heartbeat::start(locks.held.clone(), locks.lost.clone()));
        Ok(locks)
    }

    /// Whether a lock was taken away while held: its heartbeat could no longer touch it, so
    /// someone else broke or removed it and whatever this holder writes now is uncoordinated.
    pub(crate) fn lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }
}

impl Heartbeat {
    fn start(paths: Vec<PathBuf>, lost: Arc<AtomicBool>) -> Self {
        let (stop, receive) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            while receive.recv_timeout(HEARTBEAT) == Err(mpsc::RecvTimeoutError::Timeout) {
                for path in &paths {
                    if filetime::set_file_mtime(path, filetime::FileTime::now()).is_err() {
                        lost.store(true, Ordering::Release);
                        return;
                    }
                }
            }
        });
        Self { stop, worker }
    }
}

fn take(path: &Path, stale: Duration, now: SystemTime) -> Result<(), LockError> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !is_stale(path, stale, now) {
                return Err(LockError::Busy);
            }
            // Whoever left it behind is gone; Claude Code breaks an abandoned lock the same way,
            // and this inherits the same race between two processes that both judged it stale.
            let _ = fs::remove_dir(path);
            fs::create_dir(path).map_err(|_| LockError::Busy)
        }
        Err(error) => Err(LockError::Unavailable(error.to_string())),
    }
}

/// Only a real directory can be stale: a link or a file where a lock should be is left alone.
fn is_stale(path: &Path, stale: Duration, now: SystemTime) -> bool {
    fs::symlink_metadata(path)
        .ok()
        .filter(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age > stale)
}

impl Drop for ClaudeLocks {
    fn drop(&mut self) {
        if let Some(heartbeat) = self.heartbeat.take() {
            let _ = heartbeat.stop.send(());
            let _ = heartbeat.worker.join();
        }
        release(&self.held, |path| {
            let _ = fs::remove_dir(path);
        });
    }
}

/// Innermost first: the lock taken first stays held until every lock taken under it is gone.
fn release(held: &[PathBuf], mut remove: impl FnMut(&Path)) {
    for path in held.iter().rev() {
        remove(path);
    }
}

#[cfg(test)]
mod tests;
