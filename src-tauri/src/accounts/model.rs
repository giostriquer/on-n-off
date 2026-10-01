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
pub struct AccessToken(String);

impl AccessToken {
    pub fn new(token: &str) -> Self {
        Self(token.to_string())
    }

    pub fn authorization(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

pub(crate) trait LoginView {
    fn identity(&self) -> Result<Identity, String>;
    fn email(&self) -> Option<String>;
    fn fingerprint(&self) -> String;
    fn renewal_due(&self, now_ms: i64) -> bool;
}

pub(super) fn claude_fingerprint(access: Option<&Value>, refresh: Option<&Value>) -> String {
    fingerprint([access, refresh, None, None])
}

pub(super) fn codex_fingerprint(access: Option<&Value>, refresh: Option<&Value>) -> String {
    fingerprint([None, None, access, refresh])
}

fn fingerprint(slots: [Option<&Value>; 4]) -> String {
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
    pub fn is_profile_key(key: &str) -> bool {
        key.starts_with(PROFILE_KEY_PREFIX)
    }
}
const PROFILE_KEY_PREFIX: &str = "profile:";
