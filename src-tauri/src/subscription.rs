//! Codex subscription dates, read from the login's own ID token. Its `https://api.openai.com/auth`
//! claims carry `chatgpt_subscription_active_until`, the day the paid period ends, and
//! `chatgpt_subscription_last_checked`, when OpenAI last confirmed it. Codex refreshes the token
//! as it runs and `accounts/usage_renew.rs` refreshes saved logins, so the date keeps up without a
//! browser session, a network call or a Keychain item of its own. The claim says how long the
//! plan is paid for, not whether it renews: that comes from the billing endpoint, read beside the
//! card's usage (`limits/renewal.rs`), and this date is the card's fallback when that read has
//! never answered. No access or refresh token is read here.
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDate {
    /// RFC 3339: the end of the paid period, still ahead whenever a reading is `Some`.
    pub date: String,
    /// RFC 3339: when OpenAI last confirmed the date, if the token says.
    pub checked_at: Option<String>,
}

/// The paid-through date these claims give `account_id`: `None` when they belong to another
/// account, carry no readable date, or the date has passed, because a lapsed period says nothing
/// about the current one. The callers already chose the claims by account; the check here is
/// the last line, so a vault profile whose token was issued to another identity, or a native
/// login whose claims disagree with its key, can never put its date on someone else's card.
fn parse_claims(claims: &Value, account_id: &str, now: DateTime<Utc>) -> Option<SubscriptionDate> {
    if crate::accounts::codex_store::observation_key(
        claims.get("chatgpt_account_id")?.as_str()?,
        claims,
    ) != account_id
    {
        return None;
    }
    let date = claims
        .get("chatgpt_subscription_active_until")?
        .as_str()?
        .parse::<DateTime<Utc>>()
        .ok()?;
    if date <= now {
        return None;
    }
    let checked_at = claims
        .get("chatgpt_subscription_last_checked")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        .filter(|checked| *checked <= now)
        .map(|checked| checked.to_rfc3339());
    Some(SubscriptionDate {
        date: date.to_rfc3339(),
        checked_at,
    })
}

/// The date for one Limits card. The signed-in login answers for its own account, since Codex
/// refreshes it most often; any other account is looked up among the saved profiles. An unreadable
/// native login is no date, because the card already shows that login's own failure; a vault that
/// cannot be read right now is an error the UI retries, since nothing else would surface it.
pub fn read(home: &Path, account: &str) -> Result<Option<SubscriptionDate>, String> {
    read_at(home, account, Utc::now())
}

fn read_at(
    home: &Path,
    account: &str,
    now: DateTime<Utc>,
) -> Result<Option<SubscriptionDate>, String> {
    let native = crate::accounts::codex_store::metadata(&home.join(".codex"))
        .ok()
        .flatten()
        .filter(|(key, _)| key == account)
        .map(|(_, claims)| claims);
    let claims = match native {
        Some(claims) => Some(claims),
        None => crate::accounts::saved_codex_claims(home, account)?,
    };
    Ok(claims.and_then(|claims| parse_claims(&claims, account, now)))
}

/// Versions up to 0.18.1 imported billing dates from the browser and kept them, with per-account
/// identity and attempt times, under `~/.on-n-off/subscriptions`. Nothing reads that directory now,
/// so startup removes it once; a home without it is left alone.
pub fn discard_legacy_cache(home: &Path) {
    let _ = std::fs::remove_dir_all(home.join(".on-n-off/subscriptions"));
}

#[cfg(test)]
mod tests;
