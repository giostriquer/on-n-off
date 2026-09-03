//! Codex subscription limits: normalize `account/rateLimits/read` from Codex app-server.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;

use super::json::window;
use super::Parsed;
use crate::dto::{LimitWindowDto, LimitWindowKind, LimitsCreditsDto};

const WEEKLY_THRESHOLD_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct RateLimitsResponse {
    rate_limits: RateLimitBucket,
    #[serde(default)]
    rate_limits_by_limit_id: Option<BTreeMap<String, RateLimitBucket>>,
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

#[cfg(test)]
mod tests;
