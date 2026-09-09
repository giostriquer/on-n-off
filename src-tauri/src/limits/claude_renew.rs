//! The access-token renewal Claude Code performs for itself, performed here when it has not run.
//!
//! Claude Code mints an access token that lives eight hours and renews it only while Claude Code
//! is running. on-n-off runs continuously and the user does not, so every gap longer than that —
//! a night, a weekend — left the Limits screen reporting an expired login at a user who was
//! signed in the whole time, with no way back except going to type in a terminal.
//!
//! This is the one place in `limits/` that reads Claude's refresh token, redeems it, and writes
//! the result back to Claude Code's own store. It cooperates rather than races: the same two lock
//! directories in the same order, the same grant against the same client id, the same stored
//! shape. Claude Code is built for that — it takes the lock around every refresh of its own and
//! re-reads afterwards to notice one another process already did.
//!
//! Nothing leaves this module holding a refresh token. Callers get a [`ClaudeCredential`], which
//! carries only the access token and whether a refresh token exists.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};

use super::credentials::{ClaudeCredential, KeychainProbe};
use super::json::optional_string;
use crate::http::{post_grant, HttpError};

/// Claude Code's own token endpoint and OAuth client. A refresh token is issued to one client and
/// refused to any other, so these are not ours to choose.
pub(super) const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
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
/// the two implementations take turns instead of both deciding the other is stuck.
const LOCK_STALE: Duration = Duration::from_secs(60);

#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RenewError {
    /// Another process holds Claude Code's refresh lock. Its renewal is the one that should win,
    /// and the caller keeps the message it already had rather than redeeming the same token twice.
    Busy,
    /// The stored login has no refresh token to redeem.
    NoRefreshToken,
    /// The issuer refused the refresh token itself. Only a new sign-in helps, so the caller says
    /// so instead of offering to renew.
    Rejected,
    /// The network, the store, or a reply whose shape we do not recognise.
    Failed(String),
}

/// Where the login is kept, so a renewal writes it back where the next read will look.
enum Store {
    /// macOS keeps it in the `Claude Code-credentials` Keychain entry.
    #[cfg(target_os = "macos")]
    Keychain,
    /// `<home>/.claude/.credentials.json`: the only store on Windows, and the macOS fallback.
    File(PathBuf),
}

/// Redeem the stored refresh token for a new access token and write it back.
///
/// The probe is taken again rather than reused: between noticing the expiry and holding the lock,
/// Claude Code may have run and renewed the login itself, and the point of the lock is to act on
/// what the store says under it.
pub(super) fn renew(
    home: &Path,
    keychain: KeychainProbe,
    now_ms: i64,
    token_url: &str,
) -> Result<ClaudeCredential, RenewError> {
    let _lock = RefreshLock::acquire(home)?;
    let (store, mut document) = locate(home, keychain)?;
    let oauth = document
        .get("claudeAiOauth")
        .ok_or(RenewError::NoRefreshToken)?;
    // Under the lock the store is authoritative. Someone else renewing while we waited is the
    // outcome the lock exists to produce, not a failure.
    if let Some(fresh) = unexpired(oauth, now_ms) {
        return Ok(fresh);
    }
    let refresh_token =
        optional_string(oauth.get("refreshToken")).ok_or(RenewError::NoRefreshToken)?;
    let scopes = scopes(oauth);

    let reply = post_grant(token_url, &request_body(&refresh_token, &scopes)).map_err(|error| {
        match error {
            // 400 `invalid_grant` is how the issuer says the refresh token is spent or revoked.
            HttpError::Unauthorized | HttpError::Status(400) => RenewError::Rejected,
            other => RenewError::Failed(other.to_string()),
        }
    })?;

    let credential = apply(&mut document, &reply, now_ms).map_err(RenewError::Failed)?;
    store.write(&document).map_err(RenewError::Failed)?;
    Ok(credential)
}

/// The grant Claude Code sends, field for field. `scope` is required: the issuer narrows a
/// refresh to the scopes asked for, and a login that came back without them is one the usage
/// endpoint would refuse.
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
                .map(Some)
                .filter_map(optional_string)
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

/// Fold the reply into the stored document and describe what the store now holds.
///
/// Only the fields the reply carries are replaced. Everything else the login knows —
/// `subscriptionType`, `rateLimitTier`, anything a newer Claude Code has added — is left alone,
/// because this write has to leave a document Claude Code still recognises as its own.
fn apply(document: &mut Value, reply: &Value, now_ms: i64) -> Result<ClaudeCredential, String> {
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
    oauth.insert(
        "accessToken".to_string(),
        Value::String(access_token.clone()),
    );
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
    Ok(ClaudeCredential {
        token: access_token,
        expires_at_ms: Some(expires_at_ms),
        has_refresh_token: optional_string(oauth.get("refreshToken")).is_some(),
        refresh_expires_at_ms: oauth.get("refreshTokenExpiresAt").and_then(Value::as_i64),
        subscription_type: optional_string(oauth.get("subscriptionType")),
    })
}

fn seconds(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64).filter(|seconds| *seconds > 0)
}

/// The stored login when its access token is still good, so a renewal that lost the race to
/// another process reports the winner's token rather than redeeming a second one.
fn unexpired(oauth: &Value, now_ms: i64) -> Option<ClaudeCredential> {
    let token = optional_string(oauth.get("accessToken"))?;
    let expires_at_ms = oauth.get("expiresAt").and_then(Value::as_i64);
    if expires_at_ms.is_some_and(|at| at <= now_ms) {
        return None;
    }
    Some(ClaudeCredential {
        token,
        expires_at_ms,
        has_refresh_token: optional_string(oauth.get("refreshToken")).is_some(),
        refresh_expires_at_ms: oauth.get("refreshTokenExpiresAt").and_then(Value::as_i64),
        subscription_type: optional_string(oauth.get("subscriptionType")),
    })
}

/// The stored login and its store, in the order `read_claude_credential` consults them, so the
/// renewal writes back to the source the next read will actually use.
fn locate(home: &Path, keychain: KeychainProbe) -> Result<(Store, Value), RenewError> {
    #[cfg(target_os = "macos")]
    if let Ok(Some(raw)) = keychain {
        let document =
            serde_json::from_str(&raw).map_err(|error| RenewError::Failed(error.to_string()))?;
        return Ok((Store::Keychain, document));
    }
    #[cfg(not(target_os = "macos"))]
    drop(keychain);

    let path = home.join(".claude").join(".credentials.json");
    let raw = fs::read_to_string(&path)
        .map_err(|error| RenewError::Failed(format!("{}: {error}", path.display())))?;
    let document =
        serde_json::from_str(&raw).map_err(|error| RenewError::Failed(error.to_string()))?;
    Ok((Store::File(path), document))
}

impl Store {
    fn write(&self, document: &Value) -> Result<(), String> {
        let raw = serde_json::to_string(document).map_err(|error| error.to_string())?;
        match self {
            #[cfg(target_os = "macos")]
            Self::Keychain => write_keychain(&raw),
            Self::File(path) => write_file(path, &raw),
        }
    }
}

/// Replace the credentials file without ever leaving a half-written login on disk: a private
/// temporary beside it, then one rename. The file holds a refresh token, so it is created 0600
/// and never handed to `ConfigIo`, whose backups would copy the secret somewhere Claude Code
/// does not know about and does not clean up.
fn write_file(path: &Path, raw: &str) -> Result<(), String> {
    let temporary = path.with_extension("json.on-n-off");
    write_private(&temporary, raw).map_err(|error| format!("{}: {error}", temporary.display()))?;
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(format!("{}: {error}", path.display()))
        }
    }
}

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

/// `security -i` reads the command from stdin, which is the whole point: the token would
/// otherwise sit in the process table for every other user on the machine to read. `-U` updates
/// the entry in place, and `-X` takes the JSON hex-encoded, as Claude Code writes it.
#[cfg(target_os = "macos")]
fn write_keychain(raw: &str) -> Result<(), String> {
    use crate::process::{wait_with_deadline, CommandOutcome};
    use std::io::Write;
    use std::process::{Command, Stdio};

    let command = keychain_command(&keychain_account(), KEYCHAIN_SERVICE, raw);
    let mut child = Command::new("/usr/bin/security")
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run /usr/bin/security: {error}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "security took no stdin".to_string())?
        .write_all(command.as_bytes())
        .map_err(|error| format!("could not write to security: {error}"))?;
    match wait_with_deadline(child, Duration::from_secs(30)) {
        Ok(CommandOutcome::Exited { success: true, .. }) => Ok(()),
        Ok(CommandOutcome::Exited { stderr, .. }) => {
            Err(format!("Keychain write failed ({})", stderr.trim()))
        }
        Ok(CommandOutcome::TimedOut) => Err("Keychain write was not answered in time".to_string()),
        Err(error) => Err(format!("Keychain write failed: {error}")),
    }
}

/// Kept pure so the quoting is testable on every platform; only the spawn above is macOS-only.
#[cfg(any(target_os = "macos", test))]
fn keychain_command(account: &str, service: &str, raw: &str) -> String {
    let hex: String = raw.bytes().map(|byte| format!("{byte:02x}")).collect();
    format!("add-generic-password -U -a \"{account}\" -s \"{service}\" -X \"{hex}\"\n")
}

/// The account Claude Code files the entry under.
#[cfg(target_os = "macos")]
fn keychain_account() -> String {
    keychain_account_from(&std::env::var("USER").unwrap_or_default()).to_string()
}

/// `$USER`, and Claude Code's fixed fallback when that is empty or holds anything the Keychain
/// would not take as an account name. Reading the variable is the caller's job so this stays a
/// function about the rule rather than about the environment.
#[cfg(any(target_os = "macos", test))]
fn keychain_account_from(name: &str) -> &str {
    let acceptable = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if acceptable {
        name
    } else {
        "claude-code-user"
    }
}

/// Claude Code's two refresh locks, held for one renewal and released when this value drops.
///
/// They are directories, taken with `mkdir` because that is atomic on every filesystem either
/// implementation runs on. The hold is bounded by one HTTP request and one store write, well
/// inside the minute after which the other side would break it as abandoned, so there is no
/// heartbeat to keep up.
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
        fs::create_dir_all(parent).map_err(|error| RenewError::Failed(error.to_string()))?;
    }
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !is_stale(path, now) {
                return Err(RenewError::Busy);
            }
            // Whoever left it behind is gone; Claude Code breaks an abandoned lock the same way.
            let _ = fs::remove_dir(path);
            fs::create_dir(path).map_err(|_| RenewError::Busy)
        }
        Err(error) => Err(RenewError::Failed(error.to_string())),
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
