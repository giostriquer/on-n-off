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

#[cfg(test)]
pub fn projects_from_paths(paths: Vec<String>) -> Vec<ProjectDto> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            continue;
        }
        let id = normalize_project_key(trimmed);
        if !seen.insert(id.clone()) {
            continue;
        }
        out.push(ProjectDto {
            id,
            label: project_label(trimmed),
            path: trimmed.to_string(),
            branch: String::new(),
            skill_count: 0,
            mcp_count: 0,
        });
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
mod tests {
    use super::*;

    #[test]
    fn normalizes_windows_project_keys() {
        assert_eq!(
            normalize_project_key(r"E:\dev\on-n-off\"),
            if cfg!(windows) {
                "e:/dev/on-n-off"
            } else {
                "E:/dev/on-n-off"
            }
        );
        assert_eq!(project_label(r"E:\dev\on-n-off"), "on-n-off");
        assert_eq!(project_label(r"E:\dev\"), "dev");
    }

    #[test]
    fn parses_claude_and_codex_recognized_projects() {
        let claude = parse_claude_projects(
            r#"{ "projects": { "E:/dev/on-n-off": {}, "C:/tmp/app": { "mcpServers": {} } } }"#,
        );
        assert_eq!(claude.len(), 2);
        let codex = parse_codex_projects(
            "[projects.'E:\\dev\\on-n-off']\ntrust_level = \"trusted\"\n\n[projects.'E:\\dev\\conoswiki']\ntrust_level = \"trusted\"\n",
        );
        assert!(codex.iter().any(|path| path.contains("on-n-off")));
        assert!(codex.iter().any(|path| path.contains("conoswiki")));
        let projects = projects_from_paths(vec![
            r"E:\dev\on-n-off".into(),
            r"E:\dev\on-n-off\".into(),
            r"E:\dev\conoswiki".into(),
        ]);
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].label, "conoswiki");
        assert_eq!(projects[1].label, "on-n-off");
    }

    #[test]
    fn overlays_project_skills_and_mcp_read_only() {
        let root = crate::paths::scratch_dir("on-n-off-project-scope");
        fs::create_dir_all(root.join(".claude").join("skills").join("local-feed")).unwrap();
        fs::write(
            root.join(".claude")
                .join("skills")
                .join("local-feed")
                .join("SKILL.md"),
            "---\nname: local-feed\ndescription: Project only\n---\n",
        )
        .unwrap();
        fs::write(
            root.join(".mcp.json"),
            r#"{ "mcpServers": { "repo-docs": { "command": "node", "args": ["docs.js"] } } }"#,
        )
        .unwrap();

        let mut tab = AgentTabDto {
            plugins: vec![],
            user_skills: vec![SkillDto {
                id: "statusline".into(),
                plugin_id: None,
                name: "statusline".into(),
                description: "Global".into(),
                enabled: true,
                togglable: true,
                origin: String::new(),
            }],
            mcp_servers: vec![],
        };
        overlay_project(&mut tab, &root, AgentId::Claude);
        assert_eq!(tab.user_skills.len(), 2);
        let local = tab
            .user_skills
            .iter()
            .find(|skill| skill.name == "local-feed")
            .unwrap();
        assert_eq!(local.origin, ORIGIN_PROJECT);
        assert!(!local.togglable);
        assert!(local.enabled);
        assert_eq!(tab.mcp_servers.len(), 1);
        assert_eq!(tab.mcp_servers[0].name, "repo-docs");
        assert_eq!(tab.mcp_servers[0].origin, ORIGIN_PROJECT);
        assert!(!tab.mcp_servers[0].togglable);
        assert!(tab.mcp_servers[0].id.starts_with("project:"));
    }

    #[test]
    fn reads_claude_inline_project_mcp_servers() {
        let servers = parse_claude_project_mcp(
            r#"{
                "mcpServers": { "github": { "command": "npx" } },
                "projects": {
                    "E:/dev/app": { "mcpServers": { "local-only": { "command": "node" } } }
                }
            }"#,
            Path::new(r"E:\dev\app"),
        );
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].id, "local-only");
    }

    #[test]
    fn inspect_reads_git_branch_and_local_counts() {
        let root = crate::paths::scratch_dir("on-n-off-inspect-project");
        fs::create_dir_all(root.join(".claude").join("skills").join("local-feed")).unwrap();
        fs::write(
            root.join(".claude")
                .join("skills")
                .join("local-feed")
                .join("SKILL.md"),
            "---\nname: local-feed\n---\n",
        )
        .unwrap();
        fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"repo-docs":{"command":"node"}}}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let inspected = inspect_project(&root, AgentId::Claude);
        assert_eq!(inspected.branch, "main");
        assert_eq!(inspected.skill_count, 1);
        assert_eq!(inspected.mcp_count, 1);
        assert_eq!(git_branch(&root), "main");
        let expanded = expand_project_path("~/dev/app");
        assert!(
            expanded
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("dev/app"),
            "{expanded:?}"
        );
    }

    fn write_skill(dir: &Path, name: &str, description: &str) {
        fs::create_dir_all(dir.join(name)).unwrap();
        fs::write(
            dir.join(name).join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n"),
        )
        .unwrap();
    }

    #[test]
    fn overlay_collapses_same_name_across_skill_roots() {
        let root = crate::paths::scratch_dir("on-n-off-project-skill-dedupe");
        write_skill(
            &root.join(".claude").join("skills"),
            "find-skills",
            "Claude copy",
        );
        write_skill(
            &root.join(".agents").join("skills"),
            "find-skills",
            "Agents copy",
        );
        write_skill(
            &root.join(".agents").join("skills"),
            "vercel-react-best-practices",
            "Vercel",
        );
        write_skill(
            &root.join(".claude").join("skills"),
            "vercel-react-best-practices",
            "Vercel again",
        );

        let mut tab = AgentTabDto {
            plugins: vec![],
            user_skills: vec![SkillDto {
                id: "find-skills".into(),
                plugin_id: None,
                name: "find-skills".into(),
                description: "User copy".into(),
                enabled: true,
                togglable: true,
                origin: String::new(),
            }],
            mcp_servers: vec![],
        };
        overlay_project(&mut tab, &root, AgentId::Claude);
        let names: Vec<_> = tab
            .user_skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect();
        assert_eq!(names, vec!["find-skills", "vercel-react-best-practices"]);
        assert!(tab.user_skills[0].togglable);
        assert_eq!(tab.user_skills[1].origin, ORIGIN_PROJECT);
        assert!(!tab.user_skills[1].togglable);
    }
}
