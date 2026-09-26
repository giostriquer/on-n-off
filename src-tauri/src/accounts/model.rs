//! Stable native identity, and what account code asks of a login without knowing its shape
//! ([`LoginView`]). Email and plan are display metadata, never profile keys.
use super::store::Login;
use crate::dto::AgentId;
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

/// A saved or native login read by its provider's rules: `claude::ClaudeLogin` or
/// `codex::CodexLogin`, borrowed from a `Login` whose stored shape stays the provider's own. The
/// provider's adapter chooses it (`super::view`), so nothing else reads a login's JSON.
pub(crate) trait LoginView {
    /// The user and workspace this login signs in as. Refused for a login that cannot be a saved
    /// profile: no renewable tokens, an API key, or identity records that disagree.
    fn identity(&self) -> Result<Identity, String>;
    /// The account's email, trimmed, for display.
    fn email(&self) -> Option<String>;
    /// Which credential generation this is: its access and refresh tokens, and nothing that only
    /// presents them, so a new email or plan is not a new generation.
    fn fingerprint(&self) -> String;
    /// Whether a saved login is due to renew before it is read, at `now_ms`.
    fn renewal_due(&self, now_ms: i64) -> bool;
    /// Renews a never-activated private login with its provider's grant: the reply folded into
    /// the login, every field it does not name left as it was.
    fn renew_private(&self, now_ms: i64) -> Result<Login, String>;
}

/// A generation's fingerprint over the four slots every version has hashed: Claude's access and
/// refresh tokens, then Codex's. Each provider fills its own two and leaves the other's empty, so
/// the fingerprints a vault's signed-out generations and a renewal journal hold keep matching.
pub(super) fn fingerprint(slots: [Option<&Value>; 4]) -> String {
    crate::sha::sha256_hex(&serde_json::to_vec(&slots).expect("serializable credential generation"))
}

pub fn string<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, String> {
    value.pointer(pointer).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "The native login is missing required identity or renewable credentials. Sign in again with the official CLI.".into())
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
