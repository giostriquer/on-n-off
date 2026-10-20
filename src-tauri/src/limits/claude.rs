use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::claude_cli;
use super::json::{humanize, optional_string};
use super::outcome::{Read, ReadFailure};
use super::window::{QuotaWindow, ResetInstant, Scope, FIVE_HOURS, ONE_WEEK};
use crate::accounts::claude::SignedIn;
use crate::dto::LimitsAccountDto;

pub(super) fn known_account(home: &Path) -> Option<LimitsAccountDto> {
    let claude = SignedIn::resolve(home).ok()?;
    Some(claude_cli::known_account(claude.config_file()))
}

pub(super) fn claude_current(home: &Path) -> Read {
    match SignedIn::resolve(home) {
        Ok(claude) => claude_cli::read_signed_in(
            &|dir| claude.command_in(dir),
            claude.config_file(),
            &usage_config_dir(home),
        ),
        Err(why) => Read::anonymous(ReadFailure::Unavailable(format!(
            "Could not find Claude Code's config: {why}"
        ))),
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
    let (scope, length) = match (group.as_str(), kind.as_str()) {
        ("session", _) => (Scope::AllModels, FIVE_HOURS),
        ("weekly", "weekly_all") => (Scope::AllModels, ONE_WEEK),
        ("weekly", other) => {
            let name = match &named {
                Some(name) => name.clone(),
                None => humanize(other.strip_prefix("weekly_").unwrap_or(other)),
            };
            (Scope::Model(name), ONE_WEEK)
        }
        _ => return None,
    };
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
    })
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
        ("five_hour", "session", None, FIVE_HOURS),
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
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
