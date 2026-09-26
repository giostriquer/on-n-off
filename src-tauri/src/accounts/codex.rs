//! The Codex adapter: Codex's half of the accounts seam. It reads a Codex login by Codex's rules
//! ([`CodexLogin`]): `auth.json` as Codex writes it, whose ID token's claims say who it is.
use super::{
    model::{self, AccessToken, Identity, LoginView},
    store::{Login, Store},
    usage_renew,
};
use crate::dto::AgentId;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;
use std::path::Path;

/// Codex's adapter.
pub(super) struct Codex;

impl super::Adapter for Codex {
    fn native(&self, home: &Path) -> Result<Box<dyn super::NativeAccount>, String> {
        Ok(Box::new(super::native::NativeStore::resolve(home)?))
    }

    fn login<'a>(&self, login: &'a Login) -> Box<dyn LoginView + 'a> {
        Box::new(CodexLogin::of(login))
    }
}

/// A Codex login read by Codex's rules.
pub(crate) struct CodexLogin<'a> {
    auth: &'a Value,
    account: &'a Value,
}

/// The account record of a login read from its credential alone. Codex keeps none.
static NO_ACCOUNT: Value = Value::Null;

impl<'a> CodexLogin<'a> {
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

    /// The access token, for one request header; `None` for a login without one.
    pub(crate) fn access_token(&self) -> Option<AccessToken> {
        model::string(self.auth, "/tokens/access_token")
            .ok()
            .map(AccessToken::new)
    }

    /// The workspace the tokens are for, `tokens.account_id`, when it names one.
    pub(super) fn workspace(&self) -> Option<&'a str> {
        self.auth
            .pointer("/tokens/account_id")
            .and_then(Value::as_str)
            .filter(|workspace| !workspace.trim().is_empty())
    }

    /// Whether the login carries an ID token at all, readable or not.
    pub(super) fn has_id_token(&self) -> bool {
        self.auth.pointer("/tokens/id_token").is_some()
    }

    /// The ID token's `https://api.openai.com/auth` claims: who the login is, its workspace and its
    /// plan, and never a token. `None` when the token has no such claims; an error when there is
    /// no readable ID token.
    pub(crate) fn auth_claims(&self) -> Result<Option<Value>, String> {
        Ok(self.claims()?.get("https://api.openai.com/auth").cloned())
    }

    /// Whether a running Codex client may renew this login within ten minutes of `now` (Unix
    /// seconds). Codex renews shortly before expiry, five minutes in its source, and the desktop
    /// app sooner. An unreadable expiry counts as soon.
    pub(super) fn renews_soon(&self, now: i64) -> bool {
        token_claims(self.auth, "/tokens/access_token")
            .ok()
            .and_then(|claims| claims.get("exp")?.as_i64())
            .is_none_or(|expiry| expiry < now + 600)
    }

    /// The grant a private renewal sends: Codex's own JSON refresh grant, as its
    /// `login/src/oauth/client.rs` sends it (`usage_renew` sends it).
    pub(super) fn renewal_request(&self) -> Result<Value, String> {
        let token = model::string(self.auth, "/tokens/refresh_token")?;
        Ok(serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": token,
            "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
        }))
    }

    /// This login with the grant's `reply` folded in: the access token, and the refresh and ID
    /// tokens when the reply carries them, replace the stored ones, `last_refresh` records `now_ms`,
    /// and every other field is left as it was.
    pub(super) fn renewed(&self, reply: &Value, now_ms: i64) -> Result<Login, String> {
        let mut auth = self.auth.clone();
        let access = model::string(reply, "/access_token")?.to_owned();
        auth["tokens"]["access_token"] = Value::String(access);
        for name in ["refresh_token", "id_token"] {
            if let Some(value) = reply
                .get(name)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                auth["tokens"][name] = Value::String(value.to_owned());
            }
        }
        auth["last_refresh"] = Value::String(
            chrono::DateTime::from_timestamp_millis(now_ms)
                .ok_or("Invalid renewal time.")?
                .to_rfc3339(),
        );
        Ok(Login {
            auth,
            account: self.account.clone(),
        })
    }

    /// The ID token's claims.
    fn claims(&self) -> Result<Value, String> {
        token_claims(self.auth, "/tokens/id_token")
    }
}

impl LoginView for CodexLogin<'_> {
    fn identity(&self) -> Result<Identity, String> {
        model::string(self.auth, "/tokens/access_token")?;
        model::string(self.auth, "/tokens/refresh_token")?;
        if self
            .auth
            .get("OPENAI_API_KEY")
            .is_some_and(|v| !v.is_null())
        {
            return Err("API key logins cannot be saved as subscription profiles.".into());
        }
        let claims = self.claims()?;
        let user = model::string(&claims, "/https:~1~1api.openai.com~1auth/chatgpt_user_id")?;
        let workspace = model::string(
            &claims,
            "/https:~1~1api.openai.com~1auth/chatgpt_account_id",
        )?;
        if model::string(self.auth, "/tokens/account_id")? != workspace {
            return Err(
                "Native workspace and login claims disagree. Open the CLI to resolve the login."
                    .into(),
            );
        }
        Ok(Identity {
            provider: AgentId::Codex,
            user_id: user.to_owned(),
            workspace_id: workspace.to_owned(),
        })
    }

    /// The ID token's own `email` claim.
    fn email(&self) -> Option<String> {
        self.claims().ok().and_then(|claims| {
            claims
                .get("email")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|email| !email.is_empty())
                .map(str::to_owned)
        })
    }

    fn fingerprint(&self) -> String {
        model::fingerprint([
            None,
            None,
            self.auth.pointer("/tokens/access_token"),
            self.auth.pointer("/tokens/refresh_token"),
        ])
    }

    fn renewal_due(&self, now_ms: i64) -> bool {
        self.renews_soon(now_ms / 1000)
    }

    fn renew_private(&self, now_ms: i64) -> Result<Login, String> {
        usage_renew::renew_codex(self, now_ms, usage_renew::CODEX_TOKEN_URL)
    }
}

/// The payload of the JWT at `pointer`.
fn token_claims(auth: &Value, pointer: &str) -> Result<Value, String> {
    let token = model::string(auth, pointer)?;
    let encoded = token
        .split('.')
        .nth(1)
        .ok_or("Invalid native identity token.")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded.trim_end_matches('='))
        .map_err(|_| "Invalid native identity token.")?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid native identity token.".into())
}

/// The `https://api.openai.com/auth` claims of a saved Codex profile's ID token: identity and plan
/// metadata, never its tokens. `None` for an unknown key, a profile without a login, or a device
/// with no vault; an error when the vault exists and cannot be read right now.
pub(crate) fn saved_claims(home: &Path, key: &str) -> Result<Option<Value>, String> {
    if !Identity::is_profile_key(key) || !Store::vault_exists(home) {
        return Ok(None);
    }
    let db = Store::open_existing(home)?.load()?;
    Ok(db
        .observed(AgentId::Codex, key)
        .and_then(|profile| profile.login.as_ref())
        .and_then(|login| CodexLogin::of(login).auth_claims().ok().flatten()))
}

#[cfg(test)]
pub(crate) mod tests;
