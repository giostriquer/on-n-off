//! Credits a business Codex member has spent lately: the figure the Codex app's "Credit usage
//! history" shows a workspace member, read from the same per-member endpoint and summed the same way.
//!
//! A workspace member's own credit balance reads 0 and, without a per-member cap, Codex reports no
//! share of the pooled credits either (`individualLimit` is null), so spending is the one credit
//! figure a member can see. The workspace-wide endpoint the app's admins read answers a member 403.

use chrono::{DateTime, Days, NaiveDate};
use serde_json::Value;

use crate::dto::LimitsCreditsSpentDto;

/// The per-member daily breakdown the Codex app reads for a business member's usage history.
pub(crate) const CODEX_CREDIT_USAGE_URL: &str =
    "https://chatgpt.com/backend-api/wham/usage/daily-workspace-user-token-usage-breakdown";

/// Whether a Codex plan type belongs to a workspace, where credits are pooled: Codex's own
/// `PlanType::is_workspace_account` (openai/codex rust-v0.156.1, `codex-rs/protocol/src/account.rs`),
/// which counts team-like, business-like and education-like plans and enterprise.
pub(crate) fn is_workspace_plan(plan: &str) -> bool {
    matches!(
        plan,
        "team"
            | "self_serve_business_prolite"
            | "self_serve_business_usage_based"
            | "business"
            | "ent26"
            | "enterprise_cbp_automation"
            | "enterprise_cbp_usage_based"
            | "enterprise"
            | "edu"
            | "edu_plus"
            | "edu_pro"
    )
}

/// The breakdown for the 30 UTC days up to and including `today`, one row per day, the window the
/// app asks for: `start = today - (days - 1)`.
pub(crate) fn query_url(base: &str, today: NaiveDate) -> String {
    let start = today - Days::new(LONG_WINDOW_DAYS - 1);
    format!(
        "{base}?start_date={}&end_date={}&group_by=day",
        start.format("%Y-%m-%d"),
        today.format("%Y-%m-%d")
    )
}

/// The days the card's second figure covers, and the most any read asks for.
const LONG_WINDOW_DAYS: u64 = 30;
/// The days the card's headline figure covers, the app's default view.
const SHORT_WINDOW_DAYS: u64 = 7;

/// The last 7 and 30 days' spending from one breakdown response. Like the app, a day's spending is
/// the credits its models used, and only a breakdown counted in credits is read. An amount that is
/// not a finite count of at least zero, or a row without a readable date, adds nothing.
pub(crate) fn parse(payload: &Value, today: NaiveDate) -> Option<LimitsCreditsSpentDto> {
    if payload.get("units").and_then(Value::as_str) != Some("credits") {
        return None;
    }
    let long_start = today - Days::new(LONG_WINDOW_DAYS - 1);
    let short_start = today - Days::new(SHORT_WINDOW_DAYS - 1);
    let mut spent = LimitsCreditsSpentDto {
        last_7_days: 0.0,
        last_30_days: 0.0,
        updated_at: payload
            .get("data_freshness_ts")
            .and_then(Value::as_str)
            .filter(|at| DateTime::parse_from_rfc3339(at).is_ok())
            .map(str::to_string),
    };
    for row in payload.get("data")?.as_array()? {
        let Some(date) = row
            .get("date")
            .and_then(Value::as_str)
            .and_then(|date| date.get(..10))
            .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        else {
            continue;
        };
        if date < long_start || date > today {
            continue;
        }
        let credits: f64 = row
            .get("models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|model| model.get("credits").and_then(Value::as_f64))
            .filter(|credits| credits.is_finite() && *credits >= 0.0)
            .sum();
        spent.last_30_days += credits;
        if date >= short_start {
            spent.last_7_days += credits;
        }
    }
    Some(spent)
}

#[cfg(test)]
mod tests;
