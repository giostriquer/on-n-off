use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::Value;

pub(crate) type KeychainProbe = Result<Option<String>, String>;

pub(crate) const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageDir {
    path: PathBuf,
    scoped: bool,
}

impl StorageDir {
    pub(crate) fn new(path: PathBuf, scoped: bool) -> Self {
        Self { path, scoped }
    }

    pub(crate) fn of(config: &Path, custom: bool, secure_storage: Option<&SecureStorage>) -> Self {
        secure_storage.map_or_else(
            || Self::new(config.to_path_buf(), custom),
            |secure| secure.dir.clone(),
        )
    }

    pub(crate) fn credentials_file(&self) -> PathBuf {
        self.path.join(".credentials.json")
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dirs {
    pub(crate) config: PathBuf,
    pub(crate) custom: bool,
    pub(crate) secure_storage: Option<SecureStorage>,
}

impl Dirs {
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

pub(crate) const SECURE_STORAGE_VAR: &str = "CLAUDE_SECURESTORAGE_CONFIG_DIR";

pub(crate) const ENV_CREDENTIALS: [&str; 3] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecureStorage {
    pub(crate) var: OsString,
    pub(crate) dir: StorageDir,
}

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

fn nfc(path: OsString) -> PathBuf {
    use unicode_normalization::UnicodeNormalization;
    match path.into_string() {
        Ok(text) => PathBuf::from(text.nfc().collect::<String>()),
        Err(raw) => PathBuf::from(raw),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClaudeStore {
    Keychain,
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Stored {
    pub(crate) target: Result<ClaudeStore, String>,
    pub(crate) document: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoreError {
    Keychain(String),
    FileUnreadable(String),
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

#[cfg(target_os = "macos")]
fn own_account() -> String {
    claude_code_account(&crate::paths::process_env)
}

#[cfg(target_os = "macos")]
const ACCOUNT_DEADLINE: Duration = Duration::from_secs(30);

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

#[cfg(target_os = "macos")]
pub(crate) fn keychain_account(service: &str) -> Result<Option<String>, String> {
    let own = own_account();
    if attributes_account(service, Some(&own))?.is_some() {
        return Ok(Some(own));
    }
    attributes_account(service, None)
}

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

#[cfg(target_os = "macos")]
fn write_account(service: &str) -> Result<String, String> {
    keychain_account(service)?.ok_or_else(|| "The Claude Code Keychain entry disappeared.".into())
}

#[cfg(not(target_os = "macos"))]
fn write_account(_service: &str) -> Result<String, String> {
    Err("this platform has no Claude Code Keychain entry".to_string())
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn parse_keychain_account(attributes: &str) -> Option<String> {
    attributes
        .lines()
        .filter_map(|line| line.trim().strip_prefix("\"acct\"<blob>=\""))
        .find_map(|rest| rest.strip_suffix('"'))
        .filter(|account| !account.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BeginError {
    Busy,
    Lock(String),
    Store(StoreError),
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

pub(crate) struct PendingWrite<'a> {
    dir: StorageDir,
    target: Result<ClaudeStore, String>,
    storage: ClaudeLocks,
    held: &'a dyn Fn() -> bool,
}

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

pub(crate) fn begin<'a>(
    dir: &StorageDir,
    keychain: &dyn Fn(&StorageDir) -> KeychainProbe,
    held: &'a dyn Fn() -> bool,
) -> Result<(Option<Value>, PendingWrite<'a>), BeginError> {
    let storage = ClaudeLocks::acquire(dir, LockScope::StorageWrite)?;
    let stored = read(dir, keychain(dir)).map_err(BeginError::Store)?;
    Ok((
        stored.document,
        PendingWrite {
            dir: dir.clone(),
            target: stored.target,
            storage,
            held,
        },
    ))
}

impl<'a> PendingWrite<'a> {
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

    pub(crate) fn prove_in_home(self) -> Result<CredentialWrite<'a>, BeginError> {
        #[cfg(target_os = "macos")]
        if let Ok(ClaudeStore::File(path)) = &self.target {
            if fs::symlink_metadata(path).is_ok() {
                return Err(BeginError::Unavailable(
                    "This account's saved login is in a file, not the Keychain. Remove the account and sign in again."
                        .into(),
                ));
            }
            return Ok(CredentialWrite {
                target: WriteTarget::Keychain {
                    service: self.dir.service(),
                    account: own_account(),
                },
                storage: self.storage,
                held: self.held,
            });
        }
        self.prove()
    }
}

impl CredentialWrite<'_> {
    pub(crate) fn lost(&self) -> bool {
        (self.held)() || self.storage.lost()
    }

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

const LOCK_STALE: Duration = Duration::from_secs(60);

const CONFIG_LOCK_STALE: Duration = Duration::from_secs(10);

const STORAGE_WRITE_STALE: Duration = Duration::from_secs(15);

const CLAUDE_CODE_LIVENESS: Duration = Duration::from_millis(7500);

const HEARTBEAT: Duration = Duration::from_secs(2);

const _: () = assert!(HEARTBEAT.as_millis() * 3 <= CONFIG_LOCK_STALE.as_millis());
const _: () = assert!(HEARTBEAT.as_millis() * 3 <= CLAUDE_CODE_LIVENESS.as_millis());

#[derive(Debug, Clone, Copy)]
pub(crate) enum LockScope<'a> {
    RefreshAndConfig(&'a Path),
    StorageWrite,
}

impl LockScope<'_> {
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
    Busy,
    Unavailable(String),
}

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
        fs::create_dir_all(&dir.path).map_err(|error| LockError::Unavailable(error.to_string()))?;
        let mut locks = Self {
            held: Vec::new(),
            heartbeat: None,
            lost: Arc::new(AtomicBool::new(false)),
        };
        for (path, stale) in scope.paths(dir) {
            take(&path, stale)?;
            locks.held.push(path);
        }
        locks.heartbeat = Some(Heartbeat::start(locks.held.clone(), locks.lost.clone()));
        Ok(locks)
    }

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

fn take(path: &Path, stale: Duration) -> Result<(), LockError> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !is_stale(path, stale) {
                return Err(LockError::Busy);
            }
            let _ = fs::remove_dir(path);
            fs::create_dir(path).map_err(|_| LockError::Busy)
        }
        Err(error) => Err(LockError::Unavailable(error.to_string())),
    }
}

fn is_stale(path: &Path, stale: Duration) -> bool {
    fs::symlink_metadata(path)
        .ok()
        .filter(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| modified.elapsed().ok())
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

fn release(held: &[PathBuf], mut remove: impl FnMut(&Path)) {
    for path in held.iter().rev() {
        remove(path);
    }
}

#[cfg(test)]
mod tests;
