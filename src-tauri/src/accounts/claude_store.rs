//! Claude Code's native store: which Keychain item or credentials file holds its signed-in login,
//! read with Claude Code's own precedence; how that item is found; the lock directories Claude
//! Code takes around changing it; and the writes of the `claudeAiOauth` document itself.
//!
//! Every on-n-off path that reads or writes Claude's login comes through here — the Limits read,
//! the renewal in [`super::claude_renew`] and the account switch in [`super::native`] — so the
//! three cannot disagree about which store they are talking about or about the locks around it.
//! A second copy of either is how a renewal redeems the file's refresh token and writes it over a
//! Keychain entry the next read prefers, or takes a lock another path does not know to wait for.
//!
//! The grant that redeems a refresh token is not here: Claude grants live only in `claude_renew`.

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
/// Deliberately not gated to macOS. The renewal names the store it is writing to on every
/// platform, and the stub that answers "there is no Keychain here" is chosen inside
/// `accounts::keychain::write`, not by making the name itself disappear.
pub(crate) const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// One Claude Code config dir: the credentials file and the lock directories in and beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigDir {
    path: PathBuf,
}

impl ConfigDir {
    /// `<home>/.claude`, the default config dir.
    pub(crate) fn default_in(home: &Path) -> Self {
        Self {
            path: home.join(".claude"),
        }
    }

    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn credentials_file(&self) -> PathBuf {
        self.path.join(".credentials.json")
    }

    /// The Keychain entry Claude Code files this dir's login under when `CLAUDE_CONFIG_DIR` chose
    /// it: its own name, suffixed with a hash of the NFC-normalized path.
    #[cfg(target_os = "macos")]
    pub(crate) fn scoped_service(&self) -> String {
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

/// Where a stored login lives, so a write goes back to the entry the next read will consult.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClaudeStore {
    /// The `Claude Code-credentials` Keychain entry, which only macOS has.
    Keychain,
    /// `<config dir>/.credentials.json`: the only store on Windows, and the macOS fallback.
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
/// - An entry that is not JSON, or no entry, leaves it to `<config dir>/.credentials.json`.
/// - A Keychain that cannot be read (denied, not answered, locked) leaves the read to the file
///   too, but only a file that holds a document can answer for it: with nothing there the
///   Keychain's failure is the answer, because a login may be behind it. And a write refuses.
pub(crate) fn read(dir: &ConfigDir, keychain: KeychainProbe) -> Result<Stored, StoreError> {
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

/// `Ok(None)` when the file does not exist; `Err` for any other I/O or JSON failure.
pub(crate) fn read_json_file(path: &Path) -> Result<Option<Value>, String> {
    read_document(path).map_err(|error| error.to_string())
}

/// Probe the macOS Keychain for Claude Code's login, the way Limits reads it. A disposable
/// `ON_N_OFF_HOME` never reads the real login.
#[cfg(target_os = "macos")]
pub(crate) fn keychain_probe() -> KeychainProbe {
    isolated_keychain(std::env::var_os("ON_N_OFF_HOME").is_some(), || {
        keychain_secret(CLAUDE_KEYCHAIN_SERVICE)
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn keychain_probe() -> KeychainProbe {
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

/// This process's name for Claude Code's Keychain entry. A test binary sees no environment here,
/// so every test resolves the fallback name and never a developer's own.
#[cfg(target_os = "macos")]
fn own_account() -> String {
    claude_code_account(&|name| {
        if cfg!(test) {
            None
        } else {
            std::env::var_os(name)
        }
    })
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

/// The account the renewal's write goes to.
#[cfg(target_os = "macos")]
fn renewal_account() -> Result<String, String> {
    keychain_account(CLAUDE_KEYCHAIN_SERVICE)?
        .ok_or_else(|| "could not read the Claude Keychain entry's account".to_string())
}

/// Mirrors `keychain_probe`'s stub: these platforms have no such entry, and the file store is the
/// one their read will have chosen.
#[cfg(not(target_os = "macos"))]
fn renewal_account() -> Result<String, String> {
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

/// A store write resolved and proven before anything irreversible happens.
///
/// `prepare` does the parts that can fail on their own account: reading the Keychain entry's
/// account, or creating the private temporary the file store renames into. `commit` is then the
/// single irreversible step, small enough to sit comfortably inside the refresh lock's minute.
pub(crate) struct PreparedWrite {
    target: WriteTarget,
}

enum WriteTarget {
    Keychain { account: String },
    File { path: PathBuf, temporary: PathBuf },
}

impl PreparedWrite {
    pub(crate) fn prepare(store: &ClaudeStore) -> Result<Self, String> {
        let target = match store {
            ClaudeStore::Keychain => WriteTarget::Keychain {
                account: renewal_account()?,
            },
            ClaudeStore::File(path) => {
                let temporary = path.with_extension("json.on-n-off");
                // Created empty and private now, so a directory that will not take it says so
                // before the grant rather than after.
                write_private(&temporary, "")
                    .map_err(|error| format!("{}: {error}", temporary.display()))?;
                WriteTarget::File {
                    path: path.clone(),
                    temporary,
                }
            }
        };
        Ok(Self { target })
    }

    pub(crate) fn commit(self, raw: &str) -> Result<(), String> {
        match &self.target {
            WriteTarget::Keychain { account } => {
                super::keychain::write(CLAUDE_KEYCHAIN_SERVICE, account, raw.as_bytes())
            }
            WriteTarget::File { path, temporary } => {
                write_private(temporary, raw)
                    .map_err(|error| format!("{}: {error}", temporary.display()))?;
                fs::rename(temporary, path).map_err(|error| format!("{}: {error}", path.display()))
            }
        }
    }
}

impl Drop for PreparedWrite {
    /// The temporary never outlives the attempt. A successful commit renamed it away, and a failed
    /// one leaves a live refresh token in a file Claude Code does not know about and will never
    /// rotate — the same objection that keeps `ConfigIo` out of this module. The user has to sign
    /// in again either way; an orphaned secret does not help them do it.
    fn drop(&mut self) {
        if let WriteTarget::File { temporary, .. } = &self.target {
            let _ = fs::remove_file(temporary);
        }
    }
}

/// The credentials file holds a refresh token, so it is created 0600 and never handed to
/// `ConfigIo`, whose backups would copy the secret somewhere Claude Code neither knows about nor
/// rotates.
#[cfg(unix)]
fn write_private(path: &Path, raw: &str) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(raw.as_bytes())?;
    file.sync_all()
}

#[cfg(not(unix))]
fn write_private(path: &Path, raw: &str) -> io::Result<()> {
    fs::write(path, raw)
}

/// Claude Code treats a refresh lock older than a minute as abandoned. Matching that is what makes
/// the two implementations take turns instead of both deciding the other is stuck.
pub(crate) const LOCK_STALE: Duration = Duration::from_secs(60);

/// Claude Code abandons a config file's lock after ten seconds.
const CONFIG_LOCK_STALE: Duration = Duration::from_secs(10);

/// How often a held lock with a heartbeat is touched: far inside every staleness limit.
const HEARTBEAT: Duration = Duration::from_secs(2);

/// Which of Claude Code's locks to take.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LockScope<'a> {
    /// The two refresh locks, around one renewal.
    Refresh,
    /// The refresh locks and this config file's lock, around an account change that writes both.
    RefreshAndConfig(&'a Path),
}

impl LockScope<'_> {
    /// The lock directories in the order Claude Code takes them — two processes that disagree
    /// about the order deadlock — each with the age after which it counts as abandoned.
    fn paths(self, dir: &ConfigDir) -> Vec<(PathBuf, Duration)> {
        let mut legacy = dir.path.as_os_str().to_owned();
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
    pub(crate) fn acquire(dir: &ConfigDir, scope: LockScope<'_>) -> Result<Self, LockError> {
        Self::acquire_at(dir, scope, SystemTime::now())
    }

    /// `now` is a parameter so the staleness branch is reachable from a test without waiting a
    /// minute or backdating a directory the filesystem may not let us touch.
    pub(crate) fn acquire_at(
        dir: &ConfigDir,
        scope: LockScope<'_>,
        now: SystemTime,
    ) -> Result<Self, LockError> {
        let mut locks = Self {
            held: Vec::new(),
            heartbeat: None,
            lost: Arc::new(AtomicBool::new(false)),
        };
        for (path, stale) in scope.paths(dir) {
            if matches!(scope, LockScope::Refresh) {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| LockError::Unavailable(error.to_string()))?;
                }
            }
            // `?` drops `locks`, which releases whatever it had already taken.
            take(&path, stale, now)?;
            locks.held.push(path);
        }
        if matches!(scope, LockScope::RefreshAndConfig(_)) {
            locks.heartbeat = Some(Heartbeat::start(locks.held.clone(), locks.lost.clone()));
        }
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
