//! What a Claude login and a Claude config dir say: the shape of a stored `claudeAiOauth`, which the
//! account switch reads to tell a login from Claude Code's emptied sign-out, and the account a
//! config dir's `.claude.json` names, which every Claude usage read checks. Nothing here reads a
//! store, writes one or sends a token.

use std::fmt;
use std::path::Path;

use serde_json::Value;

use super::json::optional_string;
use crate::accounts::model::Identity;
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
    /// the token itself never enters this type.
    pub has_refresh_token: bool,
    /// `refreshTokenExpiresAt`, epoch milliseconds, when the login states one.
    pub refresh_expires_at_ms: Option<i64>,
    /// `subscriptionType` ("pro", "max", ...).
    pub subscription_type: Option<String>,
    /// Claude Code's tier identifier; only recognized Max multipliers affect presentation.
    pub rate_limit_tier: Option<String>,
}

/// The plan a card shows for a `subscription_type` ("pro", "max", ...) at Claude Code's `tier`: a
/// recognized Max multiplier, else the subscription type as it is.
fn plan_label(subscription_type: Option<&str>, tier: Option<&str>) -> Option<String> {
    match (subscription_type, tier) {
        (Some("max"), Some("default_claude_max_5x")) => Some("max ×5".to_string()),
        (Some("max"), Some("default_claude_max_20x")) => Some("max ×20".to_string()),
        _ => subscription_type.map(str::to_string),
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

/// What a Claude config file's `oauthAccount` says of its account.
pub(super) struct ClaudeConfigAccount {
    pub(super) identity: ClaudeIdentity,
    /// The plan its organization is on.
    pub(super) plan: Option<String>,
}

/// The account the Claude config file at `config_file` names, if it names one.
pub(super) fn read_claude_config_account(config_file: &Path) -> Option<ClaudeConfigAccount> {
    let value = read_json_file(config_file).ok()??;
    let account = value.get("oauthAccount")?;
    // `organizationType` names the subscription as `claude_<type>`, where `<type>` is what the
    // stored login's `subscriptionType` says.
    let organization_type = optional_string(account.get("organizationType"));
    let tier = optional_string(account.get("organizationRateLimitTier"));
    Some(ClaudeConfigAccount {
        identity: ClaudeIdentity {
            account: LimitsAccountDto {
                legacy_id: None,
                id: optional_string(account.get("accountUuid"))?,
                label: optional_string(account.get("emailAddress")),
            },
            organization_id: optional_string(account.get("organizationUuid")),
        },
        plan: plan_label(
            organization_type
                .as_deref()
                .and_then(|kind| kind.strip_prefix("claude_")),
            tier.as_deref(),
        ),
    })
}

impl ClaudeIdentity {
    /// Whether this is `identity`'s user in `identity`'s workspace.
    pub(super) fn names(&self, identity: &Identity) -> bool {
        self.account.id == identity.user_id
            && self.organization_id.as_deref() == Some(identity.workspace_id.as_str())
    }
}

/// `Ok(None)` when the file does not exist; `Err` for any other I/O or JSON failure.
fn read_json_file(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<Value>(&raw)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests;
