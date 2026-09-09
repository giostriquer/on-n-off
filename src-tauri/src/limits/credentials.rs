//! Lookup of the CLIs' stored logins. Nothing *here* writes to the Keychain or the credential
//! files, and nothing here refreshes a token: [`super::claude_renew`] is the one module that does,
//! and it borrows this module's source precedence rather than repeating it. Tokens live in memory
//! only: for one request, plus the Claude access token memoised in-process (`ClaudeLoginMemo`) so
//! the Keychain prompt is not repeated on every refetch — never the refresh token, never on disk.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use super::json::optional_string;
use crate::dto::LimitsAccountDto;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaudeIdentity {
    pub(super) account: LimitsAccountDto,
    pub(super) organization_id: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ClaudeCredential {
    pub token: String,
    /// `expiresAt` from Claude Code's credential JSON, epoch milliseconds.
    pub expires_at_ms: Option<i64>,
    /// A `refreshToken` is stored alongside the access token. Only its presence is carried here;
    /// the token itself never enters this type, so no consumer of a `ClaudeCredential` can reach
    /// it. [`super::claude_renew`] reads it straight off the stored document instead.
    pub has_refresh_token: bool,
    /// `refreshTokenExpiresAt`, epoch milliseconds, when the login states one.
    pub refresh_expires_at_ms: Option<i64>,
    /// `subscriptionType` ("pro", "max", ...), used as the plan label.
    pub subscription_type: Option<String>,
}

impl ClaudeCredential {
    /// Whether Claude Code can renew this login by itself: it holds a refresh token that has not
    /// passed its own expiry. An access token past `expiresAt` is then only stale, not a lost
    /// login — the next `claude` run mints a new one without any sign-in.
    fn renewable(&self, now_ms: i64) -> bool {
        self.has_refresh_token && self.refresh_expires_at_ms.is_none_or(|at| at > now_ms)
    }
}

// Manual `Debug` so a stray `{:?}` (test panic, log line) can never print a token.
impl fmt::Debug for ClaudeCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClaudeCredential")
            .field("token", &"<redacted>")
            .field("expires_at_ms", &self.expires_at_ms)
            .field("has_refresh_token", &self.has_refresh_token)
            .field("refresh_expires_at_ms", &self.refresh_expires_at_ms)
            .field("subscription_type", &self.subscription_type)
            .finish()
    }
}

/// Everything `resolve` needs to know about a provider's login state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialLookup<T> {
    Found(T),
    /// No stored login for this provider.
    Missing,
    /// A stored login whose access token has passed its own expiry. `renewable` says the CLI can
    /// mint a new one from its refresh token, so no new sign-in is needed.
    Expired {
        renewable: bool,
    },
    /// A login may exist but could not be read (Keychain denied, unreadable file).
    Unreadable(String),
    /// A renewal redeemed the refresh token and then could not store the result, carrying why. The
    /// old token is spent, so the CLI cannot renew itself out of this either: only a new sign-in
    /// will do, and the user is owed the reason on the way there.
    Stranded(String),
}

/// Outcome of probing the macOS Keychain: `Ok(Some(json))` entry found, `Ok(None)` no entry,
/// `Err(why)` the entry could not be read (access denied, tool failure).
pub type KeychainProbe = Result<Option<String>, String>;

/// The one name for Claude Code's Keychain entry. Read and write share it: a second copy that
/// drifted would mean writing a renewed login to an entry nothing reads.
#[cfg(any(target_os = "macos", test))]
pub(super) const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// What one credential source (Keychain entry or file) yielded.
enum Source {
    Found(ClaudeStore, Value),
    /// Nothing stored (or JSON without an access token).
    Absent,
    /// Something is stored but could not be read or parsed.
    Broken(String),
}

/// Where a stored login lives, so a renewal writes back to the entry the next read will consult.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClaudeStore {
    /// The `Claude Code-credentials` Keychain entry, which only macOS has.
    Keychain,
    /// `<home>/.claude/.credentials.json`: the only store on Windows, and the macOS fallback.
    File(PathBuf),
}

/// The stored login document and the store holding it: Keychain entry first (macOS), then
/// `<home>/.claude/.credentials.json` (all platforms). `Ok(None)` when no source holds a login,
/// `Err` when one is there but could not be read.
///
/// The single implementation of "which store holds the Claude login". Both the read below and the
/// renewal in [`super::claude_renew`] go through it, so the two can never disagree about which
/// entry they are talking about — a second copy of this precedence would let a renewal redeem the
/// file's refresh token and write it over a Keychain entry the next read prefers.
pub(super) fn claude_login_document(
    home: &Path,
    keychain: KeychainProbe,
) -> Result<Option<(ClaudeStore, Value)>, String> {
    let path = home.join(".claude").join(".credentials.json");
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

/// The stored login as it is, without renewing it. A token past its own `expiresAt` is reported as
/// `Expired` without any network call, tagged with whether the login can still renew itself.
///
/// Callers outside this module want [`super::claude_renew::current_login`], which is this plus the
/// renewal; reaching for the non-renewing one is how the expired-login message came back.
pub(super) fn read_claude_credential(
    home: &Path,
    keychain: KeychainProbe,
    now_ms: i64,
) -> CredentialLookup<ClaudeCredential> {
    let document = match claude_login_document(home, keychain) {
        Ok(Some((_, document))) => document,
        Ok(None) => return CredentialLookup::Missing,
        Err(why) => return CredentialLookup::Unreadable(why),
    };
    let Some(credential) = parse_claude_credential(&document) else {
        return CredentialLookup::Missing;
    };
    if credential.expires_at_ms.is_some_and(|at| at <= now_ms) {
        return CredentialLookup::Expired {
            renewable: credential.renewable(now_ms),
        };
    }
    CredentialLookup::Found(credential)
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
    if parse_claude_credential(&document).is_some() {
        Source::Found(store, document)
    } else {
        Source::Absent
    }
}

/// `{"claudeAiOauth": {"accessToken", "expiresAt", "refreshTokenExpiresAt", "subscriptionType", ...}}`.
pub(super) fn parse_claude_credential(value: &Value) -> Option<ClaudeCredential> {
    let oauth = value.get("claudeAiOauth")?;
    let token = optional_string(oauth.get("accessToken"))?;
    Some(ClaudeCredential {
        token,
        expires_at_ms: oauth.get("expiresAt").and_then(Value::as_i64),
        has_refresh_token: optional_string(oauth.get("refreshToken")).is_some(),
        refresh_expires_at_ms: oauth.get("refreshTokenExpiresAt").and_then(Value::as_i64),
        subscription_type: optional_string(oauth.get("subscriptionType")),
    })
}

/// Which Claude account the CLI is signed into, from `<home>/.claude.json`'s `oauthAccount`
/// (Claude Code rewrites it on every login). `None` when the file or the fields are absent.
pub(super) fn read_claude_identity(home: &Path) -> Option<ClaudeIdentity> {
    let value = read_json_file(&home.join(".claude.json")).ok()??;
    let account = value.get("oauthAccount")?;
    Some(ClaudeIdentity {
        account: LimitsAccountDto {
            id: optional_string(account.get("accountUuid"))?,
            label: optional_string(account.get("emailAddress")),
        },
        organization_id: optional_string(account.get("organizationUuid")),
    })
}

/// Where a looked-up login came from. Claude Code rotates the access token before its recorded
/// expiry, which invalidates the one the memo is holding while the memo still believes it good —
/// so a rejected `Memo` credential is worth re-reading the store for, and retrying if that hands
/// back a login. A rejected `Stored` credential is the login's own problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoginSource {
    Memo,
    Stored,
}

/// Process-wide memo of the last good Claude login, keyed by the account it belongs to, so a
/// user who clicked "Allow" (not "Always Allow") on the Keychain prompt sees it once per app run
/// rather than on every refetch. A hit needs the same signed-in account and an unexpired token;
/// `force` (an explicit refresh) always re-reads, and the endpoint rejecting the token clears it.
pub struct ClaudeLoginMemo(Mutex<Option<(String, ClaudeCredential)>>);

impl ClaudeLoginMemo {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// The login plus where it came from, so a caller can tell a stale memo apart from a stored
    /// login the provider has actually rejected. `read` is `Fn`, not `FnOnce`: one provider read
    /// can need two lookups, and the bound is what says so at the signature rather than leaving
    /// the second call to work by accident.
    pub(super) fn lookup(
        &self,
        force: bool,
        account_id: &str,
        now_ms: i64,
        read: impl Fn() -> CredentialLookup<ClaudeCredential>,
    ) -> (CredentialLookup<ClaudeCredential>, LoginSource) {
        if !force {
            if let Some((memo_account, credential)) = self.slot().clone() {
                let expired = credential.expires_at_ms.is_some_and(|at| at <= now_ms);
                if memo_account == account_id && !expired {
                    return (CredentialLookup::Found(credential), LoginSource::Memo);
                }
            }
        }
        let result = read();
        *self.slot() = match &result {
            CredentialLookup::Found(credential) => {
                Some((account_id.to_string(), credential.clone()))
            }
            _ => None,
        };
        (result, LoginSource::Stored)
    }

    /// The stored login, read again, when it produced one worth a second attempt — the question
    /// a caller retrying past a rejected login is actually asking. Whatever it finds replaces
    /// what the memo holds, a miss included, so the next read starts from the store either way.
    pub(super) fn refreshed(
        &self,
        account_id: &str,
        now_ms: i64,
        read: impl Fn() -> CredentialLookup<ClaudeCredential>,
    ) -> Option<ClaudeCredential> {
        match self.lookup(true, account_id, now_ms, read).0 {
            CredentialLookup::Found(credential) => Some(credential),
            _ => None,
        }
    }

    pub fn clear(&self) {
        *self.slot() = None;
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<(String, ClaudeCredential)>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub static CLAUDE_LOGIN: ClaudeLoginMemo = ClaudeLoginMemo::new();

/// `Ok(None)` when the file does not exist; `Err` for any other I/O or JSON failure.
fn read_json_file(path: &Path) -> Result<Option<Value>, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<Value>(&raw)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Map `/usr/bin/security find-generic-password -w` output to a probe result. Kept pure so the
/// branching is testable on every platform; only the spawn below is macOS-specific.
#[cfg(any(target_os = "macos", test))]
fn interpret_security_outcome(success: bool, stdout: &str, stderr: &str) -> KeychainProbe {
    if success {
        let json = stdout.trim();
        return Ok((!json.is_empty()).then(|| json.to_string()));
    }
    let detail = stderr.trim();
    if detail.contains("could not be found") {
        return Ok(None);
    }
    let detail = if detail.is_empty() {
        "no details".to_string()
    } else {
        detail.to_string()
    };
    Err(format!(
        "Keychain lookup failed ({detail}). If macOS asked to allow access, click Allow and refresh."
    ))
}

/// Probe the macOS Keychain for Claude Code's login. The first call shows the system "allow
/// access" dialog, so the deadline is generous; on timeout the tool is killed.
#[cfg(target_os = "macos")]
pub fn keychain_claude_json() -> KeychainProbe {
    use crate::process::{wait_with_deadline, CommandOutcome};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let child = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", CLAUDE_KEYCHAIN_SERVICE, "-w"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run /usr/bin/security: {error}"))?;
    match wait_with_deadline(child, Duration::from_secs(90)) {
        Ok(CommandOutcome::Exited {
            success,
            stdout,
            stderr,
        }) => interpret_security_outcome(success, &stdout, &stderr),
        Ok(CommandOutcome::TimedOut) => Err("Keychain prompt was not answered in time".to_string()),
        Err(error) => Err(format!("Keychain lookup failed: {error}")),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn keychain_claude_json() -> KeychainProbe {
    Ok(None)
}

/// The account the existing entry is filed under, so a write updates *that* item.
///
/// `security add-generic-password -U` matches on service **and** account, and the read above
/// matches on service alone. Guessing the account from `$USER` would therefore create a second
/// item for the same service whenever the guess is wrong, and `find-generic-password` then
/// returns one of the two in no defined order — on-n-off and Claude Code reading different
/// logins, with nothing on screen to say so. Asking the entry itself removes the guess.
#[cfg(target_os = "macos")]
pub(super) fn keychain_claude_account() -> Option<String> {
    use crate::process::{wait_with_deadline, CommandOutcome};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    // No `-w`, so this prints attributes only and never the secret, and raises no prompt.
    let child = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", CLAUDE_KEYCHAIN_SERVICE])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    match wait_with_deadline(child, Duration::from_secs(30)) {
        Ok(CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        }) => parse_keychain_account(&stdout),
        _ => None,
    }
}

/// Pull `acct` out of `security find-generic-password`'s attribute dump. Kept pure so the parsing
/// is testable on every platform; only the spawn above is macOS-specific.
#[cfg(any(target_os = "macos", test))]
pub(super) fn parse_keychain_account(attributes: &str) -> Option<String> {
    attributes
        .lines()
        .filter_map(|line| line.trim().strip_prefix("\"acct\"<blob>=\""))
        .find_map(|rest| rest.strip_suffix('"'))
        .filter(|account| !account.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests;
