//! Only normalized billing dates are persisted. Browser credentials stay in the short-lived
//! native reader and its supervised scratch storage. Codex credentials are never copied, refreshed, or written.
use super::{DateSource, SubscriptionDate};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

pub(super) fn identity(home: &Path) -> Option<(String, Value)> {
    crate::accounts::native::codex_metadata(&home.join(".codex"))
        .ok()
        .flatten()
}

#[derive(Serialize, Deserialize)]
struct Stored {
    version: u8,
    account_id: String,
    metadata: Option<SubscriptionDate>,
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
    let metadata = stored.metadata?;
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
        metadata: Some(metadata.clone()),
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

// The reservation is persisted before browser access, including the first failed attempt.
// This file contains identity and billing metadata only; never a browser session.
pub(super) fn claim_auto_refresh(home: &Path, account: &str, now: DateTime<Utc>) -> bool {
    record_attempt(home, account, now, false)
}
pub(super) fn record_attempt(home: &Path, account: &str, now: DateTime<Utc>, force: bool) -> bool {
    let path = path(home, account);
    let mut stored = match File::open(&path) {
        Ok(file) => match serde_json::from_reader::<_, Stored>(file.take(16 * 1024)) {
            Ok(value) if value.version == 1 && value.account_id == account => value,
            _ => return false,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Stored {
            version: 1,
            account_id: account.into(),
            metadata: None,
            last_attempt_at: None,
        },
        Err(_) => return false,
    };
    let checked = stored
        .metadata
        .as_ref()
        .and_then(|m| m.checked_at.as_deref())
        .and_then(|at| at.parse::<DateTime<Utc>>().ok());
    let latest = checked.into_iter().chain(stored.last_attempt_at).max();
    if !force && latest.is_some_and(|at| (now - at).num_seconds() < super::REFRESH_SECONDS) {
        return false;
    }
    stored.last_attempt_at = Some(now);
    let Ok(json) = serde_json::to_string(&stored) else {
        return false;
    };
    crate::usage::cache_io::atomic_write(&path, &json).is_ok()
}
