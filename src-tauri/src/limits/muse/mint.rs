use chrono::{DateTime, SecondsFormat};
use serde::Deserialize;
use serde_json::json;

use super::account_dto;
use crate::accounts::model::{AccessToken, Identity};
use crate::dto::{LimitWindowDto, LimitWindowKind, ProviderLimitsDto, Reading};
use crate::http::HttpError;
use crate::limits::outcome::{saved_card, verified, ReadFailure, Unsupported};
use crate::limits::{Parsed, SavedReadUrls};

pub(crate) const KEY_ENDPOINT: &str = "https://api.meta.ai/muse-code/key";

const DEFAULT_ROLLING_MINS: u64 = 300;

static IDLE_USAGE: SubsUsage = SubsUsage {
    tier: String::new(),
    window: Some(MintWindow {
        used_percent: Some(0.0),
        window_duration_mins: None,
        resets_at: None,
    }),
    weekly: None,
};

#[derive(Debug, Deserialize)]
pub(super) struct MintResponse {
    #[serde(default)]
    is_subs_active: Option<bool>,
    #[serde(default)]
    require_payment: bool,
    #[serde(default)]
    subs_tier_name: String,
    #[serde(default)]
    subs_usage: Option<SubsUsage>,
    #[serde(default)]
    user_email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubsUsage {
    #[serde(default)]
    tier: String,
    #[serde(default)]
    window: Option<MintWindow>,
    #[serde(default)]
    weekly: Option<MintWindow>,
}

#[derive(Debug, Deserialize)]
struct MintWindow {
    #[serde(default)]
    used_percent: Option<f64>,
    #[serde(default)]
    window_duration_mins: Option<f64>,
    #[serde(default)]
    resets_at: Option<f64>,
}

pub(super) fn fetch(url: &str, token: &AccessToken) -> Result<MintResponse, HttpError> {
    let authorization = token.authorization();
    let value = crate::http::post_json_headers(
        url,
        &[
            ("Authorization", authorization.as_str()),
            ("X-Api-Version", "1.0.0"),
        ],
        &json!({}),
    )?;
    serde_json::from_value(value)
        .map_err(|_| HttpError::Parse("Unreadable Muse key answer.".into()))
}

pub(crate) fn read_saved(
    identity: &Identity,
    token: &AccessToken,
    urls: &SavedReadUrls<'_>,
) -> Result<ProviderLimitsDto, ReadFailure> {
    saved_answer(identity, token, urls).and_then(|parsed| saved_card(identity, parsed))
}

pub(crate) fn verify(
    identity: &Identity,
    token: &AccessToken,
    urls: &SavedReadUrls<'_>,
) -> Result<(), String> {
    verified(
        identity.provider,
        &saved_answer(identity, token, urls).map(drop),
    )
}

fn saved_answer(
    identity: &Identity,
    token: &AccessToken,
    urls: &SavedReadUrls<'_>,
) -> Result<Parsed, ReadFailure> {
    let response = fetch(urls.muse, token)?;
    let email = response
        .user_email
        .as_deref()
        .map(str::trim)
        .filter(|email| !email.is_empty());
    if email.is_some_and(|email| email.to_lowercase() != identity.user_id) {
        return Err(ReadFailure::OtherAccount);
    }
    answer(email.map(str::to_string), &response)
}

pub(super) fn answer(
    email: Option<String>,
    response: &MintResponse,
) -> Result<Parsed, ReadFailure> {
    if response.require_payment {
        return Err(ReadFailure::Unsupported(Unsupported::BillingIncomplete));
    }
    if response.is_subs_active == Some(false) {
        return Err(ReadFailure::Unsupported(Unsupported::Inactive));
    }
    let usage = match &response.subs_usage {
        Some(usage) => Some(usage),
        None if response.is_subs_active == Some(true) => Some(&IDLE_USAGE),
        None => None,
    };
    let windows: Vec<LimitWindowDto> = usage
        .into_iter()
        .flat_map(|usage| {
            [
                usage.weekly.as_ref().and_then(weekly_window),
                usage.window.as_ref().and_then(session_window),
            ]
        })
        .flatten()
        .collect();
    let tier = usage.map_or("", |usage| usage.tier.trim());
    let plan = Some(response.subs_tier_name.trim())
        .filter(|name| !name.is_empty())
        .or(Some(tier))
        .filter(|name| name.chars().any(char::is_alphabetic))
        .map(str::to_string);
    Ok(Parsed {
        account: Some(account_dto(email)),
        reading: Reading {
            plan,
            windows,
            ..Reading::default()
        },
    })
}

fn weekly_window(value: &MintWindow) -> Option<LimitWindowDto> {
    let used = used_percent(value)?;
    Some(window(
        "weekly",
        "Weekly · all models".to_string(),
        LimitWindowKind::Weekly,
        used,
        resets_at_ms(value),
    ))
}

fn session_window(value: &MintWindow) -> Option<LimitWindowDto> {
    let used = used_percent(value)?;
    let minutes = value
        .window_duration_mins
        .filter(|minutes| minutes.is_finite() && *minutes > 0.0)
        .map_or(DEFAULT_ROLLING_MINS, |minutes| minutes as u64);
    let mut dto = window(
        "session",
        session_label(minutes),
        LimitWindowKind::Session,
        used,
        resets_at_ms(value),
    );
    dto.window_seconds = Some(minutes.saturating_mul(60));
    Some(dto)
}

fn session_label(minutes: u64) -> String {
    if minutes > 0 && minutes.is_multiple_of(60) {
        format!("{} hour · all models", minutes / 60)
    } else {
        format!("{minutes} min · all models")
    }
}

fn window(
    id: &str,
    label: String,
    kind: LimitWindowKind,
    used_percent: f64,
    resets_at_ms: i64,
) -> LimitWindowDto {
    LimitWindowDto {
        id: id.to_string(),
        label,
        kind,
        used_percent: used_percent.clamp(0.0, 100.0),
        resets_at: (resets_at_ms > 0).then(|| rfc3339_ms(resets_at_ms)),
        window_seconds: None,
        observed_at: String::new(),
    }
}

fn rfc3339_ms(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .map(|at| at.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn used_percent(value: &MintWindow) -> Option<f64> {
    value.used_percent.filter(|used| used.is_finite())
}

fn resets_at_ms(value: &MintWindow) -> i64 {
    value
        .resets_at
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .map_or(0, |seconds| (seconds * 1000.0) as i64)
}

#[cfg(test)]
pub(super) mod fixtures;
#[cfg(test)]
mod tests;
