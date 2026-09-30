use std::collections::HashMap;
use std::fs;
use std::hash::Hash;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::cli_locate::{
    cli_search_path, login_shell_path_dirs, registered_path_dirs, resolve_provider_cli,
};
use crate::dto::{AdapterError, AgentId};
use crate::paths;

const ALL_AGENTS: [AgentId; 4] = [
    AgentId::Claude,
    AgentId::Codex,
    AgentId::Antigravity,
    AgentId::Cursor,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub hidden_agents: Vec<AgentId>,
    #[serde(default)]
    pub binary_paths: HashMap<AgentId, String>,
    #[serde(default = "automatic_updates_default")]
    pub automatic_updates: bool,
    #[serde(default)]
    pub limit_notifications: bool,
    #[serde(default = "limits_poll_minutes_default")]
    pub limits_poll_minutes: u16,
    /// Search qualifiers (`org:NAME`, `user:NAME`, `repo:OWNER/NAME`) that narrow the GitHub
    /// screen's "Mine" list; empty means no filter.
    #[serde(default)]
    pub github_scopes: Vec<String>,
    #[serde(default)]
    pub github_notifications: bool,
    #[serde(default = "github_poll_seconds_default")]
    pub github_poll_seconds: u16,
    /// Windows only: closing the main window hides it and leaves the app in the tray.
    #[serde(default)]
    pub close_to_tray: bool,
    /// Codex accounts whose banked reset on-n-off offers once they run low, by the card's account
    /// id: an opt-in each, which spends the reset only when it says so (`ResetAlert::automatic`).
    #[serde(default)]
    pub reset_alerts: HashMap<String, ResetAlert>,
}

/// When a Codex account's banked reset is offered: with `max_left_percent` or less of its current
/// limit left, and its own reset at least `min_hours_to_renewal` away, since a reset spent just
/// before the limit renews anyway is wasted. An `automatic` alert then spends the reset itself,
/// after a wait the user can cancel it in (`limits_monitor::auto_spend`); otherwise it only tells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetAlert {
    /// The account's email when it was turned on, for Settings to name it.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "reset_max_left_default")]
    pub max_left_percent: u8,
    #[serde(default = "reset_min_hours_default")]
    pub min_hours_to_renewal: u16,
    #[serde(default)]
    pub automatic: bool,
}

/// The share of the current limit left at or under which Codex's own app lets a reset be used.
pub const CODEX_RESET_MAX_LEFT_PERCENT: u8 = 10;

/// The longest wait an alert can ask for before the limit renews by itself: a week, the cycle.
pub const RESET_ALERT_MAX_HOURS: u16 = 7 * 24;

impl ResetAlert {
    /// The share of the current limit left at or under which this account's banked reset may be
    /// spent: Codex's own 10%, or the lower share the alert names.
    pub fn spend_limit(&self) -> u8 {
        self.max_left_percent.min(CODEX_RESET_MAX_LEFT_PERCENT)
    }
}

const fn reset_max_left_default() -> u8 {
    CODEX_RESET_MAX_LEFT_PERCENT
}

const fn reset_min_hours_default() -> u16 {
    24
}

/// The share of the current limit left at or under which a banked reset of `account_id` may be
/// spent (`ResetAlert::spend_limit`), Codex's own 10% without an alert.
pub fn reset_spend_limit(settings: &AppSettings, account_id: &str) -> u8 {
    settings
        .reset_alerts
        .get(account_id)
        .map_or(CODEX_RESET_MAX_LEFT_PERCENT, ResetAlert::spend_limit)
}

const fn automatic_updates_default() -> bool {
    true
}

const fn limits_poll_minutes_default() -> u16 {
    5
}

const fn github_poll_seconds_default() -> u16 {
    60
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hidden_agents: Vec::new(),
            binary_paths: HashMap::new(),
            automatic_updates: automatic_updates_default(),
            limit_notifications: false,
            limits_poll_minutes: limits_poll_minutes_default(),
            github_scopes: Vec::new(),
            github_notifications: false,
            github_poll_seconds: github_poll_seconds_default(),
            close_to_tray: false,
            reset_alerts: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnoseCheck {
    pub id: String,
    pub label: String,
    pub ok: bool,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDiagnose {
    pub agent_id: AgentId,
    pub binary: String,
    pub home_path: String,
    pub checks: Vec<DiagnoseCheck>,
}

pub fn parse_settings(json: Option<&str>) -> AppSettings {
    // An editor that marks a file's encoding starts it with a byte-order mark, which JSON does not
    // allow. A document that is not a JSON object has no settings to keep.
    json.and_then(|text| {
        serde_json::from_str::<Map<String, Value>>(text.trim_start_matches('\u{feff}')).ok()
    })
    .map_or_else(AppSettings::default, |document| {
        normalize_settings(read_settings(&document))
    })
}

/// Settings read from a document one field at a time. A hand-edited file can hold a value the app
/// cannot read, such as a number beyond its type or a provider it does not know: that value takes
/// its default, an item or entry of a list or map that cannot be read is dropped, and every other
/// setting is kept.
fn read_settings(document: &Map<String, Value>) -> AppSettings {
    let defaults = AppSettings::default();
    let field = |key: &str| document.get(key).unwrap_or(&Value::Null);
    AppSettings {
        hidden_agents: items(field("hiddenAgents")),
        binary_paths: entries(field("binaryPaths"), read),
        automatic_updates: read(field("automaticUpdates")).unwrap_or(defaults.automatic_updates),
        limit_notifications: read(field("limitNotifications"))
            .unwrap_or(defaults.limit_notifications),
        limits_poll_minutes: read(field("limitsPollMinutes"))
            .unwrap_or(defaults.limits_poll_minutes),
        github_scopes: items(field("githubScopes")),
        github_notifications: read(field("githubNotifications"))
            .unwrap_or(defaults.github_notifications),
        github_poll_seconds: read(field("githubPollSeconds"))
            .unwrap_or(defaults.github_poll_seconds),
        close_to_tray: read(field("closeToTray")).unwrap_or(defaults.close_to_tray),
        reset_alerts: entries(field("resetAlerts"), read_reset_alert),
    }
}

/// One banked reset alert, or none when the entry is not an object. A figure is rounded and held
/// within its type, a negative one at 0, so one out of range still meets the alert's rule in
/// [`normalize_settings`].
fn read_reset_alert(alert: &Value) -> Option<ResetAlert> {
    let alert = alert.as_object()?;
    // A float's `as` cast saturates at the target type's bounds.
    let figure = |key: &str| alert.get(key).and_then(Value::as_f64).map(f64::round);
    Some(ResetAlert {
        label: alert.get("label").and_then(read),
        max_left_percent: figure("maxLeftPercent")
            .map_or_else(reset_max_left_default, |percent| percent as u8),
        min_hours_to_renewal: figure("minHoursToRenewal")
            .map_or_else(reset_min_hours_default, |hours| hours as u16),
        automatic: alert.get("automatic").and_then(read).unwrap_or(false),
    })
}

fn read<T: DeserializeOwned>(value: &Value) -> Option<T> {
    T::deserialize(value).ok()
}

/// The items of a list that can be read, in order.
fn items<T: DeserializeOwned>(list: &Value) -> Vec<T> {
    list.as_array()
        .map(|list| list.iter().filter_map(read).collect())
        .unwrap_or_default()
}

/// The entries of a map whose key and value can both be read.
fn entries<K: DeserializeOwned + Eq + Hash, V>(
    map: &Value,
    read_value: impl Fn(&Value) -> Option<V>,
) -> HashMap<K, V> {
    map.as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    Some((read(&Value::String(key.clone()))?, read_value(value)?))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn load_settings() -> AppSettings {
    let json = paths::settings_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok());
    parse_settings(json.as_deref())
}

pub fn save_settings(settings: AppSettings) -> Result<AppSettings, AdapterError> {
    save_settings_to(settings, paths::settings_path)
}

/// [`save_settings`] into the document `path` names. It validates before it resolves the path or
/// writes anything, so a refused save touches no file; tests save through this into a disposable
/// home, never the real settings document, even while the refusal they check is broken.
fn save_settings_to(
    mut settings: AppSettings,
    path: impl FnOnce() -> Result<PathBuf, AdapterError>,
) -> Result<AppSettings, AdapterError> {
    if let Some(bad) = settings
        .github_scopes
        .iter()
        .find(|scope| normalize_github_scope(scope).is_none())
    {
        return Err(AdapterError::message(format!(
            "Unrecognised GitHub scope {bad:?} — use org:NAME, user:NAME or OWNER/REPO."
        )));
    }
    settings = normalize_settings(settings);
    settings
        .binary_paths
        .retain(|_, value| !value.trim().is_empty());
    settings.hidden_agents.retain(|id| ALL_AGENTS.contains(id));
    settings.hidden_agents.sort_by_key(|id| match id {
        AgentId::Claude => 0,
        AgentId::Codex => 1,
        AgentId::Antigravity => 2,
        AgentId::Cursor => 3,
    });
    settings.hidden_agents.dedup();
    if hidden_covers_all(&settings.hidden_agents) {
        return Err(AdapterError::message(
            "Keep at least one provider visible in the agent tabs.",
        ));
    }
    let path = path()?;
    let body = serde_json::to_string_pretty(&settings)
        .map_err(|error| AdapterError::message(error.to_string()))?;
    write_settings_document(&path, &body)?;
    Ok(settings)
}

fn write_settings_document(path: &Path, body: &str) -> Result<(), AdapterError> {
    crate::usage::cache_io::atomic_write(path, body)
        .map_err(|error| AdapterError::write(error.to_string(), Some(path.display().to_string())))
}

fn normalize_settings(mut settings: AppSettings) -> AppSettings {
    if !matches!(settings.limits_poll_minutes, 5 | 10 | 15 | 30) {
        settings.limits_poll_minutes = limits_poll_minutes_default();
    }
    if !matches!(settings.github_poll_seconds, 30 | 60 | 120 | 300) {
        settings.github_poll_seconds = github_poll_seconds_default();
    }
    let mut scopes: Vec<String> = settings
        .github_scopes
        .iter()
        .filter_map(|scope| normalize_github_scope(scope))
        .collect();
    let mut seen = std::collections::HashSet::new();
    scopes.retain(|scope| seen.insert(scope.clone()));
    settings.github_scopes = scopes;
    for alert in settings.reset_alerts.values_mut() {
        alert.max_left_percent = alert
            .max_left_percent
            .clamp(1, CODEX_RESET_MAX_LEFT_PERCENT);
        alert.min_hours_to_renewal = alert.min_hours_to_renewal.min(RESET_ALERT_MAX_HOURS);
    }
    settings
}

/// One GitHub scope as the search qualifier it stands for: `org:NAME`, `user:NAME`,
/// `repo:OWNER/NAME`, or a bare `OWNER/NAME` (which becomes `repo:`). Anything else is `None`.
pub fn normalize_github_scope(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().any(char::is_whitespace) {
        return None;
    }
    let (kind, value) = match raw.split_once(':') {
        Some((kind, value)) => (kind.to_ascii_lowercase(), value),
        None => ("repo".to_string(), raw),
    };
    let valid = match kind.as_str() {
        "org" | "user" => is_github_login(value),
        "repo" => value
            .split_once('/')
            .is_some_and(|(owner, name)| is_github_login(owner) && is_github_repo_name(name)),
        _ => false,
    };
    valid.then(|| format!("{kind}:{value}"))
}

fn is_github_login(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_github_repo_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn hidden_covers_all(hidden: &[AgentId]) -> bool {
    ALL_AGENTS.iter().all(|id| hidden.contains(id))
}

pub fn binary_override_for(cli_name: &str) -> Option<PathBuf> {
    let agent = agent_for_binary(cli_name)?;
    let raw = load_settings().binary_paths.get(&agent)?.trim().to_string();
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

fn agent_for_binary(cli_name: &str) -> Option<AgentId> {
    match PathBuf::from(cli_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(cli_name)
        .to_ascii_lowercase()
        .as_str()
    {
        "claude" => Some(AgentId::Claude),
        "codex" => Some(AgentId::Codex),
        "agy" => Some(AgentId::Antigravity),
        "agent" | "cursor-agent" => Some(AgentId::Cursor),
        _ => None,
    }
}

fn home_for(id: AgentId) -> Result<PathBuf, AdapterError> {
    match id {
        AgentId::Claude => paths::claude_root(),
        AgentId::Codex => paths::codex_root(),
        AgentId::Antigravity => paths::gemini_root(),
        AgentId::Cursor => paths::cursor_root(),
    }
}

/// Why a CLI check may fail and what to do about it, per platform and provider.
fn cli_hint(id: AgentId, binary: &str, resolved: Option<&Path>) -> Option<String> {
    match resolved {
        None if id == AgentId::Cursor => Some(cursor_missing_hint()),
        None if cfg!(windows) => Some(format!(
            "If `{binary}` works in a terminal, point Binary at the .cmd/.exe next to it (nvm shims are not Win32 programs), or install the Windows CLI — not WSL-only."
        )),
        None => Some(format!(
            "If `{binary}` works in a terminal, run `which {binary}` there and paste that path into Binary."
        )),
        Some(path) if cfg!(windows) && path.extension().is_none() => Some(
            "This path has no .cmd/.exe. Windows will fail with os error 193. Pick the .cmd launcher.".into(),
        ),
        Some(_) => None,
    }
}

/// Cursor's CLI shares its `agent` name with other products, so only a launcher inside a
/// `cursor-agent` install folder (or the legacy `cursor-agent` alias) is accepted.
fn cursor_missing_hint() -> String {
    let install = if cfg!(windows) {
        r"%LOCALAPPDATA%\cursor-agent (irm 'https://cursor.com/install?win32=true' | iex)"
    } else {
        "~/.local/bin, linked into ~/.local/share/cursor-agent (curl https://cursor.com/install -fsS | bash)"
    };
    format!(
        "Cursor's CLI installs `agent` under {install}. An `agent` command from another product is not accepted; if Cursor's launcher lives elsewhere, point Binary at it (`cursor-agent` also works)."
    )
}

fn home_missing_hint() -> &'static str {
    if cfg!(windows) {
        "Expected under your Windows user profile, not inside WSL."
    } else {
        "Expected in your home folder (~). Run the CLI once so it creates its config."
    }
}

/// One line describing where CLIs are looked for, including what the login shell contributed.
fn search_detail() -> String {
    let searched = cli_search_path().len();
    let from_shell = login_shell_path_dirs().len();
    if cfg!(windows) {
        let registered = registered_path_dirs().len();
        format!(
            "searched {searched} folders (PATH, {registered} from the registered user/machine PATH, and well-known install folders)"
        )
    } else if from_shell == 0 {
        format!("searched {searched} folders · login shell PATH unavailable, using well-known install folders")
    } else {
        format!("searched {searched} folders · {from_shell} from your login shell PATH")
    }
}

pub fn diagnose_provider(id: AgentId) -> ProviderDiagnose {
    let settings = load_settings();
    let binary = id.binary_name();
    let override_path = settings
        .binary_paths
        .get(&id)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let resolved = resolve_provider_cli(id, binary);
    let home = home_for(id).ok();
    let home_exists = home.as_ref().is_some_and(|path| path.is_dir());

    let cli_detail = match (&override_path, &resolved) {
        (Some(_), Some(found)) => format!("using {}", found.display()),
        (Some(path), None) => format!("override missing · {}", path.display()),
        (None, Some(found)) => found.display().to_string(),
        (None, None) => format!("{binary} is not on the CLI search path"),
    };
    let cli_hint = cli_hint(id, binary, resolved.as_deref());

    ProviderDiagnose {
        agent_id: id,
        binary: binary.into(),
        home_path: home
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "(home not found)".into()),
        checks: vec![
            DiagnoseCheck {
                id: "cli".into(),
                label: "CLI binary".into(),
                ok: resolved.is_some(),
                detail: cli_detail,
                hint: cli_hint,
            },
            DiagnoseCheck {
                id: "home".into(),
                label: "Config folder".into(),
                ok: home_exists,
                detail: home
                    .as_ref()
                    .map(|path| {
                        if home_exists {
                            path.display().to_string()
                        } else {
                            format!("missing · {}", path.display())
                        }
                    })
                    .unwrap_or_else(|| "home directory not found".into()),
                hint: if home_exists {
                    None
                } else {
                    Some(home_missing_hint().into())
                },
            },
            DiagnoseCheck {
                id: "search".into(),
                label: "Install search".into(),
                ok: resolved.is_some(),
                detail: search_detail(),
                hint: None,
            },
        ],
    }
}

pub fn diagnose_all() -> Vec<ProviderDiagnose> {
    ALL_AGENTS.into_iter().map(diagnose_provider).collect()
}

#[cfg(test)]
mod tests;
