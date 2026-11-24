use crate::timestamp::TimestampExt;
use jiff::civil::Date;
use jiff::Timestamp;
use serde_json::Value;
use std::time::Duration;

use super::backend_memo::PerAccount;
use crate::accounts::codex_store::CodexAccess;
use crate::accounts::model::AccessToken;
use crate::dto::{LimitsCreditsSpentDto, ProviderLimitsDto};
use crate::provider_catalogue::CreditsSpent;

pub(crate) const CODEX_CREDIT_USAGE_URL: &str =
    "https://chatgpt.com/backend-api/wham/usage/daily-workspace-user-token-usage-breakdown";

pub(crate) fn asks_what_was_spent(card: &ProviderLimitsDto) -> bool {
    matches!(
        card.provider.limits_facts().credits_spent,
        Some(CreditsSpent::Codex)
    ) && card
        .reading
        .plan
        .as_deref()
        .is_some_and(super::plan::is_codex_workspace_plan)
}

pub(crate) fn query_url(base: &str, today: Date) -> String {
    let start = today - jiff::Span::new().days(LONG_WINDOW_DAYS - 1);
    format!(
        "{base}?start_date={}&end_date={}&group_by=day",
        start, today
    )
}

const LONG_WINDOW_DAYS: i64 = 30;
const SHORT_WINDOW_DAYS: i64 = 7;

pub(crate) fn parse(payload: &Value, now: Timestamp) -> Option<LimitsCreditsSpentDto> {
    let today = now.utc_date();
    if payload.get("units").and_then(Value::as_str) != Some("credits") {
        return None;
    }
    let long_start = today - jiff::Span::new().days(LONG_WINDOW_DAYS - 1);
    let short_start = today - jiff::Span::new().days(SHORT_WINDOW_DAYS - 1);
    let mut spent = LimitsCreditsSpentDto {
        last_7_days: 0.0,
        last_30_days: 0.0,
        updated_at: Some(
            payload
                .get("data_freshness_ts")
                .and_then(Value::as_str)
                .filter(|at| crate::timestamp::parse(at).is_ok())
                .map_or_else(|| now.utc_seconds_text(), str::to_string),
        ),
    };
    for row in payload.get("data")?.as_array()? {
        let Some(date) = row
            .get("date")
            .and_then(Value::as_str)
            .and_then(|date| date.get(..10))
            .and_then(|date| date.parse::<Date>().ok())
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
    now: Timestamp,
) -> Option<LimitsCreditsSpentDto> {
    let authorization = token.authorization();
    let payload = crate::http::get_json(
        &query_url(url, now.utc_date()),
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
    interval: Duration,
    now: Timestamp,
) -> Option<LimitsCreditsSpentDto> {
    MEMO.read_backed_off(&access.observation_key, interval, || {
        read(&access.token, &access.workspace_id, url, now)
    })
}

#[cfg(test)]
mod tests;
