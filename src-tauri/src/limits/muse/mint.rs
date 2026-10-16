use serde::Deserialize;
use serde_json::json;

use super::{account_dto, session_label, window, ServeFailure};
use crate::accounts::model::AccessToken;
use crate::dto::{LimitWindowDto, LimitWindowKind, Reading};
use crate::limits::Parsed;

pub(super) const KEY_ENDPOINT: &str = "https://api.meta.ai/muse-code/key";

const DEFAULT_ROLLING_MINS: u64 = 300;

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

pub(super) fn fetch(url: &str, token: &AccessToken) -> Option<MintResponse> {
    let authorization = token.authorization();
    let value = crate::http::post_json_headers(
        url,
        &[
            ("Authorization", authorization.as_str()),
            ("X-Api-Version", "1.0.0"),
        ],
        &json!({}),
    )
    .ok()?;
    serde_json::from_value(value).ok()
}

pub(super) fn normalize(
    email: Option<String>,
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
