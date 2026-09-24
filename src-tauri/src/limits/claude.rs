//! Claude subscription limits: parse `GET api.anthropic.com/api/oauth/usage` (pure).
//!
//! The payload carries a normalized `limits[]` array (kind/group/percent/resets_at) plus the
//! older top-level `five_hour` / `seven_day` / `seven_day_<model>` objects. The array wins when
//! it has usable entries; the legacy keys are the fallback. Asked with `?cedar_ember=1`, the same
//! payload also reports the account's saved resets.

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::credentials::ClaudeIdentity;
use super::json::{humanize, optional_string, percent, window};
use super::Parsed;
use crate::dto::{LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsResetCreditsDto};

/// What `GET /api/oauth/profile` says about the login: whose it is, and the subscription status
/// Anthropic reports for its organization. The status never decides the read.
pub(super) struct ClaudeProfile {
    pub(super) identity: ClaudeIdentity,
    /// `organization.subscription_status` as Anthropic writes it (`active`, `past_due`, …);
    /// unknown when absent, empty or not text.
    pub(super) subscription_status: Option<String>,
}

pub(super) fn parse_profile(payload: &Value) -> Result<ClaudeProfile, String> {
    let account = payload
        .get("account")
        .ok_or_else(|| "missing account".to_string())?;
    let organization = payload
        .get("organization")
        .ok_or_else(|| "missing organization".to_string())?;
    Ok(ClaudeProfile {
        identity: ClaudeIdentity {
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
        },
        subscription_status: optional_string(organization.get("subscription_status")),
    })
}

/// Everything one usage payload says about the account: its windows and its saved resets.
pub(super) fn parse_usage(payload: &Value, now: DateTime<Utc>) -> Parsed {
    Parsed {
        windows: parse_claude(payload),
        reset_credits: parse_reset_credits(payload, now),
        ..Parsed::default()
    }
}

fn parse_claude(payload: &Value) -> Vec<LimitWindowDto> {
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

/// Why Claude Code's saved-reset status can report no reset for reasons about the account or the
/// program itself, so no grants really means none. Any other reason (`surface`, `cli_version`,
/// `mobile`, `unknown`, or one added later) describes who is asking; on-n-off is not Claude Code, so
/// those leave the count unknown.
const ACCOUNT_HOLDS_NONE: [&str; 6] = [
    "no_grant",
    "tier",
    "seat",
    "tenure",
    "other_experiment",
    "config_off",
];
/// The status could not be read at all; Claude Code treats it as a failed read.
const UNANSWERED: &str = "unavailable";

/// Anthropic's saved rate-limit resets, which Claude Code spends with `/limit-reset`: the
/// `cedar_ember` block the usage read carries when asked with `?cedar_ember=1`. The count sums
/// `resets_left` over every grant, as Claude Code's own count does, and the expiry is the soonest
/// `ends_at` still ahead of `now` among grants holding a reset.
///
/// `None` is "unknown", never zero: no block, an `unavailable` one, grants that were sent but
/// cannot be read, or no grants for a reason about the asker. Zero is only reported when the block
/// says the account holds none.
fn parse_reset_credits(payload: &Value, now: DateTime<Utc>) -> Option<LimitsResetCreditsDto> {
    let block = payload.get("cedar_ember")?.as_object()?;
    let reason = block.get("ineligible_reason").and_then(Value::as_str);
    if reason == Some(UNANSWERED) {
        return None;
    }
    let sent: &[Value] = match block.get("grants") {
        None | Some(Value::Null) => &[],
        Some(grants) => grants.as_array()?,
    };
    let grants: Vec<(u32, Option<DateTime<Utc>>)> = sent.iter().filter_map(grant).collect();
    if grants.is_empty() {
        let holds_none = sent.is_empty()
            && (block.get("eligible").and_then(Value::as_bool) == Some(true)
                || reason.is_some_and(|reason| ACCOUNT_HOLDS_NONE.contains(&reason)));
        return holds_none.then_some(LimitsResetCreditsDto {
            available_count: 0,
            next_expires_at: None,
        });
    }
    Some(LimitsResetCreditsDto {
        available_count: grants
            .iter()
            .map(|(left, _)| *left)
            .fold(0, u32::saturating_add),
        next_expires_at: grants
            .iter()
            .filter(|(left, _)| *left > 0)
            .filter_map(|(_, ends_at)| *ends_at)
            .filter(|ends_at| *ends_at > now)
            .min()
            .map(|at| at.to_rfc3339()),
    })
}

/// One grant's `resets_left` and `ends_at`. The count must be a whole number of at least zero, as
/// Claude Code's reader requires; `1.0` is one, since JSON does not tell integers from floats.
fn grant(grant: &Value) -> Option<(u32, Option<DateTime<Utc>>)> {
    let left = grant.get("resets_left")?;
    let left = left.as_u64().or_else(|| {
        left.as_f64()
            .filter(|left| left.fract() == 0.0 && *left >= 0.0)
            .map(|left| left as u64)
    })?;
    let ends_at = optional_string(grant.get("ends_at"))
        .and_then(|at| DateTime::parse_from_rfc3339(&at).ok())
        .map(|at| at.with_timezone(&Utc));
    Some((u32::try_from(left).unwrap_or(u32::MAX), ends_at))
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
