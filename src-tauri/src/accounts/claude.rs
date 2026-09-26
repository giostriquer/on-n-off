//! The Claude adapter: Claude's half of the accounts seam. It reads a Claude login by Claude
//! Code's rules ([`ClaudeLogin`]): `claudeAiOauth` as Claude Code stores it, beside the
//! `oauthAccount` record that says who it is.
use super::{
    claude_renew,
    model::{self, Identity, LoginView},
    store::Login,
};
use crate::dto::AgentId;
use crate::limits::credentials::{parse_claude_credential, ClaudeCredential};
use crate::limits::json::optional_string;
use serde_json::Value;

/// Claude's adapter.
pub(super) struct Claude;

impl super::Adapter for Claude {
    fn login<'a>(&self, login: &'a Login) -> Box<dyn LoginView + 'a> {
        Box::new(ClaudeLogin::of(login))
    }
}

/// A Claude login read by Claude Code's rules.
pub(crate) struct ClaudeLogin<'a> {
    auth: &'a Value,
    account: &'a Value,
}

/// The account record of a login read from its credential alone.
static NO_ACCOUNT: Value = Value::Null;

impl<'a> ClaudeLogin<'a> {
    pub(crate) fn of(login: &'a Login) -> Self {
        Self {
            auth: &login.auth,
            account: &login.account,
        }
    }

    /// The credential alone, as a saved account's usage read holds it.
    pub(crate) fn of_auth(auth: &'a Value) -> Self {
        Self {
            auth,
            account: &NO_ACCOUNT,
        }
    }

    /// What Limits reads with: the access token, its expiry and the plan. `None` for a login
    /// Claude Code has signed out of, which empties `claudeAiOauth` of its access token.
    pub(crate) fn credential(&self) -> Option<ClaudeCredential> {
        parse_claude_credential(self.auth)
    }

    /// The grant a private renewal sends: the refresh token, for the client and scopes the login
    /// was issued (`claude_renew` sends it).
    pub(super) fn renewal_request(&self) -> Result<Value, String> {
        let oauth = self
            .auth
            .get("claudeAiOauth")
            .ok_or("Missing private Claude login.")?;
        let token =
            optional_string(oauth.get("refreshToken")).ok_or("Missing private renewal token.")?;
        Ok(claude_renew::request_body(
            &token,
            &claude_renew::scopes(oauth),
            claude_renew::client_id(oauth),
        ))
    }

    /// This login with the grant's `reply` folded in, as Claude Code folds it; the account record
    /// is unchanged.
    pub(super) fn renewed(&self, reply: &Value, now_ms: i64) -> Result<Login, String> {
        let mut auth = self.auth.clone();
        claude_renew::apply(&mut auth, reply, now_ms)
            .map_err(|_| "The private renewal reply was incomplete. Sign in again.")?;
        Ok(Login {
            auth,
            account: self.account.clone(),
        })
    }
}

impl LoginView for ClaudeLogin<'_> {
    fn identity(&self) -> Result<Identity, String> {
        model::string(self.auth, "/claudeAiOauth/accessToken")?;
        model::string(self.auth, "/claudeAiOauth/refreshToken")?;
        Ok(Identity {
            provider: AgentId::Claude,
            user_id: model::string(self.account, "/accountUuid")?.to_owned(),
            workspace_id: model::string(self.account, "/organizationUuid")?.to_owned(),
        })
    }

    fn email(&self) -> Option<String> {
        self.account
            .get("emailAddress")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|email| !email.is_empty())
            .map(str::to_owned)
    }

    fn fingerprint(&self) -> String {
        model::fingerprint([
            self.auth.pointer("/claudeAiOauth/accessToken"),
            self.auth.pointer("/claudeAiOauth/refreshToken"),
            None,
            None,
        ])
    }

    /// Once its access token's `expiresAt` is reached; never for a login that states none.
    fn renewal_due(&self, now_ms: i64) -> bool {
        self.auth
            .pointer("/claudeAiOauth/expiresAt")
            .and_then(Value::as_i64)
            .is_some_and(|expires_at| expires_at <= now_ms)
    }

    fn renew_private(&self, now_ms: i64) -> Result<Login, String> {
        claude_renew::renew_private(self, now_ms, claude_renew::TOKEN_URL)
    }
}

#[cfg(test)]
mod tests;
