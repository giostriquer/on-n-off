//! Credits a business Codex member has spent lately: the figure the Codex app's "Credit usage
//! history" shows a workspace member, read from the same per-member endpoint and summed the same way.
//!
//! A workspace member's own credit balance reads 0 and, without a per-member cap, Codex reports no
//! share of the pooled credits either (`individualLimit` is null), so spending is the one credit
//! figure a member can see. The workspace-wide endpoint the app's admins read answers a member 403.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Days, NaiveDate, SecondsFormat, Utc};
use serde_json::Value;

use crate::accounts::model::AccessToken;
use crate::accounts::native::CodexAccess;
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

/// Whether `card` is one that is asked what it spent: a Codex workspace plan.
pub(crate) fn asks_what_was_spent(card: &ProviderLimitsDto) -> bool {
    card.provider == AgentId::Codex && card.plan.as_deref().is_some_and(is_codex_workspace_plan)
}

/// A successful read whose spending read failed or was backing off could not tell what was spent,
/// and keeps the figure `previous` knew; one that answered replaces it. Only a Codex workspace plan
/// is ever asked, so a card on any other plan keeps nothing: an account that moved to a personal
/// plan loses the figure it had on its next read.
pub(crate) fn keep_credits_spent_from(card: &mut ProviderLimitsDto, previous: &ProviderLimitsDto) {
    if card.credits_spent.is_none() && asks_what_was_spent(card) {
        card.credits_spent.clone_from(&previous.credits_spent);
    }
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

/// Accounts whose last spending read failed, and when each may be asked again.
static FAILURES: OnceLock<Mutex<HashMap<String, (Instant, u32)>>> = OnceLock::new();

/// `read`, unless this account's last one failed recently. The read sits in series with the usage
/// read, and every request here is bounded by `http`'s timeout, so an endpoint that fails or hangs
/// would otherwise add up to that timeout to every refresh. After a failure the account waits a
/// poll interval, doubling with each further failure up to an hour, as a saved account's polling
/// does (`accounts/usage.rs`); a success clears it. A skipped read is no figure, which the card
/// fills from the one it remembers.
pub(crate) fn read_backed_off(
    account: &str,
    token: &AccessToken,
    workspace_id: &str,
    url: &str,
    now: DateTime<Utc>,
) -> Option<LimitsCreditsSpentDto> {
    let failures = FAILURES.get_or_init(Mutex::default);
    let previous = failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(account)
        .copied();
    if previous.is_some_and(|(until, _)| Instant::now() < until) {
        return None;
    }
    let spent = read(token, workspace_id, url, now);
    let mut failures = failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if spent.is_some() {
        failures.remove(account);
    } else {
        let count = previous.map_or(1, |(_, count)| count.saturating_add(1));
        let delay = backoff_delay(count, crate::limits_refresh::poll_interval());
        failures.insert(account.to_string(), (Instant::now() + delay, count));
    }
    spent
}

/// How long an account waits after its `count`th failure in a row: one poll interval, doubling with
/// each further failure up to sixteen intervals, and never more than an hour.
fn backoff_delay(count: u32, interval: Duration) -> Duration {
    interval
        .saturating_mul(1 << count.saturating_sub(1).min(4))
        .min(Duration::from_secs(3600))
}

/// The signed-in Codex account's spending, asked with its own access token: app-server reads that
/// card without handing one over, and the saved shadow of the signed-in login is never polled.
/// Using the native login's access token for this one read-only GET is the user's decision
/// (2026-09-24), an exception to Codex alone making requests for the signed-in account. It is
/// asked only for a workspace plan, with the access projection the read's identity check took from
/// the login it confirmed (`codex_app_server::normalize_app_server`), and only for that card.
pub(super) fn signed_in(
    access: Option<CodexAccess>,
    account_id: Option<&str>,
    plan: Option<&str>,
) -> Option<LimitsCreditsSpentDto> {
    signed_in_with(access, account_id, plan, CODEX_CREDIT_USAGE_URL, Utc::now())
}

fn signed_in_with(
    access: Option<CodexAccess>,
    account_id: Option<&str>,
    plan: Option<&str>,
    url: &str,
    now: DateTime<Utc>,
) -> Option<LimitsCreditsSpentDto> {
    if !plan.is_some_and(is_codex_workspace_plan) {
        return None;
    }
    let access = access?;
    if account_id != Some(access.observation_key.as_str()) {
        return None;
    }
    read_backed_off(
        &access.observation_key,
        &access.token,
        &access.workspace_id,
        url,
        now,
    )
}

#[cfg(test)]
mod tests;
