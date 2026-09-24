//! Access-only saved-account reads. No CLI is started and no credential is renewed here.
use super::*;
use crate::accounts::model::Identity;
use serde_json::Value;

const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// Each banked reset's status and expiry; the usage body carries only the count.
const CODEX_RESET_CREDITS_URL: &str =
    "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";

/// The services one saved Codex read talks to, together as `ClaudeEndpoints` keeps Claude's, so a
/// fourth costs one field and not an edit at every call site.
#[derive(Debug, Clone, Copy)]
struct CodexEndpoints<'a> {
    usage: &'a str,
    reset_credits: &'a str,
    /// What a workspace member spent (`credits_spent.rs`); asked only for a workspace plan.
    credit_usage: &'a str,
}

const CODEX: CodexEndpoints<'static> = CodexEndpoints {
    usage: CODEX_USAGE_URL,
    reset_credits: CODEX_RESET_CREDITS_URL,
    credit_usage: credits_spent::CODEX_CREDIT_USAGE_URL,
};

pub(crate) fn read(identity: &Identity, auth: &Value) -> Result<ProviderLimitsDto, HttpError> {
    read_at(identity, auth, CLAUDE_PROFILE_URL, CLAUDE_USAGE_URL, CODEX)
}

fn read_at(
    identity: &Identity,
    auth: &Value,
    profile: &str,
    claude_url: &str,
    codex: CodexEndpoints<'_>,
) -> Result<ProviderLimitsDto, HttpError> {
    let mut parsed = match identity.provider {
        AgentId::Claude => {
            let credential =
                credentials::parse_claude_credential(auth).ok_or(HttpError::Unauthorized)?;
            let bearer = format!("Bearer {}", credential.token);
            let profile = claude::parse_profile(&get_json(
                profile,
                &[
                    ("Authorization", &bearer),
                    ("anthropic-beta", "oauth-2025-04-20"),
                ],
            )?)
            .map_err(HttpError::Parse)?;
            if profile.account.id != identity.user_id
                || profile.organization_id.as_deref() != Some(&identity.workspace_id)
            {
                return Err(HttpError::Unauthorized);
            }
            let usage = claude_usage(
                claude_url,
                &[
                    ("Authorization", &bearer),
                    ("anthropic-beta", "oauth-2025-04-20"),
                ],
            )?;
            Parsed {
                account: Some(profile.account),
                plan: credential.plan(),
                ..usage
            }
        }
        AgentId::Codex => {
            let token = crate::accounts::model::string(auth, "/tokens/access_token")
                .map_err(|_| HttpError::Unauthorized)?;
            let bearer = format!("Bearer {token}");
            let headers = [
                ("Authorization", bearer.as_str()),
                ("ChatGPT-Account-Id", identity.workspace_id.as_str()),
            ];
            let payload = get_json(codex.usage, &headers)?;
            if payload
                .get("account_id")
                .or_else(|| payload.get("accountId"))
                .and_then(Value::as_str)
                .is_some_and(|id| id != identity.workspace_id)
            {
                return Err(HttpError::Unauthorized);
            }
            // As in Codex's own app-server, the detail read never decides the read: without it the
            // count still stands, only without an expiry.
            let details = (banked_reset_count(&payload["rate_limit_reset_credits"])
                .is_some_and(|count| count > 0))
            .then(|| get_json(codex.reset_credits, &headers).ok())
            .flatten();
            let mut parsed = parse_codex_usage(&payload, details.as_ref())?;
            // Only a workspace pools credits, and like the detail read, spending never decides
            // the read: a member the endpoint refuses just has no figure.
            if parsed
                .plan
                .as_deref()
                .is_some_and(credits_spent::is_codex_workspace_plan)
            {
                parsed.credits_spent = credits_spent::read_backed_off(
                    &identity.observation_key(),
                    &crate::accounts::model::AccessToken::new(token),
                    &identity.workspace_id,
                    codex.credit_usage,
                    Utc::now(),
                );
            }
            parsed
        }
        _ => return Err(HttpError::Unauthorized),
    };
    let label = parsed.account.as_ref().and_then(|a| a.label.clone());
    parsed.account = Some(LimitsAccountDto {
        id: identity.observation_key(),
        label,
        legacy_id: Some(if identity.provider == AgentId::Codex {
            identity.workspace_id.clone()
        } else {
            identity.user_id.clone()
        }),
    });
    let mut dto = finish(identity.provider, LimitsStatus::Ok, None, parsed);
    dto.current_account = false;
    if !dto.has_observations() {
        return Err(HttpError::Parse(
            "Usage response contained no quota observations.".into(),
        ));
    }
    Ok(dto)
}

/// The HTTP response uses seconds and snake_case; the existing app-server parser uses minutes
/// and camelCase. Map only the documented quota buckets, the credits, a business member's
/// workspace-credit share and the banked resets. Missing reset inventory is unknown.
fn parse_codex_usage(payload: &Value, reset_details: Option<&Value>) -> Result<Parsed, HttpError> {
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
    // A business member's share of the workspace's credits, in app-server's names.
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
        "rateLimitResetCredits":reset_credits(&payload["rate_limit_reset_credits"], reset_details)}))
    .map_err(|_| HttpError::Parse("Unrecognized Codex quota response.".into()))?;
    Ok(codex::parse_codex(&response))
}

/// `available_count` as Codex's own client reads it: a whole number, and here at least zero.
fn banked_reset_count(summary: &Value) -> Option<u64> {
    summary.get("available_count").and_then(Value::as_u64)
}

/// Banked resets in app-server shape. The usage body's `rate_limit_reset_credits` holds only the
/// count; the detail read, when it answered in full, holds each reset's status and expiry and wins,
/// as it does in Codex's app-server. A count that cannot be read is unknown, never zero.
fn reset_credits(summary: &Value, details: Option<&Value>) -> Option<Value> {
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
                Some(json!({"status": credit.get("status")?.as_str()?, "expiresAt": expires_at}))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(json!({"availableCount": banked_reset_count(details)?, "credits": credits}))
    });
    detailed
        .or_else(|| Some(json!({"availableCount": banked_reset_count(summary)?, "credits": null})))
}

#[cfg(test)]
mod tests;
