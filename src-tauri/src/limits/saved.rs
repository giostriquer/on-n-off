//! Access-only saved-account reads. No CLI is started and no credential is renewed here.
use super::*;
use crate::accounts::model::Identity;
use serde_json::Value;

pub(crate) fn read(identity: &Identity, auth: &Value) -> Result<ProviderLimitsDto, HttpError> {
    read_at(
        identity,
        auth,
        CLAUDE_PROFILE_URL,
        CLAUDE_USAGE_URL,
        "https://chatgpt.com/backend-api/wham/usage",
    )
}

fn read_at(
    identity: &Identity,
    auth: &Value,
    profile: &str,
    claude_url: &str,
    codex_url: &str,
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
            let payload = get_json(
                claude_url,
                &[
                    ("Authorization", &bearer),
                    ("anthropic-beta", "oauth-2025-04-20"),
                ],
            )?;
            Parsed {
                account: Some(profile.account),
                plan: credential.plan(),
                windows: claude::parse_claude(&payload),
                ..Parsed::default()
            }
        }
        AgentId::Codex => {
            let token = crate::accounts::model::string(auth, "/tokens/access_token")
                .map_err(|_| HttpError::Unauthorized)?;
            let payload = get_json(
                codex_url,
                &[
                    ("Authorization", &format!("Bearer {token}")),
                    ("ChatGPT-Account-Id", &identity.workspace_id),
                ],
            )?;
            if payload
                .get("account_id")
                .or_else(|| payload.get("accountId"))
                .and_then(Value::as_str)
                .is_some_and(|id| id != identity.workspace_id)
            {
                return Err(HttpError::Unauthorized);
            }
            parse_codex_usage(&payload)?
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
/// and camelCase. Map only the documented quota buckets. Missing reset inventory is unknown.
fn parse_codex_usage(payload: &Value) -> Result<Parsed, HttpError> {
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
    let response = serde_json::from_value(json!({"rateLimits":main,"rateLimitsByLimitId":buckets}))
        .map_err(|_| HttpError::Parse("Unrecognized Codex quota response.".into()))?;
    Ok(codex::parse_codex(&response))
}

#[cfg(test)]
mod tests;
