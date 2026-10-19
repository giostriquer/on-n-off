use serde::Deserialize;
use serde_json::json;

use super::{account_dto, session_label, window, ReadFailure};
use crate::accounts::model::{AccessToken, Identity};
use crate::dto::{LimitWindowDto, LimitWindowKind, ProviderLimitsDto, Reading};
use crate::http::HttpError;
use crate::limits::{saved_card, Parsed, SavedReadError, SavedReadUrls};

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
    Err(ReadFailure::Unread(account_dto(email), why))
}

pub(super) fn normalize(
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
    let usage = match &response.subs_usage {
        Some(usage) => usage,
        None if response.is_subs_active == Some(true) => &IDLE_USAGE,
        None => return None,
    };
    let windows: Vec<LimitWindowDto> = [
        usage.weekly.as_ref().and_then(weekly_window),
        usage.window.as_ref().and_then(session_window),
    ]
    .into_iter()
    .flatten()
    .collect();
    if windows.is_empty() {
        return None;
    }
    let plan = Some(response.subs_tier_name.trim())
        .filter(|name| !name.is_empty())
        .or(Some(usage.tier.trim()))
        .filter(|name| name.chars().any(char::is_alphabetic))
        .map(str::to_string);
    Some(Ok(Parsed {
        account: Some(account_dto(email)),
        reading: Reading {
            plan,
            windows,
            ..Reading::default()
        },
    }))
}

fn weekly_window(value: &MintWindow) -> Option<LimitWindowDto> {
    let used = used_percent(value)?;
    Some(window(
        "weekly",
        "Weekly · all models".to_string(),
        LimitWindowKind::Weekly,
        used,
        resets_at_ms(value),
        "",
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
        "",
    );
    dto.window_seconds = Some(minutes.saturating_mul(60));
    Some(dto)
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
mod tests;
