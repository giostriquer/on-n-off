//! Codex subscription limits: normalize `account/rateLimits/read` from Codex app-server.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;

use super::json::window;
use super::Parsed;
use crate::dto::{
    LimitWindowDto, LimitWindowKind, LimitsCreditsDto, LimitsPriceDto, LimitsResetCreditsDto,
    LimitsResetOfferDto,
};

const WEEKLY_THRESHOLD_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct RateLimitsResponse {
    rate_limits: RateLimitBucket,
    #[serde(default)]
    rate_limits_by_limit_id: Option<BTreeMap<String, RateLimitBucket>>,
    /// Absent from CLIs older than banked resets; `null` when the backend does not report them.
    #[serde(default)]
    rate_limit_reset_credits: Option<RateLimitResetCredits>,
    /// A banner the backend owns entirely, including whether it is there at all. Kept opaque:
    /// only the one call to action that names a paid reset is read out of it.
    #[serde(default)]
    rate_limit_upsell: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RateLimitBucket {
    #[serde(default)]
    limit_id: Option<String>,
    #[serde(default)]
    limit_name: Option<String>,
    #[serde(default)]
    primary: Option<RateLimitWindow>,
    #[serde(default)]
    secondary: Option<RateLimitWindow>,
    #[serde(default)]
    credits: Option<RateLimitCredits>,
    #[serde(default)]
    plan_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RateLimitWindow {
    used_percent: f64,
    #[serde(default)]
    window_duration_mins: Option<u64>,
    #[serde(default)]
    resets_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RateLimitResetCredits {
    available_count: u64,
    /// Detail rows; `null` when only the count was read, and possibly capped below the count.
    #[serde(default)]
    credits: Option<Vec<RateLimitResetCredit>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RateLimitResetCredit {
    #[serde(default)]
    status: String,
    #[serde(default)]
    expires_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RateLimitCredits {
    #[serde(default)]
    has_credits: bool,
    #[serde(default)]
    unlimited: bool,
    #[serde(default)]
    balance: Option<String>,
}

pub(super) fn parse_codex(payload: &RateLimitsResponse) -> Parsed {
    let fallback = &payload.rate_limits;
    let main_id = fallback
        .limit_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "codex".to_string());
    let buckets = payload.rate_limits_by_limit_id.as_ref();
    let main = buckets
        .and_then(|items| items.get(&main_id))
        .unwrap_or(fallback);
    let mut windows = Vec::new();
    windows.extend(bucket_windows(&main_id, main, true));
    if let Some(buckets) = buckets {
        for (id, bucket) in buckets {
            if id != &main_id {
                windows.extend(bucket_windows(id, bucket, false));
            }
        }
    }
    windows.sort_by_key(|window| super::pipeline::kind_rank(window.kind));
    Parsed {
        account: None,
        plan: main.plan_type.clone(),
        windows,
        credits: credits(main.credits.as_ref()),
        reset_credits: reset_credits(payload.rate_limit_reset_credits.as_ref()),
        reset_offer: reset_offer(payload.rate_limit_upsell.as_ref()),
    }
}

fn bucket_windows(id: &str, bucket: &RateLimitBucket, main: bool) -> Vec<LimitWindowDto> {
    [
        ("primary", bucket.primary.as_ref()),
        ("secondary", bucket.secondary.as_ref()),
    ]
    .into_iter()
    .filter_map(|(slot, entry)| rate_limit_window(id, bucket, slot, entry?, main))
    .collect()
}

fn rate_limit_window(
    id: &str,
    bucket: &RateLimitBucket,
    slot: &str,
    entry: &RateLimitWindow,
    main: bool,
) -> Option<LimitWindowDto> {
    let used = entry.used_percent.clamp(0.0, 100.0);
    if !used.is_finite() {
        return None;
    }
    let minutes = entry.window_duration_mins;
    let seconds = minutes.and_then(|minutes| minutes.checked_mul(60));
    let duration_kind = match seconds {
        Some(seconds) if seconds < WEEKLY_THRESHOLD_SECONDS => LimitWindowKind::Session,
        _ => LimitWindowKind::Weekly,
    };
    let kind = if main {
        duration_kind
    } else {
        LimitWindowKind::Model
    };
    let prefix = match duration_kind {
        LimitWindowKind::Session => session_length(minutes),
        LimitWindowKind::Weekly | LimitWindowKind::Model => "Weekly".to_string(),
    };
    let target = if main {
        "all models".to_string()
    } else {
        bucket
            .limit_name
            .clone()
            .unwrap_or_else(|| "extra limit".to_string())
    };
    let resets_at = entry
        .resets_at
        .and_then(|epoch| DateTime::<Utc>::from_timestamp(epoch, 0))
        .map(|at| at.to_rfc3339());
    let window_id = if main {
        slot.to_string()
    } else if slot == "primary" {
        format!("extra:{id}")
    } else {
        format!("extra:{id}:{slot}")
    };
    Some(LimitWindowDto {
        window_seconds: seconds,
        ..window(
            window_id,
            format!("{prefix} · {target}"),
            kind,
            used,
            resets_at,
        )
    })
}

fn session_length(minutes: Option<u64>) -> String {
    match minutes {
        Some(minutes) if minutes >= 60 && minutes.is_multiple_of(60) => {
            format!("{} hour", minutes / 60)
        }
        Some(minutes) => format!("{minutes} minute"),
        None => "Session".to_string(),
    }
}

fn credits(value: Option<&RateLimitCredits>) -> Option<LimitsCreditsDto> {
    let credits = value?;
    if !credits.has_credits {
        return None;
    }
    Some(LimitsCreditsDto {
        balance: credits.balance.clone().unwrap_or_else(|| "0".to_string()),
        unlimited: credits.unlimited,
    })
}

/// Codex's own name for the paid reset in a banner's calls to action. Anything else the backend
/// offers there — more credits, a bigger plan — is not this, and is left alone.
const BUY_RESET: &str = "buy_reset";

/// Well past any reset ever sold, and far below the point where a count stops surviving the trip
/// through JavaScript intact. A number beyond it is a backend the app does not understand.
const MAX_MINOR_UNITS: u64 = 1_000_000;

/// The banner is forwarded to clients as the backend wrote it — `rate_limit_upsell` is an untyped
/// value on the app-server response, so its keys stay snake_case — and only this one call to
/// action is read out of it.
fn reset_offer(banner: Option<&serde_json::Value>) -> Option<LimitsResetOfferDto> {
    let cta = banner?
        .get("ctas")?
        .as_array()?
        .iter()
        .find(|cta| cta.get("action").and_then(serde_json::Value::as_str) == Some(BUY_RESET))?;
    Some(LimitsResetOfferDto {
        price: price(cta.get("price")),
    })
}

fn price(price: Option<&serde_json::Value>) -> Option<LimitsPriceDto> {
    let price = price?;
    let currency = price.get("currency").and_then(serde_json::Value::as_str)?;
    let currency = currency.trim();
    // ISO 4217 is three letters. Anything else is not a currency this app will put beside a number.
    if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let amount = price
        .get("amount_minor_units")
        .and_then(serde_json::Value::as_u64)
        .filter(|amount| *amount <= MAX_MINOR_UNITS)?;
    Some(LimitsPriceDto {
        amount_minor_units: amount,
        currency: currency.to_uppercase(),
    })
}

/// The count Codex reports, and when the soonest still-available reset expires.
fn reset_credits(value: Option<&RateLimitResetCredits>) -> Option<LimitsResetCreditsDto> {
    let summary = value?;
    let next_expires_at = summary
        .credits
        .iter()
        .flatten()
        .filter(|credit| credit.status == "available")
        .filter_map(|credit| DateTime::<Utc>::from_timestamp(credit.expires_at?, 0))
        .min()
        .map(|at| at.to_rfc3339());
    Some(LimitsResetCreditsDto {
        available_count: u32::try_from(summary.available_count).unwrap_or(u32::MAX),
        next_expires_at,
    })
}

#[cfg(test)]
mod tests;
