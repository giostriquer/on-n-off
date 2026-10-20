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
    let scoped = entry.get("scope");
    let model = scoped.and_then(model_name);
    let named = model
        .clone()
        .or_else(|| optional_string(scoped.and_then(|scope| scope.get("surface"))));
    let (scope, length, role) = match (group.as_str(), kind.as_str()) {
        (SESSION, _) => (Scope::AllModels, FIVE_HOURS, None),
        ("weekly", "weekly_all") => (Scope::AllModels, ONE_WEEK, None),
        ("weekly", other) => {
            let name = named
                .clone()
                .unwrap_or_else(|| humanize(other.strip_prefix(WEEKLY_PREFIX).unwrap_or(other)));
            let model_key = if other == SCOPED_WEEKLY {
                model.as_deref()
            } else {
                other.strip_prefix(WEEKLY_PREFIX)
            };
            let role = model_key
                .is_some_and(|key| key.eq_ignore_ascii_case(INNER_RING_MODEL))
                .then_some(WindowRole::InnerRing);
            (Scope::Model(name), ONE_WEEK, role)
        }
        _ => return None,
    };
    let cycle_starts_on_first_message = group == SESSION;
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
        cycle_starts_on_first_message,
    })
}

fn model_name(scope: &Value) -> Option<String> {
    optional_string(
        scope
            .get("model")
            .and_then(|model| model.get("display_name")),
    )
}

fn reset(entry: &Value) -> Option<ResetInstant> {
    optional_string(entry.get("resets_at")).and_then(|text| ResetInstant::parse(&text))
}

struct LegacyWindow {
    key: &'static str,
    id: &'static str,
    model: Option<&'static str>,
    length: Duration,
    cycle_starts_on_first_message: bool,
}

const LEGACY: [LegacyWindow; 4] = [
    LegacyWindow {
        key: "five_hour",
        id: SESSION,
        model: None,
        length: FIVE_HOURS,
        cycle_starts_on_first_message: true,
    },
    LegacyWindow {
        key: "seven_day",
        id: "weekly_all",
        model: None,
        length: ONE_WEEK,
        cycle_starts_on_first_message: false,
    },
    LegacyWindow {
        key: "seven_day_opus",
        id: "weekly_opus",
        model: Some("Opus"),
        length: ONE_WEEK,
        cycle_starts_on_first_message: false,
    },
    LegacyWindow {
        key: "seven_day_sonnet",
        id: "weekly_sonnet",
        model: Some("Sonnet"),
        length: ONE_WEEK,
        cycle_starts_on_first_message: false,
    },
];

fn legacy_windows(payload: &Value) -> Vec<QuotaWindow> {
    LEGACY
        .iter()
        .filter_map(|legacy| {
            let entry = payload.get(legacy.key)?;
            Some(QuotaWindow {
                id: legacy.id.to_string(),
                scope: legacy
                    .model
                    .map_or(Scope::AllModels, |name| Scope::Model(name.to_string())),
                length: Some(legacy.length),
                used_percent: entry.get("utilization").and_then(Value::as_f64)?,
                reset: reset(entry),
                role: None,
                cycle_starts_on_first_message: legacy.cycle_starts_on_first_message,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
