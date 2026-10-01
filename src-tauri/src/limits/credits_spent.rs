use chrono::{DateTime, Days, NaiveDate, SecondsFormat, Utc};
use serde_json::Value;

use super::backend_memo::PerAccount;
use crate::accounts::codex_store::CodexAccess;
use crate::accounts::model::AccessToken;
use crate::dto::{AgentId, LimitsCreditsSpentDto, ProviderLimitsDto};

pub(crate) const CODEX_CREDIT_USAGE_URL: &str =
    "https://chatgpt.com/backend-api/wham/usage/daily-workspace-user-token-usage-breakdown";

pub(crate) fn is_codex_workspace_plan(plan: &str) -> bool {
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

pub(crate) fn asks_what_was_spent(card: &ProviderLimitsDto) -> bool {
    card.provider == AgentId::Codex
        && card
            .reading
            .plan
            .as_deref()
            .is_some_and(is_codex_workspace_plan)
}

pub(crate) fn query_url(base: &str, today: NaiveDate) -> String {
    let start = today - Days::new(LONG_WINDOW_DAYS - 1);
    format!(
        "{base}?start_date={}&end_date={}&group_by=day",
        start.format("%Y-%m-%d"),
        today.format("%Y-%m-%d")
    )
}

const LONG_WINDOW_DAYS: u64 = 30;
const SHORT_WINDOW_DAYS: u64 = 7;

pub(crate) fn parse(payload: &Value, now: DateTime<Utc>) -> Option<LimitsCreditsSpentDto> {
    let today = now.date_naive();
    if payload.get("units").and_then(Value::as_str) != Some("credits") {
        return None;
    }
    let long_start = today - Days::new(LONG_WINDOW_DAYS - 1);
    let short_start = today - Days::new(SHORT_WINDOW_DAYS - 1);
    let mut spent = LimitsCreditsSpentDto {
        last_7_days: 0.0,
        last_30_days: 0.0,
        updated_at: Some(
            payload
                .get("data_freshness_ts")
                .and_then(Value::as_str)
                .filter(|at| DateTime::parse_from_rfc3339(at).is_ok())
                .map_or_else(
                    || now.to_rfc3339_opts(SecondsFormat::Secs, true),
                    str::to_string,
                ),
        ),
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

pub(crate) fn read(
    token: &AccessToken,
    workspace_id: &str,
    url: &str,
    now: DateTime<Utc>,
) -> Option<LimitsCreditsSpentDto> {
    let authorization = token.authorization();
    let payload = crate::http::get_json(
        &query_url(url, now.date_naive()),
        &[
            ("Authorization", authorization.as_str()),
            ("ChatGPT-Account-Id", workspace_id),
        ],
    )
    .ok()?;
    parse(&payload, now)
}

static MEMO: PerAccount<LimitsCreditsSpentDto> = PerAccount::new(None);

pub(super) fn read_backed_off(
    access: &CodexAccess,
    url: &str,
    now: DateTime<Utc>,
) -> Option<LimitsCreditsSpentDto> {
    MEMO.read_backed_off(&access.observation_key, || {
        read(&access.token, &access.workspace_id, url, now)
    })
}

#[cfg(test)]
mod tests;
