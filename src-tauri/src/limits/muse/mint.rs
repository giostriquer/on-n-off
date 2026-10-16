// Muse subscription limits from the CLI's own key-mint call
// (`POST https://api.meta.ai/muse-code/key`, the request the CLI makes at
// startup and for /usage): beside the minted inference key it reports the
// subscription tier and both usage windows, with no model turn spent. The
// minted key and every other field are discarded unparsed; only the quota
// block below is ever kept. The device token is read, never written: inline
// in the CLI's auth.json where the CLI stores it there, else from the CLI's
// own Keychain item on macOS.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use super::{account_dto, session_label, window, ServeFailure};
use crate::dto::{LimitWindowKind, Reading};
use crate::http::HttpError;
use crate::limits::Parsed;

pub(super) const KEY_ENDPOINT: &str = "https://api.meta.ai/muse-code/key";
#[cfg(target_os = "macos")]
pub(super) const KEYCHAIN_SERVICE: &str = "ai.meta.dev.credentials";
#[cfg(target_os = "macos")]
pub(super) const KEYCHAIN_ACCOUNT: &str = "meta";

const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const DEFAULT_ROLLING_MINS: u64 = 300;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct DeviceLogin {
    pub(super) token: String,
    pub(super) email: Option<String>,
}

impl std::fmt::Debug for DeviceLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceLogin")
            .field("token", &"<redacted>")
            .field("email", &self.email)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MintError {
    Unauthorized,
    Unavailable,
}

#[derive(Debug, Default, Deserialize)]
struct CliAuth {
    #[serde(default)]
    providers: HashMap<String, CliProvider>,
}

#[derive(Debug, Default, Deserialize)]
struct CliProvider {
    #[serde(default)]
    mechanism: String,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    user_email: Option<String>,
}

/// The stored device login under `config`, when one can be read without
/// starting the CLI: the inline token in auth.json, else — for an oauth
/// login kept elsewhere, the Keychain item on macOS — the given lookup.
/// Credentials the mint endpoint cannot use (an API-key login) yield no
/// login; the serve read then reports that state instead.
pub(super) fn load_login(
    config: &Path,
    keychain: &dyn Fn() -> Option<String>,
) -> Option<DeviceLogin> {
    let text = std::fs::read_to_string(config.join("auth.json")).ok()?;
    let auth: CliAuth = serde_json::from_str(&text).ok()?;
    let meta = auth.providers.get("meta")?;
    let email = meta
        .user_email
        .as_deref()
        .map(str::trim)
        .filter(|email| !email.is_empty())
        .map(str::to_string);
    if let Some(token) = meta
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        return Some(DeviceLogin {
            token: token.to_string(),
            email,
        });
    }
    if meta.mechanism != "oauth" {
        return None;
    }
    let token = keychain_token(&keychain()?)?;
    Some(DeviceLogin { token, email })
}

/// The Keychain value is a JSON bundle carrying the OAuth credential beside
/// the Model API key the CLI last minted. Only `access_token` is declared
/// here, so every other secret in the bundle is dropped while parsing and
/// never retained. A bare token from older storage shapes still reads.
fn keychain_token(value: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Bundle {
        #[serde(default)]
        access_token: Option<String>,
    }

    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.starts_with('{') {
        let bundle: Bundle = serde_json::from_str(value).ok()?;
        return bundle
            .access_token
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty());
    }
    Some(value.to_string())
}

struct CachedLogin {
    config: PathBuf,
    auth_mtime: Option<std::time::SystemTime>,
    login: Option<DeviceLogin>,
}

static LOGIN_CACHE: Mutex<Option<CachedLogin>> = Mutex::new(None);

/// `load_login` for production polls: the Keychain lookup can prompt, so
/// its result — including a denial — is reused while `auth.json` stands
/// unchanged. A re-login rewrites that file, which is what re-reads the
/// login (and may prompt once more); a rejected token alone never does.
pub(super) fn load_login_cached(
    config: &Path,
    keychain: &dyn Fn() -> Option<String>,
) -> Option<DeviceLogin> {
    let auth_mtime = std::fs::metadata(config.join("auth.json"))
        .and_then(|metadata| metadata.modified())
        .ok();
    if let Ok(cache) = LOGIN_CACHE.lock() {
        if let Some(cached) = cache.as_ref() {
            if cached.config == *config && cached.auth_mtime == auth_mtime {
                return cached.login.clone();
            }
        }
    }
    let login = load_login(config, keychain);
    if let Ok(mut cache) = LOGIN_CACHE.lock() {
        *cache = Some(CachedLogin {
            config: config.to_path_buf(),
            auth_mtime,
            login: login.clone(),
        });
    }
    login
}

/// Only the quota fields are declared, so the minted key and the account
/// identity in the response are never retained.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct MintResponse {
    #[serde(default)]
    pub(super) is_subs_active: Option<bool>,
    #[serde(default)]
    require_payment: bool,
    #[serde(default)]
    pub(super) subs_tier_name: String,
    #[serde(default)]
    subs_usage: Option<SubsUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct SubsUsage {
    #[serde(default)]
    tier: String,
    #[serde(default)]
    window: Option<Value>,
    #[serde(default)]
    weekly: Option<Value>,
}

struct CachedResponse {
    token: String,
    fetched_at: Instant,
    response: MintResponse,
}

static RESPONSE_CACHE: Mutex<Option<CachedResponse>> = Mutex::new(None);

#[cfg(test)]
pub(super) fn reset_cache() {
    if let Ok(mut cache) = RESPONSE_CACHE.lock() {
        *cache = None;
    }
    if let Ok(mut cache) = LOGIN_CACHE.lock() {
        *cache = None;
    }
}

/// One mint call, reused for `CACHE_TTL` per token: the endpoint is
/// rate-limited and quota needs no fresher read.
pub(super) fn fetch(url: &str, token: &str) -> Result<MintResponse, MintError> {
    let digest = crate::sha::sha256_hex(token.as_bytes());
    if let Ok(cache) = RESPONSE_CACHE.lock() {
        if let Some(cached) = cache.as_ref() {
            if cached.token == digest && cached.fetched_at.elapsed() < CACHE_TTL {
                return Ok(cached.response.clone());
            }
        }
    }
    let value =
        crate::http::post_json_headers(url, token, &[("X-Api-Version", "1.0.0")], &json!({}))
            .map_err(|error| match error {
                HttpError::Unauthorized | HttpError::Status(403) => MintError::Unauthorized,
                _ => MintError::Unavailable,
            })?;
    let response: MintResponse =
        serde_json::from_value(value).map_err(|_| MintError::Unavailable)?;
    if let Ok(mut cache) = RESPONSE_CACHE.lock() {
        *cache = Some(CachedResponse {
            token: digest,
            fetched_at: Instant::now(),
            response: response.clone(),
        });
    }
    Ok(response)
}

/// `None` means the mint answer carries no window to show (an idle
/// subscription) and the serve read should answer instead, so a remembered
/// reading is never replaced by an empty one.
pub(super) fn normalize(
    login: &DeviceLogin,
    response: &MintResponse,
) -> Option<Result<Parsed, ServeFailure>> {
    if response.require_payment {
        return Some(Err(ServeFailure::Unsupported(
            "Muse subscription billing is incomplete; finish it in Muse to see subscription limits."
                .to_string(),
        )));
    }
    if response.is_subs_active == Some(false) {
        return Some(Err(ServeFailure::Unsupported(
            "This Muse subscription is inactive, so it has no subscription limits to show."
                .to_string(),
        )));
    }
    let usage = response.subs_usage.as_ref()?;
    let mut windows = Vec::new();
    if let Some(weekly) = mint_window(usage.weekly.as_ref(), LimitWindowKind::Weekly, None) {
        windows.push(weekly);
    }
    if let Some(session) = mint_window(
        usage.window.as_ref(),
        LimitWindowKind::Session,
        Some(DEFAULT_ROLLING_MINS),
    ) {
        windows.push(session);
    }
    if windows.is_empty() {
        return None;
    }
    let plan = Some(response.subs_tier_name.trim().to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| Some(usage.tier.trim().to_string()))
        .filter(|tier| tier.chars().any(|c| c.is_alphabetic()));
    Some(Ok(Parsed {
        account: Some(account_dto(login.email.clone())),
        reading: Reading {
            plan,
            windows,
            ..Reading::default()
        },
    }))
}

fn mint_window(
    value: Option<&Value>,
    kind: LimitWindowKind,
    default_mins: Option<u64>,
) -> Option<crate::dto::LimitWindowDto> {
    let value = value?;
    let used = number(value.get("used_percent"))?;
    let resets_at_ms = resets_at_ms(value.get("resets_at"));
    let (id, label, minutes) = match kind {
        LimitWindowKind::Weekly => ("weekly", "Weekly · all models".to_string(), None),
        _ => {
            let mins = number(value.get("window_duration_mins"))
                .filter(|mins| *mins > 0.0)
                .map_or(default_mins, |mins| Some(mins as u64));
            ("session", session_label(mins.unwrap_or(0)), mins)
        }
    };
    // An empty observed_at is stamped with the read time by the pipeline.
    let mut dto = window(id, label, kind, used, resets_at_ms, "");
    if let Some(mins) = minutes {
        dto.window_seconds = Some(mins.saturating_mul(60));
    }
    Some(dto)
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64).filter(|n| n.is_finite())
}

/// Reset stamps arrive as unix seconds from the mint call; a value already
/// in milliseconds is kept as either unit is unambiguous at this scale.
fn resets_at_ms(value: Option<&Value>) -> i64 {
    let Some(stamp) = number(value) else {
        return 0;
    };
    if stamp >= 100_000_000_000.0 {
        stamp as i64
    } else if stamp > 0.0 {
        (stamp * 1000.0) as i64
    } else {
        0
    }
}
