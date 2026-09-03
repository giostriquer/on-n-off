use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::dto::{AgentId, AgentTabDto, McpServerDto, ProjectDto, SkillDto};
use crate::mcp::{parse_antigravity_json, parse_claude_json, parse_codex_map, CodexMcpEntry};
use crate::paths::normalize_skill_path;
use crate::scanner::scan_user_skills;
use crate::sort::sort_tab;

pub const ORIGIN_PROJECT: &str = "project";

pub fn normalize_project_key(path: &str) -> String {
    let mut key = path.replace('\\', "/");
    while key.len() > 1 && key.ends_with('/') {
        key.pop();
    }
    if cfg!(windows) {
        key.make_ascii_lowercase();
    }
    key
}

pub fn project_label(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    trimmed
        .rsplit(['\\', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

pub fn parse_claude_projects(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    value
        .get("projects")
        .and_then(|value| value.as_object())
        .map(|projects| projects.keys().cloned().collect())
        .unwrap_or_default()
}

pub fn parse_codex_projects(text: &str) -> Vec<String> {
    let Ok(value) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    value
        .get("projects")
        .and_then(|value| value.as_table())
        .map(|projects| projects.keys().cloned().collect())
        .unwrap_or_default()
}

pub fn expand_project_path(raw: &str) -> PathBuf {
    let raw = raw.trim();
    if let Some(rest) = raw
        .strip_prefix("~/")
        .or_else(|| raw.strip_prefix("~\\"))
        .or_else(|| (raw == "~").then_some(""))
    {
        if let Ok(home) = crate::paths::user_home() {
            return if rest.is_empty() {
                home
            } else {
                home.join(rest)
            };
        }
    }
    PathBuf::from(raw)
}

pub fn git_branch(root: &Path) -> String {
    let text = fs::read_to_string(root.join(".git").join("HEAD")).unwrap_or_default();
    text.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_string)
        .unwrap_or_default()
}

pub fn inspect_project(path: &Path, agent: AgentId) -> ProjectDto {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        expand_project_path(&path.to_string_lossy())
    };
    let display = resolved.to_string_lossy().to_string();
    let mut tab = AgentTabDto {
        plugins: vec![],
        user_skills: vec![],
        mcp_servers: vec![],
    };
    overlay_project(&mut tab, &resolved, agent);
    ProjectDto {
        id: normalize_project_key(&display),
        label: project_label(&display),
        path: display,
        branch: git_branch(&resolved),
        skill_count: tab.user_skills.len() as u32,
        mcp_count: tab.mcp_servers.len() as u32,
    }
}

pub fn inspect_projects(paths: Vec<String>, agent: AgentId) -> Vec<ProjectDto> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        let dto = inspect_project(Path::new(path.trim()), agent);
        if dto.path.trim().is_empty() || !seen.insert(dto.id.clone()) {
            continue;
        }
        out.push(dto);
    }
    out.sort_by(|a, b| {
        a.label
            .to_ascii_lowercase()
            .cmp(&b.label.to_ascii_lowercase())
            .then_with(|| {
                a.path
                    .to_ascii_lowercase()
                    .cmp(&b.path.to_ascii_lowercase())
            })
    });
    out
}

pub fn overlay_project(tab: &mut AgentTabDto, project: &Path, agent: AgentId) {
    if agent == AgentId::Antigravity {
        overlay_antigravity_plugins(tab, project);
    }

    let mut seen_skill_ids: std::collections::HashSet<String> = tab
        .user_skills
        .iter()
        .map(|skill| skill.id.clone())
        .collect();
    let mut seen_skill_names: std::collections::HashSet<String> = tab
        .user_skills
        .iter()
        .map(|skill| skill.name.to_ascii_lowercase())
        .chain(tab.plugins.iter().flat_map(|plugin| {
            plugin
                .skills
                .iter()
                .map(|skill| skill.name.to_ascii_lowercase())
        }))
        .collect();
    for dir in project_skill_dirs(project, agent) {
        for skill in scan_project_skills(&dir, agent) {
            let name_key = skill.name.to_ascii_lowercase();
            let id = format!(
                "project:{}",
                normalize_skill_path(&skill.skill_md.to_string_lossy())
            );
            if !seen_skill_ids.insert(id.clone()) || !seen_skill_names.insert(name_key) {
                continue;
            }
            tab.user_skills.push(SkillDto {
                id,
                plugin_id: None,
                name: skill.name,
                description: skill.description,
                enabled: true,
                togglable: false,
                origin: ORIGIN_PROJECT.to_string(),
            });
        }
    }

    let mut seen_mcp: std::collections::HashSet<String> = tab
        .mcp_servers
        .iter()
        .map(|server| server.id.clone())
        .collect();
    for mut server in project_mcp_servers(project, agent) {
        if !server.id.starts_with("project:") {
            server.id = format!("project:{}", server.id);
        }
        if !seen_mcp.insert(server.id.clone()) {
            continue;
        }
        tab.mcp_servers.push(server);
    }
    sort_tab(tab);
}

fn overlay_antigravity_plugins(tab: &mut AgentTabDto, project: &Path) {
    let mut seen: std::collections::HashSet<String> =
        tab.plugins.iter().map(|plugin| plugin.id.clone()).collect();
    for plugin in crate::antigravity::scan_workspace_plugins(project) {
        if seen.insert(plugin.id.clone()) {
            tab.plugins.push(plugin);
        }
    }
}

fn project_skill_dirs(project: &Path, agent: AgentId) -> Vec<PathBuf> {
    match agent {
        AgentId::Claude => vec![
            project.join(".claude").join("skills"),
            project.join(".agents").join("skills"),
        ],
        AgentId::Codex => vec![
            project.join(".codex").join("skills"),
            project.join(".agents").join("skills"),
        ],
        AgentId::Antigravity => vec![project.join(".agents").join("skills")],
        AgentId::Cursor => vec![project.join(".cursor").join("skills")],
    }
}

/// Where on-n-off writes project-scoped items for a provider: the first of its skill dirs,
/// plus `.claude/agents` for Claude subagents.
pub(crate) fn project_item_roots(project: &Path, agent: AgentId) -> crate::adapter::ItemRoots {
    let skills = project_skill_dirs(project, agent)
        .into_iter()
        .next()
        .unwrap_or_else(|| project.join(".agents").join("skills"));
    let agents = match agent {
        AgentId::Claude => Some(project.join(".claude").join("agents")),
        AgentId::Codex | AgentId::Antigravity | AgentId::Cursor => None,
    };
    crate::adapter::ItemRoots { skills, agents }
}

fn scan_project_skills(dir: &Path, agent: AgentId) -> Vec<crate::scanner::ScannedSkill> {
    match agent {
        AgentId::Antigravity => crate::scanner::scan_antigravity_skills(dir),
        AgentId::Claude | AgentId::Codex | AgentId::Cursor => scan_user_skills(dir),
    }
}

fn project_mcp_servers(project: &Path, agent: AgentId) -> Vec<McpServerDto> {
    let mut servers = Vec::new();
    let mcp_json = project.join(".mcp.json");
    if let Ok(text) = fs::read_to_string(&mcp_json) {
        servers.extend(as_project_mcp(parse_claude_json(&text)));
    }
    match agent {
        AgentId::Claude => {
            if let Ok(home) = crate::paths::user_home() {
                if let Ok(text) = fs::read_to_string(home.join(".claude.json")) {
                    servers.extend(as_project_mcp(parse_claude_project_mcp(&text, project)));
                }
            }
        }
        AgentId::Codex => {
            let config = project.join(".codex").join("config.toml");
            if let Ok(text) = fs::read_to_string(&config) {
                if let Ok(parsed) = toml::from_str::<ProjectCodexConfig>(&text) {
                    servers.extend(as_project_mcp(parse_codex_map(&parsed.mcp_servers)));
                }
            }
        }
        AgentId::Antigravity => {
            let config = project.join(".agents").join("mcp_config.json");
            if let Ok(text) = fs::read_to_string(&config) {
                servers.extend(as_project_mcp(parse_antigravity_json(&text)));
            }
        }
        AgentId::Cursor => {
            let config = project.join(".cursor").join("mcp.json");
            if let Ok(text) = fs::read_to_string(&config) {
                servers.extend(as_project_mcp(parse_antigravity_json(&text)));
            }
        }
    }
    servers
}

#[derive(Debug, Deserialize, Default)]
struct ProjectCodexConfig {
    #[serde(default)]
    mcp_servers: HashMap<String, CodexMcpEntry>,
}

pub fn parse_claude_project_mcp(text: &str, project: &Path) -> Vec<McpServerDto> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(projects) = value.get("projects").and_then(|value| value.as_object()) else {
        return Vec::new();
    };
    let key = normalize_project_key(&project.to_string_lossy());
    let Some((_, entry)) = projects
        .iter()
        .find(|(path, _)| normalize_project_key(path) == key)
    else {
        return Vec::new();
    };
    let Some(servers) = entry.get("mcpServers") else {
        return Vec::new();
    };
    let wrapped = serde_json::json!({ "mcpServers": servers });
    parse_claude_json(&wrapped.to_string())
}

fn as_project_mcp(servers: Vec<McpServerDto>) -> Vec<McpServerDto> {
    servers
        .into_iter()
        .map(|mut server| {
            server.togglable = false;
            server.origin = ORIGIN_PROJECT.to_string();
            server
        })
        .collect()
}

#[cfg(test)]
mod tests;
