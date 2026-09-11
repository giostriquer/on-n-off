//! Only normalized billing dates are persisted. Browser credentials stay in the short-lived
//! native reader and its supervised scratch storage. Codex credentials are never copied, refreshed, or written.
use super::{DateSource, SubscriptionDate};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
struct Auth {
    tokens: Option<Tokens>,
}
#[derive(Deserialize)]
struct Tokens {
    account_id: Option<String>,
    id_token: Option<String>,
}

pub(super) fn identity(home: &Path) -> Option<(String, Value)> {
    let file = File::open(home.join(".codex/auth.json")).ok()?;
    let auth: Auth = serde_json::from_reader(file.take(1024 * 1024)).ok()?;
    let tokens = auth.tokens?;
    let account = tokens.account_id?.trim().to_string();
    if account.is_empty() {
        return None;
    }
    let token = tokens.id_token?;
    let mut parts = token.split('.');
    parts.next()?;
    let body = parts.next()?;
    parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(body.trim_end_matches('=')).ok()?;
    let payload: Value = serde_json::from_slice(&decoded).ok()?;
    let claims = payload.get("https://api.openai.com/auth")?.clone();
    if claims.get("chatgpt_account_id")?.as_str()? != account {
        return None;
    }
    Some((account, claims))
}

#[derive(Serialize, Deserialize)]
struct Stored {
    version: u8,
    account_id: String,
    metadata: SubscriptionDate,
    #[serde(default)]
    last_attempt_at: Option<DateTime<Utc>>,
}
fn path(home: &Path, account: &str) -> PathBuf {
    let hash = crate::sha::sha256_hex(account.as_bytes());
    home.join(".on-n-off/subscriptions")
        .join(format!("codex-{hash}.json"))
}
pub(super) fn load(home: &Path, account: &str) -> Option<SubscriptionDate> {
    let file = File::open(path(home, account)).ok()?;
    let stored: Stored = serde_json::from_reader(file.take(16 * 1024)).ok()?;
    let metadata = stored.metadata;
    if stored.version != 1 || stored.account_id != account || metadata.source != DateSource::Billing
    {
        return None;
    }
    let checked = metadata
        .checked_at
        .as_deref()?
        .parse::<DateTime<Utc>>()
        .ok()?;
    // Re-run the parser so malformed local cache records never manufacture a billing status.
    let payload = match (&metadata.date, metadata.kind) {
        (None, None) => serde_json::json!({"active_until":null,"will_renew":false}),
        (Some(date), Some(super::DateKind::Renews)) => {
            serde_json::json!({"active_until":date,"will_renew":true})
        }
        (Some(date), Some(super::DateKind::Expires)) => {
            serde_json::json!({"active_until":date,"will_renew":false})
        }
        _ => return None,
    };
    super::parse_billing(&payload, checked)
}
pub(super) fn save(home: &Path, account: &str, metadata: &SubscriptionDate) -> Result<(), String> {
    let stored = Stored {
        version: 1,
        account_id: account.into(),
        metadata: metadata.clone(),
        last_attempt_at: None,
    };
    let json = serde_json::to_string(&stored).map_err(|_| "Could not encode subscription date.")?;
    crate::usage::cache_io::atomic_write(&path(home, account), &json)
        .map_err(|_| "Could not save subscription date.".to_string())
}
pub(super) fn forget(home: &Path, account: &str) -> Result<(), String> {
    match std::fs::remove_file(path(home, account)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Could not forget subscription date.".into()),
    }
}

// Called under the browser state lock before scheduling work. Persist before touching
// browsers so failures and application restarts cannot create a retry storm.
pub(super) fn claim_auto_refresh(home: &Path, account: &str, now: DateTime<Utc>) -> bool {
    let Some(metadata) = load(home, account) else {
        return false;
    };
    let Some(checked) = metadata
        .checked_at
        .as_deref()
        .and_then(|at| at.parse::<DateTime<Utc>>().ok())
    else {
        return false;
    };
    let Ok(file) = File::open(path(home, account)) else {
        return false;
    };
    let Ok(mut stored) = serde_json::from_reader::<_, Stored>(file.take(16 * 1024)) else {
        return false;
    };
    let latest = stored
        .last_attempt_at
        .map_or(checked, |attempt| attempt.max(checked));
    if (now - latest).num_seconds() < super::REFRESH_SECONDS {
        return false;
    }
    stored.last_attempt_at = Some(now);
    let Ok(json) = serde_json::to_string(&stored) else {
        return false;
    };
    crate::usage::cache_io::atomic_write(&path(home, account), &json).is_ok()
}
