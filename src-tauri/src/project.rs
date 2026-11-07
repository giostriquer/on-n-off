use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::ItemRoots;
use crate::dto::{AgentTabDto, McpServerDto, PluginDto, ProjectDto, SkillDto};
use crate::mcp::McpFormat;
use crate::paths::normalize_skill_path;
use crate::scanner::SkillFiles;
use crate::sort::sort_tab;

pub const ORIGIN_PROJECT: &str = "project";

const PROJECT_ID: &str = "project:";

#[derive(Debug, Clone)]
pub struct Projects {
    pub items: ItemRoots,
    pub more_skills: Vec<PathBuf>,
    pub skill_files: SkillFiles,
    pub plugins: Option<PluginFolders>,
    pub mcp: Vec<McpFile>,
    pub record: Option<ProjectRecord>,
}

#[derive(Debug, Clone)]
pub struct PluginFolders {
    pub dirs: Vec<PathBuf>,
    pub read: fn(&Path) -> Vec<PluginDto>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpFile {
    path: PathBuf,
    format: McpFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectRecord {
    Claude(PathBuf),
    Codex(PathBuf),
}

pub fn relative(parts: &[&str]) -> PathBuf {
    parts.iter().collect()
}

impl McpFile {
    pub fn at(parts: &[&str], format: McpFormat) -> Self {
        Self {
            path: relative(parts),
            format,
        }
    }

    pub fn mcp_json() -> Self {
        Self::at(&[".mcp.json"], McpFormat::ClaudeJson)
    }

    fn servers(&self, project: &Path) -> Vec<McpServerDto> {
        fs::read_to_string(project.join(&self.path))
            .map(|text| self.format.servers(&text))
            .unwrap_or_default()
    }
}

impl ProjectRecord {
    fn paths(&self) -> Vec<String> {
        match self {
            Self::Claude(path) => parse_claude_projects(&read(path)),
            Self::Codex(path) => parse_codex_projects(&read(path)),
        }
    }
}

impl Projects {
    pub fn view(&self, tab: &mut AgentTabDto, project: &Path) {
        self.add_plugins(tab, project);
        self.add_skills(tab, project);
        self.add_servers(tab, project);
        sort_tab(tab);
    }

    pub fn inspect(&self, path: &Path, home: Option<&Path>) -> ProjectDto {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            expand_project_path_in(&path.to_string_lossy(), home)
        };
        let display = resolved.to_string_lossy().to_string();
        let mut tab = AgentTabDto {
            plugins: vec![],
            user_skills: vec![],
            mcp_servers: vec![],
            hooks: vec![],
        };
        self.view(&mut tab, &resolved);
        ProjectDto {
            id: normalize_project_key(&display),
            label: project_label(&display),
            path: display,
            branch: git_branch(&resolved),
            skill_count: tab.user_skills.len() as u32,
            mcp_count: tab.mcp_servers.len() as u32,
        }
    }

    pub fn list(&self, home: Option<&Path>) -> Vec<ProjectDto> {
        let paths = self.record.iter().flat_map(ProjectRecord::paths);
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for path in paths {
            let dto = self.inspect(Path::new(path.trim()), home);
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

    fn add_plugins(&self, tab: &mut AgentTabDto, project: &Path) {
        let Some(folders) = &self.plugins else {
            return;
        };
        let mut seen: HashSet<String> =
            tab.plugins.iter().map(|plugin| plugin.id.clone()).collect();
        for dir in &folders.dirs {
            for plugin in (folders.read)(&project.join(dir)) {
                let plugin = as_project_plugin(plugin);
                if seen.insert(plugin.id.clone()) {
                    tab.plugins.push(plugin);
                }
            }
        }
    }

    fn add_skills(&self, tab: &mut AgentTabDto, project: &Path) {
        let mut seen_ids: HashSet<String> = tab
            .user_skills
            .iter()
            .map(|skill| skill.id.clone())
            .collect();
        let mut seen_names: HashSet<String> = tab
            .user_skills
            .iter()
            .chain(tab.plugins.iter().flat_map(|plugin| plugin.skills.iter()))
            .map(|skill| skill.name.to_ascii_lowercase())
            .collect();
        let dirs = std::iter::once(&self.items.skills).chain(&self.more_skills);
        for dir in dirs {
            for skill in self.skill_files.scan(&project.join(dir)) {
                let name_key = skill.name.to_ascii_lowercase();
                let id = format!(
                    "{PROJECT_ID}{}",
                    normalize_skill_path(&skill.skill_md.to_string_lossy())
                );
                if !seen_ids.insert(id.clone()) || !seen_names.insert(name_key) {
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
    }

    fn add_servers(&self, tab: &mut AgentTabDto, project: &Path) {
        let mut servers: Vec<McpServerDto> = self
            .mcp
            .iter()
            .flat_map(|file| file.servers(project))
            .collect();
        if let Some(ProjectRecord::Claude(path)) = &self.record {
            let claude_json: serde_json::Value =
                serde_json::from_str(&read(path)).unwrap_or_default();
            servers.extend(crate::claude_mcp::project_servers(
                &mut tab.mcp_servers,
                &claude_json,
                project,
            ));
        }
        let mut seen: HashSet<String> = tab
            .mcp_servers
            .iter()
            .map(|server| server.id.clone())
            .collect();
        for mut server in servers {
            if !server.id.starts_with(PROJECT_ID) {
                server.id = format!("{PROJECT_ID}{}", server.id);
            }
            if !seen.insert(server.id.clone()) {
                continue;
            }
            server.togglable = false;
            server.origin = ORIGIN_PROJECT.to_string();
            tab.mcp_servers.push(server);
        }
    }
}

fn as_project_plugin(mut plugin: PluginDto) -> PluginDto {
    plugin.id = format!("{PROJECT_ID}{}", plugin.id);
    plugin.togglable = false;
    for skill in &mut plugin.skills {
        skill.origin = ORIGIN_PROJECT.to_string();
        skill.togglable = false;
    }
    plugin
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

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

fn project_label(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    trimmed
        .rsplit(['\\', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

fn parse_claude_projects(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    value
        .get("projects")
        .and_then(|value| value.as_object())
        .map(|projects| projects.keys().cloned().collect())
        .unwrap_or_default()
}

fn parse_codex_projects(text: &str) -> Vec<String> {
    let Ok(document) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    document
        .get("projects")
        .and_then(|value| value.as_table())
        .map(|projects| projects.keys().cloned().collect())
        .unwrap_or_default()
}

fn expand_project_path_in(raw: &str, home: Option<&Path>) -> PathBuf {
    let raw = raw.trim();
    if let Some(rest) = raw
        .strip_prefix("~/")
        .or_else(|| raw.strip_prefix("~\\"))
        .or_else(|| (raw == "~").then_some(""))
    {
        if let Some(home) = home {
            return if rest.is_empty() {
                home.to_path_buf()
            } else {
                home.join(rest)
            };
        }
    }
    PathBuf::from(raw)
}

fn git_branch(root: &Path) -> String {
    let text = fs::read_to_string(root.join(".git").join("HEAD")).unwrap_or_default();
    text.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_string)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
