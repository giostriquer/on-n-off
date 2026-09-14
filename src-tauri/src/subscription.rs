//! Optional Codex subscription dates. Billing observations retain authority over cached ID-token
//! claims, including an explicit empty billing response. No access or refresh token is read here.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDate {
    pub date: Option<String>,
    pub kind: Option<DateKind>,
    pub source: DateSource,
    pub checked_at: Option<String>,
    pub stale: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DateKind {
    Renews,
    Expires,
    PaidThrough,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DateSource {
    Billing,
    LocalToken,
}

fn parse_billing(payload: &Value, now: DateTime<Utc>) -> Option<SubscriptionDate> {
    let date = payload
        .get("active_until")
        .or_else(|| payload.get("activeUntil"))?;
    let renew = payload
        .get("will_renew")
        .or_else(|| payload.get("willRenew"))?;
    let (date, kind) = if date.is_null() && (renew.is_null() || renew.as_bool() == Some(false)) {
        (None, None)
    } else {
        let date = date.as_str()?.parse::<DateTime<Utc>>().ok()?;
        let kind = if renew.as_bool()? {
            DateKind::Renews
        } else {
            DateKind::Expires
        };
        (Some(date.to_rfc3339()), Some(kind))
    };
    Some(SubscriptionDate {
        date,
        kind,
        source: DateSource::Billing,
        checked_at: Some(now.to_rfc3339()),
        stale: false,
    })
}
fn parse_claims(claims: &Value, account_id: &str, now: DateTime<Utc>) -> Option<SubscriptionDate> {
    if crate::accounts::model::codex_observation_key(
        claims.get("chatgpt_account_id")?.as_str()?,
        claims,
    ) != account_id
    {
        return None;
    }
    let date = claims
        .get("chatgpt_subscription_active_until")?
        .as_str()?
        .parse::<DateTime<Utc>>()
        .ok()?;
    if date <= now {
        return None;
    }
    let checked_at = claims
        .get("chatgpt_subscription_last_checked")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        .filter(|checked| *checked <= now)
        .map(|checked| checked.to_rfc3339());
    Some(SubscriptionDate {
        date: Some(date.to_rfc3339()),
        kind: Some(DateKind::PaidThrough),
        source: DateSource::LocalToken,
        checked_at,
        stale: true,
    })
}
fn choose(
    billing: Option<SubscriptionDate>,
    local: Option<SubscriptionDate>,
) -> Option<SubscriptionDate> {
    billing.or(local)
}

const REFRESH_SECONDS: i64 = 24 * 60 * 60;

pub(crate) mod browser;
#[cfg(target_os = "macos")]
mod import_owner;
mod store;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionReading {
    pub metadata: Option<SubscriptionDate>,
    pub connected: bool,
    pub unavailable: bool,
    pub browser_supported: bool,
    pub can_connect: bool,
}

pub fn read(
    _app: &tauri::AppHandle,
    home: &std::path::Path,
    account: &str,
) -> Result<SubscriptionReading, String> {
    let (reading, context) = browser::read_metadata(home, account, chrono::Utc::now())?;
    browser::refresh_if_due(home, account, context.as_ref());
    Ok(reading)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BillingContext {
    workspace: String,
    user: String,
    saved: bool,
}
fn with_billing_context<T>(
    home: &std::path::Path,
    account: &str,
    consume: impl FnOnce(Result<Option<BillingContext>, String>) -> T,
) -> T {
    crate::accounts::with_saved_billing_identity(home, account, |saved| {
        consume(saved.map(|saved| resolve_context(home, account, saved)))
    })
}
fn billing_context(
    home: &std::path::Path,
    account: &str,
) -> Result<Option<BillingContext>, String> {
    with_billing_context(home, account, |context| context)
}
fn validate_context(context: Result<Option<BillingContext>, String>) -> Result<(), String> {
    context?
        .map(|_| ())
        .ok_or_else(|| "Save this account or sign in with Codex before connecting billing.".into())
}
fn resolve_context(
    home: &std::path::Path,
    account: &str,
    saved: Option<crate::accounts::model::Identity>,
) -> Option<BillingContext> {
    if let Some(identity) = saved
        .filter(|id| id.provider == crate::dto::AgentId::Codex && id.observation_key() == account)
    {
        return Some(BillingContext {
            workspace: identity.workspace_id,
            user: identity.user_id,
            saved: true,
        });
    }
    let (key, claims) = store::identity(home)?;
    if key != account {
        return None;
    }
    Some(BillingContext {
        workspace: claims.get("chatgpt_account_id")?.as_str()?.into(),
        user: claims
            .get("chatgpt_user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        saved: false,
    })
}
pub fn validate_account(home: &std::path::Path, account: &str) -> Result<(), String> {
    validate_context(billing_context(home, account))
}
pub use browser::{connect, disconnect};
pub fn forget(home: &std::path::Path, account: &str) -> Result<(), String> {
    browser::forget(home, account)
}

#[cfg(test)]
mod tests;

#[cfg(target_os = "macos")]
pub fn recover_imports() {
    if let Ok(root) = import_owner::root() {
        import_owner::recover(&root);
    }
}
#[cfg(target_os = "macos")]
pub fn shutdown() {
    import_owner::OWNER.shutdown();
}
