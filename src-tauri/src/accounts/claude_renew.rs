//! The access-token renewal Claude Code performs for itself, performed here when it has not run.
//!
//! Claude Code mints an access token that lives eight hours and renews it only while Claude Code
//! is running. on-n-off runs continuously and the user does not, so every gap longer than that —
//! a night, a weekend — left the Limits screen reporting an expired login at a user who was
//! signed in the whole time, with no way back except going to type in a terminal.
//!
//! This is the one place that redeems Claude's active refresh token and writes
//! the result back to Claude Code's own store. It cooperates rather than races: the same lock
//! directories in the same order, the same grant against the same client id, the same stored
//! shape, and [`claude_store`] rather than a second opinion about which store holds the login or
//! how to lock it. Claude Code is built for that — it takes the lock around every refresh of its
//! own and re-reads under it to notice one another process already did.
//!
//! **The redemption is the point of no return.** The issuer rotates the refresh token on most
//! renewals, which kills the old one as soon as the reply is written; from then until the store
//! is written, the only live credential the user has is a value on this stack. So the work is
//! ordered around that instant rather than recovered from afterwards: [`PreparedWrite::prepare`]
//! resolves and proves the write *before* the grant is sent, and everything left after it is one
//! `rename` or one `security -U`. What cannot be moved earlier is reported as
//! [`RenewError::Stranded`], which says on-n-off spent the login and could not store it, because
//! by then "run `claude` to renew it" is advice that cannot work.
//!
//! Nothing leaves this module holding a refresh token. Callers get a [`ClaudeCredential`], which
//! carries only the access token and whether a refresh token exists.

use std::path::Path;
use std::sync::Mutex;

use serde_json::{json, Value};

use super::claude_store::{
    self, ClaudeLocks, ClaudeStore, ConfigDir, KeychainProbe, LockError, LockScope, PreparedWrite,
};
use crate::http::{post_grant, HttpError};
use crate::limits::credentials::{self, ClaudeCredential, CredentialLookup};
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
/// commit — is deadline-bounded to stay inside [`claude_store::LOCK_STALE`], so Claude Code never
/// breaks a lock this still holds.
fn renew<P: Fn() -> KeychainProbe>(
    home: &Path,
    keychain: &P,
    now_ms: i64,
    token_url: &str,
    refused: &RefusedLogin,
) -> Result<ClaudeCredential, RenewError> {
    let dir = ConfigDir::default_in(home);
    let _lock = ClaudeLocks::acquire(&dir, LockScope::Refresh).map_err(|error| match error {
        LockError::Busy => RenewError::Busy,
        LockError::Unavailable(why) => RenewError::Unavailable(why),
    })?;
    let stored = claude_store::read(&dir, keychain())
        .map_err(|error| RenewError::Unavailable(error.to_string()))?;
    let mut document = stored
        .document
        .ok_or_else(|| RenewError::Unavailable("no stored Claude login to renew".to_string()))?;

    // Under the lock the store is authoritative. Another process having renewed while we waited is
    // the outcome the lock exists to produce, not a failure.
    if let Some(fresh) = credentials::parse_claude_credential(&document)
        .filter(|credential| !credential.expires_at_ms.is_some_and(|at| at <= now_ms))
    {
        refused.forget();
        return Ok(fresh);
    }
    // A Keychain that could not be read leaves no store the write can be sure Claude Code reads
    // next, so nothing is redeemed that could not then be stored.
    let target = stored.target.map_err(RenewError::Unavailable)?;
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
    let writer = PreparedWrite::prepare(&target).map_err(RenewError::Unavailable)?;

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

#[cfg(test)]
mod tests;
