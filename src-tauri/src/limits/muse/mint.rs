use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use super::{account_dto, ReadFailure};
use crate::accounts::model::{AccessToken, Identity};
use crate::dto::{ProviderLimitsDto, Reading};
use crate::http::HttpError;
use crate::limits::window::{QuotaWindow, Scope, FIVE_HOURS, ONE_WEEK};
use crate::limits::{saved_card, Parsed, SavedReadError, SavedReadUrls};

pub(crate) const KEY_ENDPOINT: &str = "https://api.meta.ai/muse-code/key";

const SESSION: &str = "session";
const WEEKLY: &str = "weekly";

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
) -> Result<ProviderLimitsDto, SavedReadError> {
    let response = fetch(urls.muse, token)?;
    let email = response
        .user_email
        .as_deref()
        .map(str::trim)
        .filter(|email| !email.is_empty());
    if email.is_some_and(|email| email.to_lowercase() != identity.user_id) {
        return Err(SavedReadError::OtherAccount);
    }
    match normalize(email.map(str::to_string), &response) {
        Some(Ok(parsed)) => saved_card(identity, parsed),
        Some(Err(_)) => Err(SavedReadError::Unavailable(
            "This Muse subscription is inactive or awaiting payment.",
        )),
        None => Err(SavedReadError::Unavailable(
            "Muse shows no recent usage for this account.",
        )),
    }
}

pub(super) fn current_read(
    email: Option<String>,
    answer: Result<MintResponse, HttpError>,
) -> Result<Parsed, ReadFailure> {
    let why = match answer {
        Ok(response) => match normalize(email.clone(), &response) {
            Some(read) => return read,
            None => "Meta's answer had no subscription usage for this Muse account.".to_string(),
        },
        Err(HttpError::Unauthorized) => {
            "Meta did not accept Muse's login. Sign in with `muse` again to see subscription limits."
                .to_string()
        }
        Err(HttpError::RateLimited(_)) => {
            "Meta is limiting how often Muse usage can be read. on-n-off will read it again later."
                .to_string()
        }
        Err(error) => {
            format!("The Muse usage read failed ({error}). on-n-off will read it again later.")
        }
    };
    Err(ReadFailure::Failed(account_dto(email), why))
}

fn normalize(
    email: Option<String>,
    response: &MintResponse,
) -> Option<Result<Parsed, ReadFailure>> {
    if response.require_payment {
        return Some(Err(ReadFailure::Unsupported(
            "Muse subscription billing is incomplete; finish it in Muse to see subscription limits."
                .to_string(),
        )));
    }
    if response.is_subs_active == Some(false) {
        return Some(Err(ReadFailure::Unsupported(
            "This Muse subscription is inactive, so it has no subscription limits to show."
                .to_string(),
        )));
    }
    let windows = quota_windows(response)?;
    let tier = response
        .subs_usage
        .as_ref()
        .map_or("", |usage| usage.tier.trim());
    let plan = Some(response.subs_tier_name.trim())
        .filter(|name| !name.is_empty())
        .or(Some(tier))
        .filter(|name| name.chars().any(char::is_alphabetic))
        .map(str::to_string);
    Some(Ok(Parsed {
        account: Some(account_dto(email)),
        reading: Reading {
            plan,
            windows: windows.into_iter().map(QuotaWindow::into_dto).collect(),
            ..Reading::default()
        },
    }))
}

fn quota_windows(response: &MintResponse) -> Option<Vec<QuotaWindow>> {
    let windows: Vec<QuotaWindow> = match &response.subs_usage {
        Some(usage) => [
            usage.weekly.as_ref().and_then(weekly_window),
            usage.window.as_ref().and_then(session_window),
        ]
        .into_iter()
        .flatten()
        .collect(),
        None if response.is_subs_active == Some(true) => {
            vec![QuotaWindow::idle(SESSION, FIVE_HOURS)]
        }
        None => return None,
    };
    (!windows.is_empty()).then_some(windows)
}

fn weekly_window(value: &MintWindow) -> Option<QuotaWindow> {
    Some(QuotaWindow {
        id: WEEKLY.to_string(),
        scope: Scope::AllModels,
        length: Some(ONE_WEEK),
        used_percent: value.used_percent?,
        reset: reset(value),
    })
}

fn session_window(value: &MintWindow) -> Option<QuotaWindow> {
    let length = value
        .window_duration_mins
        .filter(|minutes| minutes.is_finite() && *minutes > 0.0)
        .map_or(FIVE_HOURS, |minutes| {
            Duration::from_secs((minutes as u64).saturating_mul(60))
        });
    Some(QuotaWindow {
        id: SESSION.to_string(),
        scope: Scope::AllModels,
        length: Some(length),
        used_percent: value.used_percent?,
        reset: reset(value),
    })
}

fn reset(value: &MintWindow) -> Option<DateTime<Utc>> {
    value
        .resets_at
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .and_then(|seconds| DateTime::from_timestamp_millis((seconds * 1000.0) as i64))
}

#[cfg(test)]
pub(super) mod fixtures;
#[cfg(test)]
mod tests;
