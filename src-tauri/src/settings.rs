use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::dto::{AdapterError, AgentId};
use crate::paths::{self, well_known_cli_dirs};

const ALL_AGENTS: [AgentId; 3] = [AgentId::Claude, AgentId::Codex, AgentId::Antigravity];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub hidden_agents: Vec<AgentId>,
    #[serde(default)]
    pub binary_paths: HashMap<AgentId, String>,
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
    serde_json::from_str(text).unwrap_or_default()
}

pub fn load_settings() -> AppSettings {
    let json = paths::settings_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok());
    parse_settings(json.as_deref())
}

pub fn save_settings(mut settings: AppSettings) -> Result<AppSettings, AdapterError> {
    settings.binary_paths.retain(|_, value| !value.trim().is_empty());
    settings.hidden_agents.retain(|id| ALL_AGENTS.contains(id));
    settings.hidden_agents.sort_by_key(|id| match id {
        AgentId::Claude => 0,
        AgentId::Codex => 1,
        AgentId::Antigravity => 2,
    });
    settings.hidden_agents.dedup();
    if hidden_covers_all(&settings.hidden_agents) {
        return Err(AdapterError::message(
            "Keep at least one provider visible in the agent tabs.",
        ));
    }
    let path = paths::settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| AdapterError::write(error.to_string(), Some(path.display().to_string())))?;
    }
    let body = serde_json::to_string_pretty(&settings)
        .map_err(|error| AdapterError::message(error.to_string()))?;
    fs::write(&path, body).map_err(|error| AdapterError::write(error.to_string(), Some(path.display().to_string())))?;
    Ok(settings)
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
        _ => None,
    }
}

fn binary_name(id: AgentId) -> &'static str {
    match id {
        AgentId::Claude => "claude",
        AgentId::Codex => "codex",
        AgentId::Antigravity => "agy",
    }
}

fn home_for(id: AgentId) -> Result<PathBuf, AdapterError> {
    match id {
        AgentId::Claude => paths::claude_root(),
        AgentId::Codex => paths::codex_root(),
        AgentId::Antigravity => paths::gemini_root(),
    }
}

pub fn diagnose_provider(id: AgentId) -> ProviderDiagnose {
    let settings = load_settings();
    let binary = binary_name(id);
    let override_path = settings
        .binary_paths
        .get(&id)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let resolved = paths::resolve_cli_binary(binary);
    let home = home_for(id).ok();
    let home_exists = home.as_ref().is_some_and(|path| path.is_dir());
    let extra = well_known_cli_dirs();

    let cli_detail = match (&override_path, &resolved) {
        (Some(_), Some(found)) => format!("using {}", found.display()),
        (Some(path), None) => format!("override missing · {}", path.display()),
        (None, Some(found)) => found.display().to_string(),
        (None, None) => format!("{binary} is not on PATH"),
    };
    let cli_hint = if resolved.is_none() {
        Some(format!(
            "If `{binary}` works in a terminal, point Binary at the .cmd/.exe next to it (nvm shims are not Win32 programs), or install the Windows CLI — not WSL-only."
        ))
    } else if cfg!(windows)
        && resolved
            .as_ref()
            .is_some_and(|path| path.extension().is_none())
    {
        Some("This path has no .cmd/.exe. Windows will fail with os error 193. Pick the .cmd launcher.".into())
    } else {
        None
    };

    let extra_hit = paths::find_in_dirs(binary, &extra).is_some();
    let search_detail = if extra.is_empty() {
        "no extra install dirs".into()
    } else {
        format!(
            "checked {} well-known install folders{}",
            extra.len(),
            if extra_hit { " · found a copy" } else { "" }
        )
    };

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
                    Some("Expected under your Windows user profile, not inside WSL.".into())
                },
            },
            DiagnoseCheck {
                id: "search".into(),
                label: "Install search".into(),
                ok: resolved.is_some() || extra_hit,
                detail: search_detail,
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

    #[test]
    fn parse_defaults_when_missing_or_malformed() {
        assert_eq!(parse_settings(None), AppSettings::default());
        assert_eq!(parse_settings(Some("{nope")), AppSettings::default());
    }

    #[test]
    fn parse_hidden_and_binary_paths() {
        let settings = parse_settings(Some(
            r#"{ "hiddenAgents": ["antigravity"], "binaryPaths": { "claude": "C:\\bin\\claude.cmd" } }"#,
        ));
        assert_eq!(settings.hidden_agents, vec![AgentId::Antigravity]);
        assert_eq!(
            settings.binary_paths.get(&AgentId::Claude).map(String::as_str),
            Some(r"C:\bin\claude.cmd")
        );
    }

    #[test]
    fn refuses_hiding_every_provider() {
        let err = save_settings(AppSettings {
            hidden_agents: vec![AgentId::Claude, AgentId::Codex, AgentId::Antigravity],
            binary_paths: HashMap::new(),
        })
        .unwrap_err();
        assert!(err.message.contains("at least one provider"));
    }
}
