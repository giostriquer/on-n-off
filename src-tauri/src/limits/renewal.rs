use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

use super::backend_memo::PerAccount;
use crate::accounts::codex_store::CodexAccess;
use crate::accounts::model::AccessToken;
use crate::dto::{AgentId, LimitsSubscriptionDto, ProviderLimitsDto, SubscriptionNote};

pub(super) const CODEX_SUBSCRIPTIONS_URL: &str = "https://chatgpt.com/backend-api/subscriptions";

const FRESH_FOR: Duration = Duration::from_secs(24 * 60 * 60);

fn query_url(base: &str, workspace_id: &str) -> String {
    let workspace: String = workspace_id
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect();
    format!("{base}?account_id={workspace}")
}

pub(super) fn asks_about_renewal(card: &ProviderLimitsDto) -> bool {
    card.provider == AgentId::Codex && card.account.is_some()
}

fn instant(value: Option<&Value>) -> Option<DateTime<Utc>> {
    value
        .and_then(Value::as_str)
        .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.with_timezone(&Utc))
}

fn present(value: Option<&Value>) -> bool {
    value.is_some_and(|value| !value.is_null())
}

fn parse(payload: &Value, now: DateTime<Utc>) -> Option<LimitsSubscriptionDto> {
    let entitlement = payload.get("entitlement");
    let active_until = instant(payload.get("active_until"))?;
    let will_renew = payload.get("will_renew").and_then(Value::as_bool)?;
    let note = if present(payload.get("cancellation_outcome"))
        || present(entitlement.and_then(|e| e.get("cancels_at")))
    {
        Some(SubscriptionNote::Cancelled)
    } else if entitlement
        .and_then(|e| e.get("is_delinquent"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        Some(SubscriptionNote::PastDue)
    } else if present(entitlement.and_then(|e| e.get("scheduled_plan_change"))) {
        Some(SubscriptionNote::PlanChange)
    } else {
        None
    };
    Some(LimitsSubscriptionDto {
        active_until: active_until.to_rfc3339_opts(SecondsFormat::Secs, true),
        will_renew,
        note,
        checked_at: now.to_rfc3339_opts(SecondsFormat::Secs, true),
    })
}

fn read(
    token: &AccessToken,
    workspace_id: &str,
    url: &str,
    now: DateTime<Utc>,
) -> Option<LimitsSubscriptionDto> {
    let authorization = token.authorization();
    let payload = crate::http::get_json(
        &query_url(url, workspace_id),
        &[
            ("Authorization", authorization.as_str()),
            ("ChatGPT-Account-Id", workspace_id),
        ],
    )
    .ok()?;
    parse(&payload, now)
}

static MEMO: PerAccount<LimitsSubscriptionDto> = PerAccount::new(Some(FRESH_FOR));

pub(super) fn read_backed_off(
    access: &CodexAccess,
    url: &str,
    now: DateTime<Utc>,
) -> Option<LimitsSubscriptionDto> {
    MEMO.read_backed_off(&access.observation_key, || {
        read(&access.token, &access.workspace_id, url, now)
    })
}

#[cfg(test)]
mod tests;
