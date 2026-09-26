//! Stable native identity. Email and plan are display metadata, never profile keys.
use crate::dto::AgentId;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub provider: AgentId,
    pub user_id: String,
    pub workspace_id: String,
}
/// An OAuth access token on its way into one request header, and nothing more: never a refresh or
/// id token, never the login JSON it came from. It has no `Debug`, `Display`, `Clone` or
/// serialization, so it cannot reach a log, a DTO or a snapshot by accident; `authorization` is
/// the only way to read it back, already as the header value.
pub struct AccessToken(String);

impl AccessToken {
    pub fn new(token: &str) -> Self {
        Self(token.to_string())
    }

    /// The `Authorization` header value.
    pub fn authorization(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

pub fn string<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, String> {
    value.pointer(pointer).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "The native login is missing required identity or renewable credentials. Sign in again with the official CLI.".into())
}
pub fn claims(auth: &Value) -> Result<Value, String> {
    token_claims(auth, "/tokens/id_token")
}
/// Whether a running Codex client may renew this login within ten minutes of `now` (Unix
/// seconds). Codex renews shortly before expiry, five minutes in its source, and the desktop app
/// sooner. An unreadable expiry counts as soon.
pub fn codex_renews_soon(auth: &Value, now: i64) -> bool {
    token_claims(auth, "/tokens/access_token")
        .ok()
        .and_then(|claims| claims.get("exp")?.as_i64())
        .is_none_or(|expiry| expiry < now + 600)
}
fn token_claims(auth: &Value, pointer: &str) -> Result<Value, String> {
    let token = string(auth, pointer)?;
    let encoded = token
        .split('.')
        .nth(1)
        .ok_or("Invalid native identity token.")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded.trim_end_matches('='))
        .map_err(|_| "Invalid native identity token.")?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid native identity token.".into())
}
pub fn identity(provider: AgentId, auth: &Value, account: &Value) -> Result<Identity, String> {
    let (user, workspace) = match provider {
        AgentId::Codex => {
            string(auth, "/tokens/access_token")?;
            string(auth, "/tokens/refresh_token")?;
            if auth.get("OPENAI_API_KEY").is_some_and(|v| !v.is_null()) {
                return Err("API key logins cannot be saved as subscription profiles.".into());
            }
            let claims = claims(auth)?;
            let user = string(&claims, "/https:~1~1api.openai.com~1auth/chatgpt_user_id")?;
            let workspace = string(
                &claims,
                "/https:~1~1api.openai.com~1auth/chatgpt_account_id",
            )?;
            if string(auth, "/tokens/account_id")? != workspace {
                return Err("Native workspace and login claims disagree. Open the CLI to resolve the login.".into());
            }
            (user.to_owned(), workspace.to_owned())
        }
        AgentId::Claude => {
            string(auth, "/claudeAiOauth/accessToken")?;
            string(auth, "/claudeAiOauth/refreshToken")?;
            (
                string(account, "/accountUuid")?.to_owned(),
                string(account, "/organizationUuid")?.to_owned(),
            )
        }
        _ => return Err("Saved subscription profiles are not supported for this provider.".into()),
    };
    Ok(Identity {
        provider,
        user_id: user,
        workspace_id: workspace,
    })
}

impl Identity {
    pub fn observation_key(&self) -> String {
        let tuple = serde_json::to_vec(&(self.provider, &self.user_id, &self.workspace_id))
            .expect("serializable identity");
        format!("{PROFILE_KEY_PREFIX}{}", crate::sha::sha256_hex(&tuple))
    }
    /// Whether an observation key names a scoped profile, as opposed to a legacy workspace id.
    pub fn is_profile_key(key: &str) -> bool {
        key.starts_with(PROFILE_KEY_PREFIX)
    }
}
const PROFILE_KEY_PREFIX: &str = "profile:";
#[cfg(test)]
pub(crate) mod tests;
