use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::json::window;
use super::pipeline::finish;
use super::{codex_app_server, credits_spent, renewal, saved_card, Parsed};
use super::{SavedReadError, SavedReadUrls};
use crate::accounts::codex_store::CodexAccess;
use crate::accounts::model::{AccessToken, Identity};
use crate::dto::{
    AgentId, LimitWindowDto, LimitWindowKind, LimitsBankedResetDto, LimitsCreditsDto,
    LimitsPriceDto, LimitsResetCreditsDto, LimitsResetOfferDto, LimitsStatus,
    LimitsWorkspaceCreditsDto, ProviderLimitsDto, Reading, ResetCreditOutcome,
};
use crate::http::{get_json, HttpError};
use crate::paths;

const WEEKLY_THRESHOLD_SECONDS: u64 = 24 * 60 * 60;

const HIDDEN_BUCKETS: [&str; 2] = ["base_model_inference", "codex_bengalfox"];
const HIDDEN_NAMES: [&str; 2] = ["gpt-reserve", "gpt-5.3-codex-spark"];

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct RateLimitsResponse {
    rate_limits: RateLimitBucket,
    #[serde(default)]
    rate_limits_by_limit_id: Option<BTreeMap<String, RateLimitBucket>>,
    #[serde(default)]
    rate_limit_reset_credits: Option<RateLimitResetCredits>,
    #[serde(default)]
    rate_limit_upsell: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RateLimitBucket {
    #[serde(default)]
    limit_id: Option<String>,
    #[serde(default)]
    limit_name: Option<String>,
    #[serde(default)]
    primary: Option<RateLimitWindow>,
    #[serde(default)]
    secondary: Option<RateLimitWindow>,
    #[serde(default)]
    credits: Option<RateLimitCredits>,
    #[serde(default)]
    individual_limit: Option<serde_json::Value>,
    #[serde(default)]
    spend_control_reached: Option<bool>,
    #[serde(default)]
    plan_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RateLimitWindow {
    used_percent: f64,
    #[serde(default)]
    window_duration_mins: Option<u64>,
    #[serde(default)]
    resets_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RateLimitResetCredits {
    available_count: u64,
    #[serde(default)]
    credits: Option<Vec<RateLimitResetCredit>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RateLimitResetCredit {
    #[serde(default)]
    status: String,
    #[serde(default)]
    expires_at: Option<i64>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RateLimitCredits {
    #[serde(default)]
    has_credits: bool,
    #[serde(default)]
    unlimited: bool,
    #[serde(default)]
    balance: Option<String>,
}

pub(super) fn parse_codex(payload: &RateLimitsResponse) -> Reading {
    let fallback = &payload.rate_limits;
    let main_id = fallback
        .limit_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "codex".to_string());
    let buckets = payload.rate_limits_by_limit_id.as_ref();
    let main = buckets
        .and_then(|items| items.get(&main_id))
        .unwrap_or(fallback);
    let mut windows = Vec::new();
    windows.extend(bucket_windows(&main_id, main, true));
    if let Some(buckets) = buckets {
        for (id, bucket) in buckets {
            if id != &main_id {
                windows.extend(bucket_windows(id, bucket, false));
            }
        }
    }
    drop_hidden(&mut windows);
    Reading {
        plan: main.plan_type.clone(),
        windows,
        credits: credits(main.credits.as_ref()),
        workspace_credits: workspace_credits(
            main.individual_limit.as_ref(),
            main.spend_control_reached,
        ),
        credits_spent: None,
        subscription: None,
        reset_credits: reset_credits(payload.rate_limit_reset_credits.as_ref()),
        reset_offer: reset_offer(payload.rate_limit_upsell.as_ref()),
    }
}

pub(super) fn drop_hidden(windows: &mut Vec<LimitWindowDto>) {
    windows.retain(|window| !is_hidden(window));
}

fn is_hidden(window: &LimitWindowDto) -> bool {
    let name = window
        .label
        .rsplit('·')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    HIDDEN_NAMES.contains(&name.as_str())
        || window.id.strip_prefix("extra:").is_some_and(|bucket| {
            HIDDEN_BUCKETS.iter().any(|hidden| {
                bucket
                    .strip_prefix(hidden)
                    .is_some_and(|slot| slot.is_empty() || slot.starts_with(':'))
            })
        })
}

fn bucket_windows(id: &str, bucket: &RateLimitBucket, main: bool) -> Vec<LimitWindowDto> {
    [
        ("primary", bucket.primary.as_ref()),
        ("secondary", bucket.secondary.as_ref()),
    ]
    .into_iter()
    .filter_map(|(slot, entry)| rate_limit_window(id, bucket, slot, entry?, main))
    .collect()
}

fn rate_limit_window(
    id: &str,
    bucket: &RateLimitBucket,
    slot: &str,
    entry: &RateLimitWindow,
    main: bool,
) -> Option<LimitWindowDto> {
    let used = entry.used_percent.clamp(0.0, 100.0);
    if !used.is_finite() {
        return None;
    }
    let minutes = entry.window_duration_mins;
    let seconds = minutes.and_then(|minutes| minutes.checked_mul(60));
    let duration_kind = match seconds {
        Some(seconds) if seconds < WEEKLY_THRESHOLD_SECONDS => LimitWindowKind::Session,
        _ => LimitWindowKind::Weekly,
    };
    let kind = if main {
        duration_kind
    } else {
        LimitWindowKind::Model
    };
    let prefix = match duration_kind {
        LimitWindowKind::Session => session_length(minutes),
        LimitWindowKind::Weekly | LimitWindowKind::Model => "Weekly".to_string(),
    };
    let target = if main {
        "all models".to_string()
    } else {
        bucket
            .limit_name
            .clone()
            .unwrap_or_else(|| "extra limit".to_string())
    };
    let resets_at = entry.resets_at.and_then(rfc3339_from_epoch);
    let window_id = if main {
        slot.to_string()
    } else if slot == "primary" {
        format!("extra:{id}")
    } else {
        format!("extra:{id}:{slot}")
    };
    Some(LimitWindowDto {
        window_seconds: seconds,
        ..window(
            window_id,
            format!("{prefix} · {target}"),
            kind,
            used,
            resets_at,
        )
    })
}

fn session_length(minutes: Option<u64>) -> String {
    match minutes {
        Some(minutes) if minutes >= 60 && minutes.is_multiple_of(60) => {
            format!("{} hour", minutes / 60)
        }
        Some(minutes) => format!("{minutes} minute"),
        None => "Session".to_string(),
    }
}

fn credits(value: Option<&RateLimitCredits>) -> Option<LimitsCreditsDto> {
    let credits = value?;
    if !credits.has_credits {
        return None;
    }
    Some(LimitsCreditsDto {
        balance: credits.balance.clone().unwrap_or_else(|| "0".to_string()),
        unlimited: credits.unlimited,
    })
}

fn workspace_credits(
    individual_limit: Option<&serde_json::Value>,
    reached: Option<bool>,
) -> Option<LimitsWorkspaceCreditsDto> {
    let share = individual_limit?.as_object()?;
    let amount = |key: &str| {
        let text = match share.get(key)? {
            serde_json::Value::String(text) => text.trim().to_string(),
            serde_json::Value::Number(number) => number.to_string(),
            _ => return None,
        };
        let value: f64 = text.parse().ok()?;
        (value.is_finite() && value >= 0.0).then_some((text, value))
    };
    let (limit, limit_value) = amount("limit")?;
    let (used, used_value) = amount("used")?;
    let reached = reached == Some(true);
    let remaining = share
        .get("remainingPercent")
        .and_then(serde_json::Value::as_f64)
        .filter(|percent| (0.0..=100.0).contains(percent));
    let used_percent = match remaining {
        _ if reached => 100.0,
        Some(remaining) => 100.0 - remaining,
        None if limit_value <= 0.0 => 100.0,
        None => (used_value / limit_value * 100.0).clamp(0.0, 100.0),
    };
    Some(LimitsWorkspaceCreditsDto {
        limit,
        used,
        used_percent,
        resets_at: share
            .get("resetsAt")
            .and_then(serde_json::Value::as_i64)
            .and_then(rfc3339_from_epoch),
        reached,
    })
}

fn rfc3339_from_epoch(epoch: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(epoch, 0).map(|at| at.to_rfc3339())
}

const BUY_RESET: &str = "buy_reset";

const MAX_MINOR_UNITS: u64 = 1_000_000;

fn reset_offer(banner: Option<&serde_json::Value>) -> Option<LimitsResetOfferDto> {
    let cta = banner?
        .get("ctas")?
        .as_array()?
        .iter()
        .find(|cta| cta.get("action").and_then(serde_json::Value::as_str) == Some(BUY_RESET))?;
    Some(LimitsResetOfferDto {
        price: price(cta.get("price")),
    })
}

fn price(price: Option<&serde_json::Value>) -> Option<LimitsPriceDto> {
    let price = price?;
    let currency = price.get("currency").and_then(serde_json::Value::as_str)?;
    let currency = currency.trim();
    if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let amount = price
        .get("amount_minor_units")
        .and_then(serde_json::Value::as_u64)
        .filter(|amount| *amount <= MAX_MINOR_UNITS)?;
    Some(LimitsPriceDto {
        amount_minor_units: amount,
        currency: currency.to_uppercase(),
    })
}

fn reset_credits(value: Option<&RateLimitResetCredits>) -> Option<LimitsResetCreditsDto> {
    let summary = value?;
    let mut available: Vec<(Option<DateTime<Utc>>, Option<String>)> = summary
        .credits
        .iter()
        .flatten()
        .filter(|credit| credit.status == "available")
        .map(|credit| {
            let expires_at = credit
                .expires_at
                .and_then(|at| DateTime::<Utc>::from_timestamp(at, 0));
            let title = credit
                .title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_string);
            (expires_at, title)
        })
        .collect();
    available.sort_by_key(|(expires_at, _)| (expires_at.is_none(), *expires_at));
    let available_count = u32::try_from(summary.available_count).unwrap_or(u32::MAX);
    available.truncate(usize::try_from(available_count).unwrap_or(usize::MAX));
    Some(LimitsResetCreditsDto {
        available_count,
        next_expires_at: available
            .first()
            .and_then(|(at, _)| at.map(|at| at.to_rfc3339())),
        resets: available
            .into_iter()
            .map(|(expires_at, title)| LimitsBankedResetDto {
                title,
                expires_at: expires_at.map(|at| at.to_rfc3339()),
            })
            .collect(),
    })
}

pub(super) fn codex_limits(home: &Path, force: bool) -> ProviderLimitsDto {
    let figures = |card: &mut Parsed, access: Option<&CodexAccess>| {
        backend_figures(card, access, CODEX, Utc::now());
    };
    match codex_app_server::read(home, force, figures) {
        Ok(parsed) => finish(AgentId::Codex, LimitsStatus::Ok, None, parsed),
        Err(codex_app_server::AppServerFailure::SignedOut) => finish(
            AgentId::Codex,
            LimitsStatus::SignedOut,
            Some("Sign in with `codex` to see subscription limits.".to_string()),
            Parsed::default(),
        ),
        Err(codex_app_server::AppServerFailure::Unsupported(message)) => finish(
            AgentId::Codex,
            LimitsStatus::Unsupported,
            Some(message),
            Parsed::default(),
        ),
        Err(codex_app_server::AppServerFailure::Failed(message)) => finish(
            AgentId::Codex,
            LimitsStatus::Failed,
            Some(message),
            Parsed::default(),
        ),
    }
}

#[cfg(test)]
pub(crate) fn codex_card(
    rate_limits: serde_json::Value,
    account_id: &str,
    observed_at: &str,
) -> ProviderLimitsDto {
    let payload = serde_json::from_value(rate_limits).expect("an app-server rate-limits result");
    let mut reading = parse_codex(&payload);
    for window in &mut reading.windows {
        window.observed_at = observed_at.to_string();
    }
    super::signed_in_card(AgentId::Codex, account_id, reading)
}

pub fn consume_codex_reset_credit(
    account_id: &str,
    idempotency_key: &str,
    max_left_percent: u8,
) -> Result<ResetCreditOutcome, String> {
    let home = paths::user_home().map_err(|error| error.message)?;
    let _provider_guard = super::provider_read_guard(AgentId::Codex);
    codex_app_server::consume_reset_credit(&home, account_id, idempotency_key, max_left_percent)
}

const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const CODEX_RESET_CREDITS_URL: &str =
    "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";

#[derive(Debug, Clone, Copy)]
pub(crate) struct CodexEndpoints<'a> {
    pub(crate) usage: &'a str,
    pub(crate) reset_credits: &'a str,
    pub(crate) credit_usage: &'a str,
    pub(crate) subscriptions: &'a str,
}

pub(super) const CODEX: CodexEndpoints<'static> = CodexEndpoints {
    usage: CODEX_USAGE_URL,
    reset_credits: CODEX_RESET_CREDITS_URL,
    credit_usage: credits_spent::CODEX_CREDIT_USAGE_URL,
    subscriptions: renewal::CODEX_SUBSCRIPTIONS_URL,
};

pub(crate) fn read_saved_codex(
    identity: &Identity,
    token: AccessToken,
    urls: &SavedReadUrls<'_>,
) -> Result<ProviderLimitsDto, SavedReadError> {
    let urls = urls.codex;
    let bearer = token.authorization();
    let headers = [
        ("Authorization", bearer.as_str()),
        ("ChatGPT-Account-Id", identity.workspace_id.as_str()),
    ];
    let payload = get_json(urls.usage, &headers)?;
    if payload
        .get("account_id")
        .or_else(|| payload.get("accountId"))
        .and_then(Value::as_str)
        .is_some_and(|id| id != identity.workspace_id)
    {
        return Err(SavedReadError::OtherAccount);
    }
    let details = (banked_reset_count(&payload["rate_limit_reset_credits"])
        .is_some_and(|count| count > 0))
    .then(|| get_json(urls.reset_credits, &headers).ok())
    .flatten();
    let mut card = Parsed {
        account: Some(super::scoped_account(identity, None)),
        reading: parse_codex_usage(&payload, details.as_ref())?,
    };
    let access = CodexAccess {
        observation_key: identity.observation_key(),
        workspace_id: identity.workspace_id.clone(),
        token,
    };
    backend_figures(&mut card, Some(&access), urls, Utc::now());
    saved_card(identity, card)
}

fn backend_figures(
    card: &mut Parsed,
    access: Option<&CodexAccess>,
    urls: CodexEndpoints<'_>,
    now: DateTime<Utc>,
) {
    let access = access.filter(|access| {
        card.account.as_ref().map(|account| account.id.as_str())
            == Some(access.observation_key.as_str())
    });
    let workspace = card
        .reading
        .plan
        .as_deref()
        .is_some_and(credits_spent::is_codex_workspace_plan);
    card.reading.credits_spent = access
        .filter(|_| workspace)
        .and_then(|access| credits_spent::read_backed_off(access, urls.credit_usage, now));
    card.reading.subscription =
        access.and_then(|access| renewal::read_backed_off(access, urls.subscriptions, now));
}

fn parse_codex_usage(payload: &Value, reset_details: Option<&Value>) -> Result<Reading, HttpError> {
    use serde_json::json;
    fn window(value: &Value) -> Value {
        if value.is_null() {
            return Value::Null;
        }
        json!({"usedPercent": value.get("used_percent"),
            "windowDurationMins": value.get("limit_window_seconds").and_then(Value::as_u64).map(|v| v / 60),
            "resetsAt": value.get("reset_at")})
    }
    fn bucket(value: &Value) -> Value {
        json!({"primary":window(&value["primary_window"]), "secondary":window(&value["secondary_window"])})
    }
    let mut main = bucket(&payload["rate_limit"]);
    main["planType"] = payload["plan_type"].clone();
    if let Some(credits) = payload.get("credits").filter(|v| v.is_object()) {
        main["credits"] = json!({"hasCredits":credits["has_credits"].as_bool().unwrap_or(false),
            "unlimited":credits["unlimited"].as_bool().unwrap_or(false),
            "balance":credits.get("balance").and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_f64().map(|n| n.to_string()))) });
    }
    if let Some(spend_control) = payload.get("spend_control").filter(|v| v.is_object()) {
        main["spendControlReached"] = json!(spend_control["reached"].as_bool());
        if let Some(share) = spend_control
            .get("individual_limit")
            .filter(|v| v.is_object())
        {
            main["individualLimit"] = json!({"limit": share["limit"], "used": share["used"],
                "remainingPercent": share["remaining_percent"], "resetsAt": share["reset_at"]});
        }
    }
    let mut buckets = serde_json::Map::new();
    buckets.insert("codex".into(), main.clone());
    for extra in payload
        .get("additional_rate_limits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = extra
            .get("metered_feature")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && *id != "codex")
        else {
            continue;
        };
        let mut entry = bucket(&extra["rate_limit"]);
        entry["limitName"] = extra["limit_name"].clone();
        buckets.insert(id.to_owned(), entry);
    }
    let response = serde_json::from_value(json!({"rateLimits":main,"rateLimitsByLimitId":buckets,
        "rateLimitResetCredits":wham_reset_credits(&payload["rate_limit_reset_credits"], reset_details)}))
    .map_err(|_| HttpError::Parse("Unrecognized Codex quota response.".into()))?;
    Ok(parse_codex(&response))
}

fn banked_reset_count(summary: &Value) -> Option<u64> {
    summary.get("available_count").and_then(Value::as_u64)
}

fn wham_reset_credits(summary: &Value, details: Option<&Value>) -> Option<Value> {
    use serde_json::json;
    let detailed = details.and_then(|details| {
        let credits = details
            .get("credits")?
            .as_array()?
            .iter()
            .map(|credit| {
                let expires_at = match credit.get("expires_at") {
                    None | Some(Value::Null) => None,
                    Some(at) => Some(
                        chrono::DateTime::parse_from_rfc3339(at.as_str()?)
                            .ok()?
                            .timestamp(),
                    ),
                };
                Some(json!({
                    "status": credit.get("status")?.as_str()?,
                    "expiresAt": expires_at,
                    "title": credit.get("title").and_then(Value::as_str),
                }))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(json!({"availableCount": banked_reset_count(details)?, "credits": credits}))
    });
    detailed
        .or_else(|| Some(json!({"availableCount": banked_reset_count(summary)?, "credits": null})))
}

#[cfg(test)]
mod tests;
