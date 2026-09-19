//! The access-token renewal Claude Code performs for itself, performed here when it has not run.
//!
//! Claude Code mints an access token that lives eight hours and renews it only while Claude Code
//! is running. on-n-off runs continuously and the user does not, so every gap longer than that —
//! a night, a weekend — left the Limits screen reporting an expired login at a user who was
//! signed in the whole time, with no way back except going to type in a terminal.
//!
//! This is the one place that redeems Claude's active refresh token and writes
//! the result back to Claude Code's own store. It cooperates rather than races: the same two lock
//! directories in the same order, the same grant against the same client id, the same stored
//! shape, and [`credentials::claude_login_document`] rather than a second opinion about which
//! store holds the login. Claude Code is built for that — it takes the lock around every refresh
//! of its own and re-reads under it to notice one another process already did.
//!
//! **The redemption is the point of no return.** The issuer rotates the refresh token on most
//! renewals, which kills the old one as soon as the reply is written; from then until the store
//! is written, the only live credential the user has is a value on this stack. So the work is
//! ordered around that instant rather than recovered from afterwards: [`Writer::prepare`] resolves
//! and proves the write *before* the grant is sent, and everything left after it is one `rename`
//! or one `security -U`. What cannot be moved earlier is reported as [`RenewError::Stranded`],
//! which says on-n-off spent the login and could not store it, because by then "run `claude` to
//! renew it" is advice that cannot work.
//!
//! Nothing leaves this module holding a refresh token. Callers get a [`ClaudeCredential`], which
//! carries only the access token and whether a refresh token exists.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};

use crate::http::{post_grant, HttpError};
use crate::limits::credentials::{
    self, ClaudeCredential, ClaudeStore, CredentialLookup, KeychainProbe,
};
use crate::limits::json::optional_string;

/// Claude Code's own token endpoint and OAuth client. A refresh token is issued to one client and
/// refused to any other, so these are not ours to choose.
pub(crate) const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// What Claude Code asks for when the stored login does not name its own scopes.
const DEFAULT_SCOPES: [&str; 5] = [
    "user:profile",
    "user:inference",
    "user:sessions:claude_code",
    "user:mcp_servers",
    "user:file_upload",
];

/// Claude Code treats a refresh lock older than a minute as abandoned. Matching that is what makes
/// the two implementations take turns instead of both deciding the other is stuck — and it is the
/// budget everything between `acquire` and release has to fit inside.
const LOCK_STALE: Duration = Duration::from_secs(60);

/// Deadline for the one `security` call made while the lock is held. Comfortably inside
/// [`LOCK_STALE`], because a write that outlives the lock is a write racing whoever broke it.
#[cfg(target_os = "macos")]
const KEYCHAIN_WRITE_DEADLINE: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenewError {
    /// Another process holds Claude Code's refresh lock. Its renewal is the one that should win,
    /// and the caller keeps the message it already had rather than redeeming the same token twice.
    Busy,
    /// The issuer refused the refresh token itself. Only a new sign-in helps.
    Rejected,
    /// The renewal could not be performed and nothing was spent: an issuer that could not be
    /// reached, a store that changed under the lock, a write that could not be prepared. The login
    /// is untouched and still renewable, so the user's remedy does not change and the reason is
    /// on-n-off's problem rather than theirs.
    Unavailable(String),
    /// Redeemed, then not stored, carrying why. The user is signed out of Claude Code as a result
    /// and is the only one who can put it right, so the reason travels all the way to the screen.
    Stranded(String),
}

/// The stored login this process has already had refused, so a dead grant is sent once rather
/// than every five minutes for as long as the user leaves it alone — which would also mean taking
/// both of Claude Code's lock directories on that same schedule, forever.
///
/// Identified by its store and its timestamps rather than by the token: Claude Code rewrites
/// `expiresAt` on every renewal, so a login differing in them is a different login and worth
/// another attempt. No part of a credential is kept here.
static REFUSED: RefusedLogin = RefusedLogin::new();

/// Which stored login was refused: the store holding it, its access token's expiry, and its
/// refresh token's, when it states one.
type LoginId = (ClaudeStore, i64, Option<i64>);

pub(crate) struct RefusedLogin(Mutex<Option<LoginId>>);

impl RefusedLogin {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    fn matches(&self, login: &LoginId) -> bool {
        self.slot().as_ref() == Some(login)
    }

    fn remember(&self, login: LoginId) {
        *self.slot() = Some(login);
    }

    fn forget(&self) {
        *self.slot() = None;
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<LoginId>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// What tells one stored login from the next without holding any of it.
fn identify(store: &ClaudeStore, oauth: &Value) -> LoginId {
    (
        store.clone(),
        oauth.get("expiresAt").and_then(Value::as_i64).unwrap_or(0),
        oauth.get("refreshTokenExpiresAt").and_then(Value::as_i64),
    )
}

/// The current Claude login, renewed when the stored one has expired and can still renew itself.
///
/// This is the whole answer to "what is the Claude login right now", so every caller gets the
/// renewal — including the re-read that follows a rejected token, where the stored login may be a
/// renewable expired one rather than a dead one.
pub(crate) fn current_login<P: Fn() -> KeychainProbe>(
    home: &Path,
    keychain: &P,
    now_ms: i64,
    token_url: &str,
) -> CredentialLookup<ClaudeCredential> {
    let lookup = credentials::read_claude_credential(home, keychain(), now_ms);
    if !matches!(lookup, CredentialLookup::Expired { renewable: true }) {
        return lookup;
    }
    match renew(home, keychain, now_ms, token_url, &REFUSED) {
        Ok(credential) => CredentialLookup::Found(credential),
        // Only a new sign-in helps, so say that rather than offer a renewal that would fail again.
        Err(RenewError::Rejected) => CredentialLookup::Expired { renewable: false },
        Err(RenewError::Stranded(why)) => CredentialLookup::Stranded(why),
        // Nothing was spent and nothing is wrong with the login. Whoever holds the lock is
        // renewing now and the next poll reads what they wrote; an issuer we could not reach will
        // be there next time; a write we could not prepare is ours to fix, not theirs. In every
        // case "send a prompt with `claude` to renew it" is still the accurate advice, and it is
        // the message the user already had.
        Err(RenewError::Busy | RenewError::Unavailable(_)) => {
            CredentialLookup::Expired { renewable: true }
        }
    }
}

/// Redeem the stored refresh token for a new access token and write it back.
///
/// The store is read *under* the lock, Keychain probe included: acting on what it says while
/// holding the lock is the whole mechanism for not redeeming a token another process has already
/// replaced. Everything the lock covers — the probe, the write's preparation, the grant, the
/// commit — is deadline-bounded to stay inside [`LOCK_STALE`], so Claude Code never breaks a lock
/// this still holds.
fn renew<P: Fn() -> KeychainProbe>(
    home: &Path,
    keychain: &P,
    now_ms: i64,
    token_url: &str,
    refused: &RefusedLogin,
) -> Result<ClaudeCredential, RenewError> {
    let _lock = RefreshLock::acquire(home)?;
    let (target, mut document) = credentials::claude_login_document(home, keychain())
        .map_err(RenewError::Unavailable)?
        .ok_or_else(|| RenewError::Unavailable("no stored Claude login to renew".to_string()))?;

    // Under the lock the store is authoritative. Another process having renewed while we waited is
    // the outcome the lock exists to produce, not a failure.
    if let Some(fresh) = credentials::parse_claude_credential(&document)
        .filter(|credential| !credential.expires_at_ms.is_some_and(|at| at <= now_ms))
    {
        refused.forget();
        return Ok(fresh);
    }
    let oauth = document
        .get("claudeAiOauth")
        .ok_or_else(|| RenewError::Unavailable("stored login has no claudeAiOauth".to_string()))?;
    let identity = identify(&target, oauth);
    if refused.matches(&identity) {
        return Err(RenewError::Rejected);
    }
    // The read only reaches a renewal when it saw a refresh token, so its absence here means the
    // store changed under us into one that needs a sign-in rather than a refresh.
    let refresh_token = optional_string(oauth.get("refreshToken")).ok_or(RenewError::Rejected)?;
    let scopes = scopes(oauth);

    // Everything about the write that can fail for reasons unrelated to the reply fails here,
    // where failing costs nothing. What is left afterwards is one rename or one `security -U`.
    let writer = Writer::prepare(&target).map_err(RenewError::Unavailable)?;

    let reply = post_grant(token_url, &request_body(&refresh_token, &scopes)).map_err(|error| {
        match error {
            // 400 `invalid_grant` is how the issuer says the refresh token is spent or revoked.
            HttpError::Unauthorized | HttpError::Status(400) => {
                refused.remember(identity.clone());
                RenewError::Rejected
            }
            // A success we could not read. `post_grant` only parses on 2xx, so the issuer very
            // likely did rotate the token and the reply carrying it is the part we lost.
            HttpError::Parse(why) => RenewError::Stranded(format!("unreadable token reply: {why}")),
            other => RenewError::Unavailable(other.to_string()),
        }
    })?;

    // Past here the old refresh token is dead and `document` holds the only live one.
    apply(&mut document, &reply, now_ms).map_err(RenewError::Stranded)?;
    let raw = serde_json::to_string(&document)
        .map_err(|error| RenewError::Stranded(error.to_string()))?;
    writer.commit(&raw).map_err(RenewError::Stranded)?;
    refused.forget();
    // Parsed back out of what was written, so the credential returned is provably the stored one.
    credentials::parse_claude_credential(&document)
        .ok_or_else(|| RenewError::Stranded("the renewed login did not parse".to_string()))
}

/// The saved-account owner has already persisted an encrypted renewal intent and proved
/// exclusive ownership of this never-activated login. Native renewal remains under its locks.
pub(super) fn renew_private(auth: &Value, now_ms: i64, token_url: &str) -> Result<Value, String> {
    let oauth = auth
        .get("claudeAiOauth")
        .ok_or("Missing private Claude login.")?;
    let token =
        optional_string(oauth.get("refreshToken")).ok_or("Missing private renewal token.")?;
    let reply = post_grant(token_url, &request_body(&token, &scopes(oauth)))
        .map_err(|_| "Could not renew the private Claude login. Sign in again if needed.")?;
    let mut auth = auth.clone();
    apply(&mut auth, &reply, now_ms)
        .map_err(|_| "The private renewal reply was incomplete. Sign in again.")?;
    Ok(auth)
}

/// A store write resolved and proven before anything is redeemed.
///
/// `prepare` does the parts that can fail on their own account: reading the Keychain entry's
/// account, or creating the private temporary the file store renames into. `commit` is then the
/// single irreversible step, small enough to sit comfortably inside the refresh lock's minute.
struct Writer {
    target: WriterTarget,
}

enum WriterTarget {
    Keychain { account: String },
    File { path: PathBuf, temporary: PathBuf },
}

impl Writer {
    fn prepare(target: &ClaudeStore) -> Result<Self, String> {
        let target = match target {
            ClaudeStore::Keychain => WriterTarget::Keychain {
                account: keychain_account()?,
            },
            ClaudeStore::File(path) => {
                let temporary = path.with_extension("json.on-n-off");
                // Created empty and private now, so a directory that will not take it says so
                // before the grant rather than after.
                write_private(&temporary, "")
                    .map_err(|error| format!("{}: {error}", temporary.display()))?;
                WriterTarget::File {
                    path: path.clone(),
                    temporary,
                }
            }
        };
        Ok(Self { target })
    }

    fn commit(self, raw: &str) -> Result<(), String> {
        match &self.target {
            WriterTarget::Keychain { account } => {
                write_keychain(account, credentials::CLAUDE_KEYCHAIN_SERVICE, raw)
            }
            WriterTarget::File { path, temporary } => {
                write_private(temporary, raw)
                    .map_err(|error| format!("{}: {error}", temporary.display()))?;
                fs::rename(temporary, path).map_err(|error| format!("{}: {error}", path.display()))
            }
        }
    }
}

impl Drop for Writer {
    /// The temporary never outlives the attempt. A successful commit renamed it away, and a failed
    /// one leaves a live refresh token in a file Claude Code does not know about and will never
    /// rotate — the same objection that keeps `ConfigIo` out of this module. The user has to sign
    /// in again either way; an orphaned secret does not help them do it.
    fn drop(&mut self) {
        if let WriterTarget::File { temporary, .. } = &self.target {
            let _ = fs::remove_file(temporary);
        }
    }
}

/// The grant Claude Code sends, field for field. `scope` is required: the issuer narrows a refresh
/// to the scopes asked for, and a login that came back without them is one the usage endpoint
/// would refuse.
fn request_body(refresh_token: &str, scopes: &[String]) -> Value {
    json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLIENT_ID,
        "scope": scopes.join(" "),
    })
}

fn scopes(oauth: &Value) -> Vec<String> {
    let stored: Vec<String> = oauth
        .get("scopes")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if stored.is_empty() {
        return DEFAULT_SCOPES
            .iter()
            .map(|scope| (*scope).to_string())
            .collect();
    }
    stored
}

/// Fold the reply into the stored document.
///
/// Only the fields the reply carries are replaced. Everything else the login knows —
/// `subscriptionType`, `rateLimitTier`, anything a newer Claude Code has added — is left alone,
/// because this write has to leave a document Claude Code still recognises as its own.
fn apply(document: &mut Value, reply: &Value, now_ms: i64) -> Result<(), String> {
    let access_token =
        optional_string(reply.get("access_token")).ok_or("token reply carried no access_token")?;
    let expires_at_ms = seconds(reply.get("expires_in"))
        .map(|seconds| now_ms + seconds * 1000)
        .ok_or("token reply carried no expires_in")?;
    let refresh_expires_at_ms =
        seconds(reply.get("refresh_token_expires_in")).map(|seconds| now_ms + seconds * 1000);
    let rotated = optional_string(reply.get("refresh_token"));
    let granted: Option<Vec<Value>> = optional_string(reply.get("scope")).map(|scope| {
        scope
            .split_whitespace()
            .map(|entry| Value::String(entry.to_string()))
            .collect()
    });

    let oauth = document
        .get_mut("claudeAiOauth")
        .and_then(Value::as_object_mut)
        .ok_or("stored login is not a claudeAiOauth object")?;
    oauth.insert("accessToken".to_string(), Value::String(access_token));
    oauth.insert("expiresAt".to_string(), json!(expires_at_ms));
    if let Some(rotated) = rotated {
        oauth.insert("refreshToken".to_string(), Value::String(rotated));
    }
    if let Some(at) = refresh_expires_at_ms {
        oauth.insert("refreshTokenExpiresAt".to_string(), json!(at));
    }
    if let Some(granted) = granted {
        oauth.insert("scopes".to_string(), Value::Array(granted));
    }
    Ok(())
}

fn seconds(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64).filter(|seconds| *seconds > 0)
}

/// The credentials file holds a refresh token, so it is created 0600 and never handed to
/// `ConfigIo`, whose backups would copy the secret somewhere Claude Code neither knows about nor
/// rotates.
#[cfg(unix)]
fn write_private(path: &Path, raw: &str) -> std::io::Result<()> {
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
fn write_private(path: &Path, raw: &str) -> std::io::Result<()> {
    fs::write(path, raw)
}

/// The account Claude Code files its entry under, read off the entry rather than guessed. `-U`
/// matches on service *and* account, so a wrong guess would file a second item under the same
/// service, and the read — which matches on service alone — could then return either one.
#[cfg(target_os = "macos")]
fn keychain_account() -> Result<String, String> {
    credentials::keychain_claude_account()
        .ok_or_else(|| "could not read the Claude Keychain entry's account".to_string())
}

/// Mirrors `keychain_claude_json`'s stub: these platforms have no such entry, and the file store
/// is the one their read will have chosen.
#[cfg(not(target_os = "macos"))]
fn keychain_account() -> Result<String, String> {
    Err("this platform has no Claude Code Keychain entry".to_string())
}

/// `security -i` reads the command from stdin, which is the whole point: the token would otherwise
/// sit in the process table for every other user on the machine to read. `-U` updates the entry in
/// place, and `-X` takes the JSON hex-encoded, as Claude Code writes it.
#[cfg(target_os = "macos")]
fn write_keychain(account: &str, service: &str, raw: &str) -> Result<(), String> {
    use crate::process::{wait_with_deadline, CommandOutcome};
    use std::io::Write;
    use std::process::{Command, Stdio};

    let command = keychain_command(account, service, raw);
    let mut child = Command::new("/usr/bin/security")
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run /usr/bin/security: {error}"))?;
    // Bound and dropped explicitly: `security` reads commands until stdin closes, so the handle
    // has to go before the wait. One command of a few kilobytes cannot fill a pipe buffer, so
    // writing it before the drainers start cannot deadlock.
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "security took no stdin".to_string())?;
    let written = stdin.write_all(command.as_bytes());
    drop(stdin);
    written.map_err(|error| format!("could not write to security: {error}"))?;
    match wait_with_deadline(child, KEYCHAIN_WRITE_DEADLINE) {
        Ok(CommandOutcome::Exited { success: true, .. }) => Ok(()),
        Ok(CommandOutcome::Exited { stderr, .. }) => {
            Err(format!("Keychain write failed ({})", stderr.trim()))
        }
        Ok(CommandOutcome::TimedOut) => Err("Keychain write was not answered in time".to_string()),
        Err(error) => Err(format!("Keychain write failed: {error}")),
    }
}

#[cfg(not(target_os = "macos"))]
fn write_keychain(_account: &str, _service: &str, _raw: &str) -> Result<(), String> {
    Err("this platform has no Claude Code Keychain entry".to_string())
}

/// Kept pure so the quoting is testable on every platform; only the spawn above is macOS-only.
#[cfg(any(target_os = "macos", test))]
fn keychain_command(account: &str, service: &str, raw: &str) -> String {
    let hex: String = raw.bytes().map(|byte| format!("{byte:02x}")).collect();
    format!("add-generic-password -U -a \"{account}\" -s \"{service}\" -X \"{hex}\"\n")
}

/// Claude Code's two refresh locks, held for one renewal and released when this value drops.
///
/// They are directories, taken with `mkdir` because that is atomic on every filesystem either
/// implementation runs on. Every step taken while they are held is deadline-bounded to stay well
/// inside [`LOCK_STALE`], so there is no heartbeat to keep up and no window in which the other
/// side breaks a lock this process still believes it holds.
#[derive(Debug)]
struct RefreshLock {
    held: Vec<PathBuf>,
}

impl RefreshLock {
    fn acquire(home: &Path) -> Result<Self, RenewError> {
        Self::acquire_at(home, SystemTime::now())
    }

    /// `now` is a parameter so the staleness branch is reachable from a test without waiting a
    /// minute or backdating a directory the filesystem may not let us touch.
    fn acquire_at(home: &Path, now: SystemTime) -> Result<Self, RenewError> {
        let config = home.join(".claude");
        // Same order as Claude Code: two processes that disagree about the order deadlock.
        let paths = [
            config.join(".oauth_refresh.lock"),
            home.join(".claude.lock"),
        ];
        let mut lock = Self { held: Vec::new() };
        for path in paths {
            // `?` drops `lock`, which releases whatever it had already taken.
            take(&path, now)?;
            lock.held.push(path);
        }
        Ok(lock)
    }
}

fn take(path: &Path, now: SystemTime) -> Result<(), RenewError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| RenewError::Unavailable(error.to_string()))?;
    }
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !is_stale(path, now) {
                return Err(RenewError::Busy);
            }
            // Whoever left it behind is gone; Claude Code breaks an abandoned lock the same way,
            // and this inherits the same race between two processes that both judged it stale.
            let _ = fs::remove_dir(path);
            fs::create_dir(path).map_err(|_| RenewError::Busy)
        }
        Err(error) => Err(RenewError::Unavailable(error.to_string())),
    }
}

fn is_stale(path: &Path, now: SystemTime) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|meta| meta.modified()) else {
        return false;
    };
    now.duration_since(modified)
        .is_ok_and(|age| age > LOCK_STALE)
}

impl Drop for RefreshLock {
    fn drop(&mut self) {
        for path in self.held.drain(..).rev() {
            let _ = fs::remove_dir(path);
        }
    }
}

#[cfg(test)]
mod tests;
