//! Claude subscription limits: the signed-in account's card, which Claude Code reports for the
//! user's own config dir (`limits::claude_cli`), and the parse of the windows in a usage report.
//! on-n-off sends no request with a Claude login and reads no Claude credential for Limits.
//!
//! A usage report carries a normalized `limits[]` array (kind/group/percent/resets_at) plus, in
//! older answers, the top-level `five_hour` / `seven_day` / `seven_day_<model>` objects. The array
//! wins when it has usable entries; the legacy keys are the fallback.

use std::path::Path;

use serde_json::Value;

use super::claude_cli;
use super::json::{humanize, optional_string, percent, window};
use super::pipeline::finish;
use super::Parsed;
use crate::accounts::claude::SignedIn;
use crate::dto::{AgentId, LimitWindowDto, LimitWindowKind, LimitsStatus, ProviderLimitsDto};

/// The signed-in Claude account's card: Claude Code's own usage report for the user's config dir
/// under `home`, where the environment puts it.
pub(super) fn claude_current(home: &Path) -> ProviderLimitsDto {
    match SignedIn::resolve(home) {
        Ok(claude) => claude_cli::read_signed_in(&|| claude.command(), claude.config_file()),
        Err(why) => finish(
            AgentId::Claude,
            LimitsStatus::Failed,
            Some(format!("Could not find Claude Code's config: {why}")),
            Parsed::default(),
        ),
    }
}

pub(super) fn parse_claude(payload: &Value) -> Vec<LimitWindowDto> {
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
