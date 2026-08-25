use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
}

const fn automatic_updates_default() -> bool {
    true
}

const fn limits_poll_minutes_default() -> u16 {
    10
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
    let Some(text) = json else {
        return AppSettings::default();
    };
    normalize_settings(serde_json::from_str(text).unwrap_or_default())
}

pub fn load_settings() -> AppSettings {
    let json = paths::settings_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok());
    parse_settings(json.as_deref())
}

pub fn save_settings(mut settings: AppSettings) -> Result<AppSettings, AdapterError> {
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
    let path = paths::settings_path()?;
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
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn unix_diagnostic_hints_do_not_talk_about_windows() {
        let hint = cli_hint(AgentId::Codex, "codex", None).expect("missing CLI gets a hint");
        for forbidden in ["Windows", "WSL", ".cmd", "Win32"] {
            assert!(!hint.contains(forbidden), "{hint}");
        }
        assert!(hint.contains("which codex"), "{hint}");
        assert_eq!(
            cli_hint(
                AgentId::Codex,
                "codex",
                Some(Path::new("/usr/local/bin/codex"))
            ),
            None
        );
        assert!(!home_missing_hint().contains("Windows"));
        assert!(!home_missing_hint().contains("WSL"));
        assert!(
            search_detail().starts_with("searched "),
            "{}",
            search_detail()
        );
    }

    #[test]
    fn parse_defaults_when_missing_or_malformed() {
        assert_eq!(parse_settings(None), AppSettings::default());
        assert_eq!(parse_settings(Some("{nope")), AppSettings::default());
    }

    #[test]
    fn saving_settings_replaces_the_complete_document() {
        let root = crate::paths::scratch_dir("settings-atomic-replace");
        let path = root.join("settings.json");
        let open_reader = root.join("open-reader.json");
        write_settings_document(&path, r#"{"generation":1}"#).unwrap();
        fs::hard_link(&path, &open_reader).unwrap();

        write_settings_document(&path, r#"{"generation":2}"#).unwrap();

        assert_eq!(
            fs::read_to_string(&open_reader).unwrap(),
            r#"{"generation":1}"#
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"generation":2}"#);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parse_hidden_and_binary_paths() {
        let settings = parse_settings(Some(
            r#"{ "hiddenAgents": ["antigravity"], "binaryPaths": { "claude": "C:\\bin\\claude.cmd" } }"#,
        ));
        assert_eq!(settings.hidden_agents, vec![AgentId::Antigravity]);
        assert_eq!(
            settings
                .binary_paths
                .get(&AgentId::Claude)
                .map(String::as_str),
            Some(r"C:\bin\claude.cmd")
        );
    }

    #[test]
    fn existing_settings_default_automatic_updates_to_enabled() {
        let new_settings = serde_json::to_value(parse_settings(None)).unwrap();
        let settings = parse_settings(Some(
            r#"{ "hiddenAgents": ["antigravity"], "binaryPaths": { "claude": "C:\\bin\\claude.cmd" } }"#,
        ));
        let serialized = serde_json::to_value(settings).unwrap();

        assert_eq!(new_settings["automaticUpdates"], true);
        assert_eq!(serialized["automaticUpdates"], true);
        assert_eq!(
            serialized["hiddenAgents"],
            serde_json::json!(["antigravity"])
        );
        assert_eq!(
            serialized["binaryPaths"]["claude"],
            serde_json::json!(r"C:\bin\claude.cmd")
        );
    }

    #[test]
    fn existing_automatic_update_opt_out_is_preserved() {
        let settings = parse_settings(Some(r#"{ "automaticUpdates": false }"#));
        let serialized = serde_json::to_value(settings).unwrap();

        assert_eq!(serialized["automaticUpdates"], false);
    }

    #[test]
    fn existing_settings_keep_limit_notifications_off_at_ten_minutes() {
        let serialized =
            serde_json::to_value(parse_settings(Some(r#"{ "hiddenAgents": ["codex"] }"#))).unwrap();

        assert_eq!(serialized["limitNotifications"], false);
        assert_eq!(serialized["limitsPollMinutes"], 10);
    }

    #[test]
    fn unsupported_limits_poll_interval_falls_back_to_ten_minutes() {
        let serialized = serde_json::to_value(parse_settings(Some(
            r#"{ "limitNotifications": true, "limitsPollMinutes": 7 }"#,
        )))
        .unwrap();

        assert_eq!(serialized["limitNotifications"], true);
        assert_eq!(serialized["limitsPollMinutes"], 10);
    }

    #[test]
    fn existing_settings_default_the_github_screen_off_at_sixty_seconds() {
        let serialized =
            serde_json::to_value(parse_settings(Some(r#"{ "hiddenAgents": ["codex"] }"#))).unwrap();

        assert_eq!(serialized["githubScopes"], serde_json::json!([]));
        assert_eq!(serialized["githubNotifications"], false);
        assert_eq!(serialized["githubPollSeconds"], 60);
    }

    #[test]
    fn unsupported_github_poll_interval_falls_back_to_sixty_seconds() {
        let serialized = serde_json::to_value(parse_settings(Some(
            r#"{ "githubNotifications": true, "githubPollSeconds": 45 }"#,
        )))
        .unwrap();

        assert_eq!(serialized["githubNotifications"], true);
        assert_eq!(serialized["githubPollSeconds"], 60);
    }

    #[test]
    fn github_scopes_are_normalised_to_search_qualifiers() {
        assert_eq!(
            normalize_github_scope(" foo/bar "),
            Some("repo:foo/bar".to_string())
        );
        assert_eq!(
            normalize_github_scope("repo:foo/bar.js"),
            Some("repo:foo/bar.js".to_string())
        );
        assert_eq!(
            normalize_github_scope("org:acme"),
            Some("org:acme".to_string())
        );
        assert_eq!(
            normalize_github_scope("user:me-1"),
            Some("user:me-1".to_string())
        );
        assert_eq!(
            normalize_github_scope("ORG:Acme"),
            Some("org:Acme".to_string())
        );
        for invalid in [
            "",
            "   ",
            "x",
            "org: x",
            "repo:o",
            "repo:o/r/x",
            "team:acme/core",
            "org:a b",
        ] {
            assert_eq!(normalize_github_scope(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn loading_settings_drops_malformed_github_scopes_and_normalises_the_rest() {
        let settings = parse_settings(Some(
            r#"{ "githubScopes": ["acme/app", "org: broken", "user:me", "user:me"] }"#,
        ));

        assert_eq!(
            settings.github_scopes,
            vec!["repo:acme/app".to_string(), "user:me".to_string()]
        );
    }

    #[test]
    fn saving_settings_refuses_a_malformed_github_scope_and_names_it() {
        let err = save_settings(AppSettings {
            github_scopes: vec!["org:acme".into(), "org: broken".into()],
            ..AppSettings::default()
        })
        .unwrap_err();

        assert!(err.message.contains("org: broken"), "{}", err.message);
        assert!(err.message.contains("org:NAME"), "{}", err.message);
    }

    #[test]
    fn cursor_uses_agent_as_its_command_and_keeps_the_legacy_alias() {
        assert_eq!(AgentId::Cursor.binary_name(), "agent");
        assert_eq!(agent_for_binary("agent"), Some(AgentId::Cursor));
        assert_eq!(agent_for_binary("cursor-agent"), Some(AgentId::Cursor));
        assert_eq!(agent_for_binary("cursor-agent.cmd"), Some(AgentId::Cursor));
        assert_eq!(
            agent_for_binary("cursor"),
            None,
            "the editor launcher is not the CLI"
        );
    }

    #[test]
    fn missing_cursor_cli_hint_explains_the_agent_name_clash() {
        let hint = cli_hint(AgentId::Cursor, "agent", None).expect("hint");
        assert!(hint.contains("cursor-agent"), "{hint}");
        assert!(hint.contains("another product"), "{hint}");
        let other = cli_hint(AgentId::Codex, "codex", None).expect("hint");
        assert!(!other.contains("another product"), "{other}");
    }

    #[test]
    fn refuses_hiding_every_provider() {
        let err = save_settings(AppSettings {
            hidden_agents: vec![
                AgentId::Claude,
                AgentId::Codex,
                AgentId::Antigravity,
                AgentId::Cursor,
            ],
            ..AppSettings::default()
        })
        .unwrap_err();
        assert!(err.message.contains("at least one provider"));
    }
}
