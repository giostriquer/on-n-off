//! Claude Code's native store: which Keychain item or credentials file holds its signed-in login,
//! how that item is found, the lock directories Claude Code takes around changing it, and the
//! writes of the `claudeAiOauth` document itself.
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

/// What one credential source (Keychain entry or file) yielded.
enum Source {
    Found(ClaudeStore, Value),
    /// Nothing stored (or JSON without an access token).
    Absent,
    /// Something is stored but could not be read or parsed.
    Broken(String),
}

/// The stored login document and the store holding it: Keychain entry first (macOS), then
/// `<config dir>/.credentials.json` (all platforms). `Ok(None)` when no source holds a login,
/// `Err` when one is there but could not be read.
///
/// The Limits read and the renewal both go through this, so the two can never disagree about
/// which entry they are talking about.
pub(crate) fn login_document(
    dir: &ConfigDir,
    keychain: KeychainProbe,
) -> Result<Option<(ClaudeStore, Value)>, String> {
    let path = dir.credentials_file();
    let sources = [claude_from_keychain(keychain), claude_from_file(&path)];
    let mut first_break = None;
    for source in sources {
        match source {
            Source::Found(store, document) => return Ok(Some((store, document))),
            Source::Absent => {}
            Source::Broken(why) => {
                first_break.get_or_insert(why);
            }
        }
    }
    first_break.map_or(Ok(None), Err)
}

/// A source counts as `Found` only when it parses into a login. A Keychain entry holding JSON
/// without an access token is `Absent`, so the file behind it still gets its turn.
fn claude_from_keychain(probe: KeychainProbe) -> Source {
    match probe {
        Ok(Some(json)) => match serde_json::from_str::<Value>(&json) {
            Ok(value) => found(ClaudeStore::Keychain, value),
            Err(error) => Source::Broken(format!("Keychain entry is not valid JSON: {error}")),
        },
        Ok(None) => Source::Absent,
        Err(why) => Source::Broken(why),
    }
}

fn claude_from_file(path: &Path) -> Source {
    match read_json_file(path) {
        Ok(Some(value)) => found(ClaudeStore::File(path.to_path_buf()), value),
        Ok(None) => Source::Absent,
        Err(why) => Source::Broken(why),
    }
}

fn found(store: ClaudeStore, document: Value) -> Source {
    if crate::limits::credentials::parse_claude_credential(&document).is_some() {
        Source::Found(store, document)
    } else {
        Source::Absent
    }
}

/// `Ok(None)` when the file does not exist; `Err` for any other I/O or JSON failure.
pub(crate) fn read_json_file(path: &Path) -> Result<Option<Value>, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<Value>(&raw)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Probe the macOS Keychain for Claude Code's login, the way Limits reads it. A disposable
/// `ON_N_OFF_HOME` never reads the real login.
#[cfg(target_os = "macos")]
pub(crate) fn keychain_probe() -> KeychainProbe {
    isolated_keychain(std::env::var_os("ON_N_OFF_HOME").is_some(), || {
        super::keychain::find_password(CLAUDE_KEYCHAIN_SERVICE, None)
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

/// Deadline for the renewal's attribute lookup, which raises no prompt.
#[cfg(target_os = "macos")]
const RENEWAL_ACCOUNT_DEADLINE: Duration = Duration::from_secs(30);

/// The account the renewal's Keychain entry is filed under, so a write updates *that* item.
///
/// `security add-generic-password -U` matches on service **and** account, and the Limits read
/// matches on service alone. Guessing the account from `$USER` would therefore create a second
/// item for the same service whenever the guess is wrong, and `find-generic-password` then
/// returns one of the two in no defined order — on-n-off and Claude Code reading different
/// logins, with nothing on screen to say so. Asking the entry itself removes the guess.
#[cfg(target_os = "macos")]
fn renewal_account() -> Result<String, String> {
    match super::keychain::find_attributes(CLAUDE_KEYCHAIN_SERVICE, RENEWAL_ACCOUNT_DEADLINE) {
        Ok(crate::process::CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        }) => parse_keychain_account(&stdout),
        _ => None,
    }
    .ok_or_else(|| "could not read the Claude Keychain entry's account".to_string())
}

/// Mirrors `keychain_probe`'s stub: these platforms have no such entry, and the file store is the
/// one their read will have chosen.
#[cfg(not(target_os = "macos"))]
fn renewal_account() -> Result<String, String> {
    Err("this platform has no Claude Code Keychain entry".to_string())
}

/// Deadline for the account switch's attribute lookup.
#[cfg(target_os = "macos")]
const NATIVE_ACCOUNT_DEADLINE: Duration = Duration::from_secs(10);

/// The account of the account switch's Keychain entry: `Ok(None)` when there is no entry, an
/// error when it could not be inspected or names no account.
#[cfg(target_os = "macos")]
pub(crate) fn native_account(service: &str) -> Result<Option<String>, String> {
    use crate::process::CommandOutcome;
    match super::keychain::find_attributes(service, NATIVE_ACCOUNT_DEADLINE) {
        Ok(CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        }) => parse_keychain_account(&stdout)
            .map(Some)
            .ok_or_else(|| "Cannot identify the native Keychain entry.".into()),
        Ok(CommandOutcome::Exited { stderr, .. }) if stderr.contains("could not be found") => {
            Ok(None)
        }
        Ok(_) => Err("Native Keychain access was denied or unavailable.".into()),
        Err(_) => Err("Cannot inspect native Keychain entry.".into()),
    }
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
