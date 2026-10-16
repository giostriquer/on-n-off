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

pub(crate) struct MuseLogin {
    pub(crate) token: AccessToken,
    pub(crate) email: Option<String>,
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
    token: String,
    email: Option<String>,
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

    pub(crate) fn login(
        &self,
        config: &Path,
        now_secs: i64,
        keychain: &dyn Fn() -> Option<String>,
    ) -> Option<MuseLogin> {
        let text = std::fs::read_to_string(config.join("auth.json")).ok()?;
        let auth_digest = crate::sha::sha256_hex(text.as_bytes());
        if let Some(login) = self.cached(config, &auth_digest, now_secs) {
            return login;
        }
        let stored = stored_login(&text, now_secs, keychain);
        let login = stored.as_ref().and_then(|stored| usable(stored, now_secs));
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(Cached {
            config: config.to_path_buf(),
            auth_digest,
            stored,
        });
        login
    }

    fn cached(&self, config: &Path, auth_digest: &str, now_secs: i64) -> Option<Option<MuseLogin>> {
        let cache = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let cached = cache.as_ref()?;
        (cached.config == config && cached.auth_digest == auth_digest).then(|| {
            cached
                .stored
                .as_ref()
                .and_then(|stored| usable(stored, now_secs))
        })
    }
}

fn usable(stored: &Stored, now_secs: i64) -> Option<MuseLogin> {
    stored
        .expires_at
        .is_none_or(|expires_at| expires_at > now_secs)
        .then(|| MuseLogin {
            token: AccessToken::new(&stored.token),
            email: stored.email.clone(),
        })
}

fn stored_login(
    text: &str,
    now_secs: i64,
    keychain: &dyn Fn() -> Option<String>,
) -> Option<Stored> {
    let auth: CliAuth = serde_json::from_str(text).ok()?;
    let meta = auth.providers.get("meta")?;
    let expires_at = meta.expires_at.as_ref().and_then(Value::as_i64);
    if expires_at.is_some_and(|expires_at| expires_at <= now_secs) {
        return None;
    }
    let token = match non_empty(meta.access_token.as_deref()) {
        Some(token) => token,
        None if meta.mechanism == "oauth" => bundle_access_token(&keychain()?)?,
        None => return None,
    };
    Some(Stored {
        token,
        email: non_empty(meta.user_email.as_deref()),
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

pub(crate) fn signed_in_login(config: &Path) -> Option<MuseLogin> {
    let now_secs = chrono::Utc::now().timestamp();
    SIGNED_IN.login(config, now_secs, &keychain_secret)
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
