use crate::timestamp::TimestampExt;
use jiff::Timestamp;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDate {
    pub date: String,
    pub checked_at: Option<String>,
}

fn parse_claims(claims: &Value, account_id: &str, now: Timestamp) -> Option<SubscriptionDate> {
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
        .parse::<Timestamp>()
        .ok()?;
    if date <= now {
        return None;
    }
    let checked_at = claims
        .get("chatgpt_subscription_last_checked")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<Timestamp>().ok())
        .filter(|checked| *checked <= now)
        .map(|checked| checked.utc_offset_text());
    Some(SubscriptionDate {
        date: date.utc_offset_text(),
        checked_at,
    })
}

pub fn read(home: &Path, account: &str) -> Result<Option<SubscriptionDate>, String> {
    read_at(home, account, Timestamp::now())
}

fn read_at(home: &Path, account: &str, now: Timestamp) -> Result<Option<SubscriptionDate>, String> {
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

pub fn discard_legacy_cache(home: &Path) {
    let _ = std::fs::remove_dir_all(home.join(".on-n-off/subscriptions"));
}

#[cfg(test)]
mod tests;
