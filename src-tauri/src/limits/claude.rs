//! Claude subscription limits: parse `GET api.anthropic.com/api/oauth/usage` (pure).
//!
//! The payload carries a normalized `limits[]` array (kind/group/percent/resets_at) plus the
//! older top-level `five_hour` / `seven_day` / `seven_day_<model>` objects. The array wins when
//! it has usable entries; the legacy keys are the fallback. The same read reports saved resets.

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::credentials::ClaudeIdentity;
use super::json::{humanize, optional_string, percent, window};
use crate::dto::{LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsResetCreditsDto};

pub(super) fn parse_profile(payload: &Value) -> Result<ClaudeIdentity, String> {
    let account = payload
        .get("account")
        .ok_or_else(|| "missing account".to_string())?;
    let organization = payload
        .get("organization")
        .ok_or_else(|| "missing organization".to_string())?;
    Ok(ClaudeIdentity {
        account: LimitsAccountDto {
            legacy_id: None,
            id: optional_string(account.get("uuid"))
                .ok_or_else(|| "missing account uuid".to_string())?,
            label: optional_string(account.get("email")),
        },
        organization_id: Some(
            optional_string(organization.get("uuid"))
                .ok_or_else(|| "missing organization uuid".to_string())?,
        ),
    })
}

pub fn parse_claude(payload: &Value) -> Vec<LimitWindowDto> {
    let normalized: Vec<LimitWindowDto> = payload
        .get("limits")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(normalized_window).collect())
        .unwrap_or_default();
    if !normalized.is_empty() {
        return normalized;
    }
    legacy_windows(payload)
}

fn normalized_window(entry: &Value) -> Option<LimitWindowDto> {
    let kind = optional_string(entry.get("kind"))?;
    let group = optional_string(entry.get("group"))?;
    let used = percent(entry.get("percent"))?;
    let resets_at = optional_string(entry.get("resets_at"));
    let scope = scope_name(entry.get("scope"));
    let (window_kind, label) = match (group.as_str(), kind.as_str()) {
        ("session", _) => (LimitWindowKind::Session, "5 hour · all models".to_string()),
        ("weekly", "weekly_all") => (LimitWindowKind::Weekly, "Weekly · all models".to_string()),
        ("weekly", other) => {
            let name = match &scope {
                Some(scope) => scope.clone(),
                None => humanize(other.strip_prefix("weekly_").unwrap_or(other)),
            };
            (LimitWindowKind::Model, format!("Weekly · {name}"))
        }
        _ => return None,
    };
    let id = match &scope {
        Some(scope) => format!("{kind}:{scope}"),
        None => kind,
    };
    Some(window(id, label, window_kind, used, resets_at))
}

/// `scope.model.display_name`, else `scope.surface`, for per-model / per-surface windows.
fn scope_name(scope: Option<&Value>) -> Option<String> {
    let scope = scope?;
    optional_string(
        scope
            .get("model")
            .and_then(|model| model.get("display_name")),
    )
    .or_else(|| optional_string(scope.get("surface")))
}

/// Anthropic's saved rate-limit resets, which Claude Code spends with `/limit-reset`: the
/// `cedar_ember` block the usage read carries when asked with `?cedar_ember=1`. The count sums every
/// grant, and the expiry is the soonest `ends_at` among grants still holding a reset.
///
/// `None` is "unknown", never zero: no block, or one saying it could not answer (`unavailable`,
/// which Claude Code treats as a failed read). Any other block is an answer, so a missing or empty
/// `grants` reports 0 and replaces a remembered count.
pub(super) fn parse_reset_credits(payload: &Value) -> Option<LimitsResetCreditsDto> {
    let block = payload.get("cedar_ember")?.as_object()?;
    if block.get("ineligible_reason").and_then(Value::as_str) == Some("unavailable") {
        return None;
    }
    let grants: Vec<(u32, Option<DateTime<Utc>>)> = block
        .get("grants")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|grant| {
            let left = grant.get("resets_left")?.as_u64()?;
            let ends_at = optional_string(grant.get("ends_at"))
                .and_then(|at| DateTime::parse_from_rfc3339(&at).ok())
                .map(|at| at.with_timezone(&Utc));
            Some((u32::try_from(left).unwrap_or(u32::MAX), ends_at))
        })
        .collect();
    Some(LimitsResetCreditsDto {
        available_count: grants
            .iter()
            .fold(0, |total: u32, (left, _)| total.saturating_add(*left)),
        next_expires_at: grants
            .iter()
            .filter(|(left, _)| *left > 0)
            .filter_map(|(_, ends_at)| *ends_at)
            .min()
            .map(|at| at.to_rfc3339()),
    })
}

fn legacy_windows(payload: &Value) -> Vec<LimitWindowDto> {
    const LEGACY: [(&str, &str, &str, LimitWindowKind); 4] = [
        (
            "five_hour",
            "session",
            "5 hour · all models",
            LimitWindowKind::Session,
        ),
        (
            "seven_day",
            "weekly_all",
            "Weekly · all models",
            LimitWindowKind::Weekly,
        ),
        (
            "seven_day_opus",
            "weekly_opus",
            "Weekly · Opus",
            LimitWindowKind::Model,
        ),
        (
            "seven_day_sonnet",
            "weekly_sonnet",
            "Weekly · Sonnet",
            LimitWindowKind::Model,
        ),
    ];
    LEGACY
        .iter()
        .filter_map(|(key, id, label, kind)| {
            let entry = payload.get(*key)?;
            let used = percent(entry.get("utilization"))?;
            let resets_at = optional_string(entry.get("resets_at"));
            Some(window(*id, *label, *kind, used, resets_at))
        })
        .collect()
}

#[cfg(test)]
mod tests;
