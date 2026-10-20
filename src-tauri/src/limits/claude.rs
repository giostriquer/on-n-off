use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::claude_cli;
use super::json::{humanize, optional_string};
use super::pipeline::finish;
use super::window::{QuotaWindow, ResetInstant, Scope, FIVE_HOURS, ONE_WEEK};
use super::Parsed;
use crate::accounts::claude::SignedIn;
use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto, WindowRole};

const SESSION: &str = "session";
const WEEKLY_PREFIX: &str = "weekly_";
const SCOPED_WEEKLY: &str = "weekly_scoped";
const INNER_RING_MODEL: &str = "fable";

pub(super) fn claude_current(home: &Path) -> ProviderLimitsDto {
    match SignedIn::resolve(home) {
        Ok(claude) => claude_cli::read_signed_in(
            &|dir| claude.command_in(dir),
            claude.config_file(),
            &usage_config_dir(home),
        ),
        Err(why) => finish(
            AgentId::Claude,
            LimitsStatus::Failed,
            Some(format!("Could not find Claude Code's config: {why}")),
            Parsed::default(),
        ),
    }
}

pub(super) fn usage_config_dir(home: &Path) -> PathBuf {
    home.join(".on-n-off").join("claude-usage")
}

pub(super) fn parse_claude(payload: &Value) -> Vec<QuotaWindow> {
    let normalized: Vec<QuotaWindow> = payload
        .get("limits")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(normalized_window).collect())
        .unwrap_or_default();
    if !normalized.is_empty() {
        return normalized;
    }
    legacy_windows(payload)
}

fn normalized_window(entry: &Value) -> Option<QuotaWindow> {
    let kind = optional_string(entry.get("kind"))?;
    let group = optional_string(entry.get("group"))?;
    let used_percent = entry.get("percent").and_then(Value::as_f64)?;
    let named = scope_name(entry.get("scope"));
    let (scope, length, role) = match (group.as_str(), kind.as_str()) {
        (SESSION, _) => (Scope::AllModels, FIVE_HOURS, None),
        ("weekly", "weekly_all") => (Scope::AllModels, ONE_WEEK, None),
        ("weekly", other) => {
            let name = match &named {
                Some(name) => name.clone(),
                None => humanize(other.strip_prefix(WEEKLY_PREFIX).unwrap_or(other)),
            };
            let role = model_key(other, entry.get("scope"))
                .is_some_and(|model| model.eq_ignore_ascii_case(INNER_RING_MODEL))
                .then_some(WindowRole::InnerRing);
            (Scope::Model(name), ONE_WEEK, role)
        }
        _ => return None,
    };
    let starts_on_first_message = group == SESSION;
    let id = match &named {
        Some(name) => format!("{kind}:{name}"),
        None => kind,
    };
    Some(QuotaWindow {
        id,
        scope,
        length: Some(length),
        used_percent,
        reset: reset(entry),
        role,
        starts_on_first_message,
    })
}

fn model_key(kind: &str, scope: Option<&Value>) -> Option<String> {
    if kind == SCOPED_WEEKLY {
        return optional_string(
            scope?
                .get("model")
                .and_then(|model| model.get("display_name")),
        );
    }
    kind.strip_prefix(WEEKLY_PREFIX).map(str::to_string)
}

fn reset(entry: &Value) -> Option<ResetInstant> {
    optional_string(entry.get("resets_at")).and_then(|text| ResetInstant::parse(&text))
}

fn scope_name(scope: Option<&Value>) -> Option<String> {
    let scope = scope?;
    optional_string(
        scope
            .get("model")
            .and_then(|model| model.get("display_name")),
    )
    .or_else(|| optional_string(scope.get("surface")))
}

fn legacy_windows(payload: &Value) -> Vec<QuotaWindow> {
    const LEGACY: [(&str, &str, Option<&str>, Duration); 4] = [
        ("five_hour", SESSION, None, FIVE_HOURS),
        ("seven_day", "weekly_all", None, ONE_WEEK),
        ("seven_day_opus", "weekly_opus", Some("Opus"), ONE_WEEK),
        (
            "seven_day_sonnet",
            "weekly_sonnet",
            Some("Sonnet"),
            ONE_WEEK,
        ),
    ];
    LEGACY
        .iter()
        .filter_map(|(key, id, model, length)| {
            let entry = payload.get(*key)?;
            Some(QuotaWindow {
                id: (*id).to_string(),
                scope: model.map_or(Scope::AllModels, |name| Scope::Model(name.to_string())),
                length: Some(*length),
                used_percent: entry.get("utilization").and_then(Value::as_f64)?,
                reset: reset(entry),
                role: None,
                starts_on_first_message: *id == SESSION,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
