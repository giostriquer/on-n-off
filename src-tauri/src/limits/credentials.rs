//! What Limits makes of the CLIs' stored logins. Which store holds Claude's login is
//! `accounts::claude_store`'s question, and nothing here writes a store or refreshes a token:
//! [`super::claude_renew`] is the one module that does. Tokens live in memory only: for one
//! request, plus the Claude access token memoised in-process (`ClaudeLoginMemo`) so the Keychain
//! prompt is not repeated on every refetch — never the refresh token, never on disk.

use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use serde_json::Value;

use super::json::optional_string;
use crate::accounts::claude_store::{self, ConfigDir, KeychainProbe};
use crate::dto::LimitsAccountDto;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeIdentity {
    pub(crate) account: LimitsAccountDto,
    pub(crate) organization_id: Option<String>,
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
    /// `subscriptionType` ("pro", "max", ...).
    pub subscription_type: Option<String>,
    /// Claude Code's tier identifier; only recognized Max multipliers affect presentation.
    pub rate_limit_tier: Option<String>,
}

impl ClaudeCredential {
    pub(super) fn plan(&self) -> Option<String> {
        match (
            self.subscription_type.as_deref(),
            self.rate_limit_tier.as_deref(),
        ) {
            (Some("max"), Some("default_claude_max_5x")) => Some("max ×5".to_string()),
            (Some("max"), Some("default_claude_max_20x")) => Some("max ×20".to_string()),
            _ => self.subscription_type.clone(),
        }
    }

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
            .field("rate_limit_tier", &self.rate_limit_tier)
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

/// The stored login as it is, without renewing it. A token past its own `expiresAt` is reported as
/// `Expired` without any network call, tagged with whether the login can still renew itself.
///
/// Callers outside this module want [`super::claude_renew::current_login`], which is this plus the
/// renewal; reaching for the non-renewing one is how the expired-login message came back.
pub(crate) fn read_claude_credential(
    home: &Path,
    keychain: KeychainProbe,
    now_ms: i64,
) -> CredentialLookup<ClaudeCredential> {
    let document = match claude_store::login_document(&ConfigDir::default_in(home), keychain) {
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

/// `{"claudeAiOauth": {"accessToken", "expiresAt", "refreshTokenExpiresAt", "subscriptionType", ...}}`.
pub(crate) fn parse_claude_credential(value: &Value) -> Option<ClaudeCredential> {
    let oauth = value.get("claudeAiOauth")?;
    let token = optional_string(oauth.get("accessToken"))?;
    Some(ClaudeCredential {
        token,
        expires_at_ms: oauth.get("expiresAt").and_then(Value::as_i64),
        has_refresh_token: optional_string(oauth.get("refreshToken")).is_some(),
        refresh_expires_at_ms: oauth.get("refreshTokenExpiresAt").and_then(Value::as_i64),
        subscription_type: optional_string(oauth.get("subscriptionType")),
        rate_limit_tier: optional_string(oauth.get("rateLimitTier")),
    })
}

/// Which Claude account the CLI is signed into, from `<home>/.claude.json`'s `oauthAccount`
/// (Claude Code rewrites it on every login). `None` when the file or the fields are absent.
pub(crate) fn read_claude_identity(home: &Path) -> Option<ClaudeIdentity> {
    let native =
        crate::accounts::native::NativeStore::resolve(crate::dto::AgentId::Claude, home).ok()?;
    let value = claude_store::read_json_file(&native.config_file).ok()??;
    let account = value.get("oauthAccount")?;
    Some(ClaudeIdentity {
        account: LimitsAccountDto {
            legacy_id: None,
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
pub(crate) enum LoginSource {
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
    pub(crate) fn lookup(
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
    pub(crate) fn refreshed(
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

#[cfg(test)]
mod tests;
