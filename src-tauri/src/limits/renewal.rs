//! Whether a Codex subscription renews: the term ChatGPT's own billing endpoint reports for the
//! account, read with the login's access token the way `credits_spent.rs` reads spending. The
//! login's ID token gives the end of the paid period (`subscription.rs`) but not whether the plan
//! renews then, and that is what tells a user which subscription to spend down before it ends.
//!
//! Every Codex card asks, once a day: a term changes with a cancellation or a plan change, not
//! with usage, so a fresh answer is kept in memory for the day and the card fills from the one it
//! remembers when a read fails or is backing off.

use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

use super::backend_memo::PerAccount;
use super::Parsed;
use crate::accounts::codex_store::CodexAccess;
use crate::accounts::model::AccessToken;
use crate::dto::{AgentId, LimitsSubscriptionDto, ProviderLimitsDto, SubscriptionNote};

/// The account's subscription term, the endpoint ChatGPT's own billing settings read.
pub(super) const CODEX_SUBSCRIPTIONS_URL: &str = "https://chatgpt.com/backend-api/subscriptions";

/// How long a successful answer stands before the account is asked again.
const FRESH_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// The endpoint answers 400 without the account: the `ChatGPT-Account-Id` header alone is not
/// enough, the workspace goes in the query.
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

/// Whether `card` is one that is asked about its term: any Codex account. A successful read of one
/// whose term read failed or was backing off keeps the remembered term (`limits/reading.rs`).
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

/// The term one response describes. `active_until` and `will_renew` are both required, and only
/// those: `entitlement.expires_at` sits hours after `active_until` on a live answer, so it is not
/// the same date under another name, and a term the card cannot stand behind is none. A
/// cancellation, a scheduled plan change or an overdue payment is reported as the note,
/// cancellation first since it is the one that ends the plan.
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

/// The account's term: one GET to `url` with its access token. Any failure is no term.
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

/// An answer stands for the day: a term changes with a cancellation or a plan change, not with use.
static MEMO: PerAccount<LimitsSubscriptionDto> = PerAccount::new(Some(FRESH_FOR));

/// `read` for the account `access` belongs to, unless its last answer still stands or its last
/// read failed recently.
pub(super) fn read_backed_off(
    access: &CodexAccess,
    url: &str,
    now: DateTime<Utc>,
) -> Option<LimitsSubscriptionDto> {
    MEMO.read_backed_off(&access.observation_key, || {
        read(&access.token, &access.workspace_id, url, now)
    })
}

/// The signed-in Codex account's term, asked with its own access token under the same decision as
/// `credits_spent::signed_in` (extended to this read on 2026-09-25): only for the card the read's
/// identity check confirmed.
pub(super) fn signed_in(
    access: Option<&CodexAccess>,
    parsed: &Parsed,
    url: &str,
    now: DateTime<Utc>,
) -> Option<LimitsSubscriptionDto> {
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
