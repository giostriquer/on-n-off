use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::Deserialize;
use serde_json::Value;

use super::model::AccessToken;

#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "ai.meta.dev.credentials";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "meta";

#[derive(Default)]
pub(crate) struct MuseAuth {
    pub(crate) email: Option<String>,
    pub(crate) token: Option<AccessToken>,
}

#[derive(Default, Deserialize)]
struct CliAuth {
    #[serde(default)]
    providers: HashMap<String, CliProvider>,
}

#[derive(Default, Deserialize)]
struct CliProvider {
    #[serde(default)]
    mechanism: String,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    user_email: Option<String>,
    #[serde(default)]
    expires_at: Option<Value>,
}

#[derive(Deserialize)]
struct KeychainBundle {
    #[serde(default)]
    access_token: Option<String>,
}

struct Stored {
    email: Option<String>,
    token: Option<String>,
    expires_at: Option<i64>,
}

struct Cached {
    config: PathBuf,
    auth_digest: String,
    stored: Option<Stored>,
}

pub(crate) struct LoginCache(Mutex<Option<Cached>>);

impl LoginCache {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub(crate) fn auth(
        &self,
        config: &Path,
        now_secs: i64,
        keychain: &dyn Fn() -> Option<String>,
    ) -> MuseAuth {
        let Ok(text) = std::fs::read_to_string(config.join("auth.json")) else {
            return MuseAuth::default();
        };
        let auth_digest = crate::sha::sha256_hex(text.as_bytes());
        if let Some(auth) = self.cached(config, &auth_digest, now_secs) {
            return auth;
        }
        let stored = stored_auth(&text, now_secs, keychain);
        let auth = current(stored.as_ref(), now_secs);
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(Cached {
            config: config.to_path_buf(),
            auth_digest,
            stored,
        });
        auth
    }

    fn cached(&self, config: &Path, auth_digest: &str, now_secs: i64) -> Option<MuseAuth> {
        let cache = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let cached = cache.as_ref()?;
        (cached.config == config && cached.auth_digest == auth_digest)
            .then(|| current(cached.stored.as_ref(), now_secs))
    }
}

fn current(stored: Option<&Stored>, now_secs: i64) -> MuseAuth {
    let Some(stored) = stored else {
        return MuseAuth::default();
    };
    let live = stored
        .expires_at
        .is_none_or(|expires_at| expires_at > now_secs);
    MuseAuth {
        email: stored.email.clone(),
        token: stored
            .token
            .as_deref()
            .filter(|_| live)
            .map(AccessToken::new),
    }
}

fn stored_auth(text: &str, now_secs: i64, keychain: &dyn Fn() -> Option<String>) -> Option<Stored> {
    let auth: CliAuth = serde_json::from_str(text).ok()?;
    let meta = auth.providers.get("meta")?;
    let expires_at = meta.expires_at.as_ref().and_then(Value::as_i64);
    let expired = expires_at.is_some_and(|expires_at| expires_at <= now_secs);
    let token = match non_empty(meta.access_token.as_deref()) {
        _ if expired => None,
        Some(token) => Some(token),
        None if meta.mechanism == "oauth" => keychain().as_deref().and_then(bundle_access_token),
        None => None,
    };
    Some(Stored {
        email: non_empty(meta.user_email.as_deref()),
        token,
        expires_at,
    })
}

fn bundle_access_token(value: &str) -> Option<String> {
    let bundle: KeychainBundle = serde_json::from_str(value.trim()).ok()?;
    non_empty(bundle.access_token.as_deref())
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

static SIGNED_IN: LoginCache = LoginCache::new();

pub(crate) fn signed_in(config: &Path) -> MuseAuth {
    let now_secs = chrono::Utc::now().timestamp();
    SIGNED_IN.auth(config, now_secs, &keychain_secret)
}

#[cfg(target_os = "macos")]
fn keychain_secret() -> Option<String> {
    super::keychain::find_password(KEYCHAIN_SERVICE, Some(KEYCHAIN_ACCOUNT))
        .ok()
        .flatten()
}

#[cfg(not(target_os = "macos"))]
fn keychain_secret() -> Option<String> {
    None
}

#[cfg(test)]
mod tests;
