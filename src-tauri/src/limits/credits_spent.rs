//! Credits a business Codex member has spent lately: the figure the Codex app's "Credit usage
//! history" shows a workspace member, read from the same per-member endpoint and summed the same way.
//!
//! A workspace member's own credit balance reads 0 and, without a per-member cap, Codex reports no
//! share of the pooled credits either (`individualLimit` is null), so spending is the one credit
//! figure a member can see. The workspace-wide endpoint the app's admins read answers a member 403.

use chrono::{DateTime, Days, NaiveDate, SecondsFormat, Utc};
use serde_json::Value;

use super::backend_memo::PerAccount;
use super::Parsed;
use crate::accounts::codex_store::CodexAccess;
use crate::accounts::model::AccessToken;
use crate::dto::{AgentId, LimitsCreditsSpentDto, ProviderLimitsDto};

/// The per-member daily breakdown the Codex app reads for a business member's usage history.
pub(crate) const CODEX_CREDIT_USAGE_URL: &str =
    "https://chatgpt.com/backend-api/wham/usage/daily-workspace-user-token-usage-breakdown";

/// Whether a Codex plan type belongs to a workspace, where credits are pooled: Codex's own
/// `PlanType::is_workspace_account` (openai/codex rust-v0.156.1, `codex-rs/protocol/src/account.rs`),
/// which counts team-like, business-like and education-like plans and enterprise.
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

/// Whether `card` is one that is asked what it spent: a Codex workspace plan. A successful read of
/// one whose spending read failed or was backing off keeps the remembered figure
/// (`limits/reading.rs`); a card on any other plan keeps nothing, so an account that moved to a
/// personal plan loses the figure it had on its next read.
pub(crate) fn asks_what_was_spent(card: &ProviderLimitsDto) -> bool {
    card.provider == AgentId::Codex
        && card
            .reading
            .plan
            .as_deref()
            .is_some_and(is_codex_workspace_plan)
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
        // Without a freshness time of its own, the figure is as fresh as this read. The card does not
        // show it; it stays with the figure, and with a remembered one, as a record of its age.
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

/// What a Codex login spent lately: one GET to `url` with its access token, for its workspace,
/// covering the 30 UTC days up to `now`. Any failure is no figure.
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

/// What each account answered lately is not kept: spending moves with every day's use, so every
/// refresh asks, and only a failure holds the account back.
static MEMO: PerAccount<LimitsCreditsSpentDto> = PerAccount::new(None);

/// `read` for the account `access` belongs to, unless its last read failed recently.
pub(super) fn read_backed_off(
    access: &CodexAccess,
    url: &str,
    now: DateTime<Utc>,
) -> Option<LimitsCreditsSpentDto> {
    MEMO.read_backed_off(&access.observation_key, || {
        read(&access.token, &access.workspace_id, url, now)
    })
}

/// The signed-in Codex account's spending, asked with its own access token: app-server reads that
/// card without handing one over, and the saved shadow of the signed-in login is never polled.
/// Using the native login's access token for this one read-only GET is the user's decision
/// (2026-09-24), an exception to Codex alone making requests for the signed-in account. It is
/// asked only for a workspace plan, with the access projection the read's identity check took from
/// the login it confirmed (`codex_app_server::normalize_app_server`), and only for that card.
pub(super) fn signed_in(
    access: Option<&CodexAccess>,
    parsed: &Parsed,
    url: &str,
    now: DateTime<Utc>,
) -> Option<LimitsCreditsSpentDto> {
    if !parsed
        .reading
        .plan
        .as_deref()
        .is_some_and(is_codex_workspace_plan)
    {
        return None;
    }
    let access = access?;
    if parsed.account.as_ref().map(|account| account.id.as_str()) != Some(&access.observation_key) {
        return None;
    }
    read_backed_off(access, url, now)
}

/// Drop what is remembered about `account`, so a test starts from nothing.
#[cfg(test)]
pub(super) fn forget(account: &str) {
    MEMO.forget(account);
}

#[cfg(test)]
mod tests;
