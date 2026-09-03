use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::{AgentAdapter, ItemRoots};
use crate::cli_locate::agent_info;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto, ItemScope, PluginDto, SkillDto};
use crate::mcp::parse_antigravity_json;
use crate::paths::{cursor_root, normalize_skill_path};
use crate::scanner::{scan_plugin_skills, scan_user_skills, ScannedSkill};
use crate::sort::sort_tab;

const SOURCE_LOCAL: &str = "local";

/// Cursor keeps MCP on/off in its own state (the IDE's settings; the CLI's per-project
/// `~/.cursor/projects/<slug>/mcp-{approvals,disabled}.json`) and does not read a `disabled`
/// key from `mcp.json`, so on-n-off lists Cursor's servers but never switches them.
pub const MCP_READ_ONLY: &str = "Cursor manages MCP servers itself: switch them in the Cursor app or with `agent mcp enable <name>` / `agent mcp disable <name>`; on-n-off only lists them.";

#[derive(Debug, Clone)]
struct DiscoveredPlugin {
    id: String,
    name: String,
    source: String,
    path: PathBuf,
}

/// Read-only view of `~/.cursor`: plugins, skills, and MCP servers are listed, never written.
pub struct CursorAdapter {
    root: Option<PathBuf>,
}

impl CursorAdapter {
    pub fn new() -> Self {
        Self {
            root: cursor_root().ok(),
        }
    }

    #[cfg(test)]
    pub fn at(root: PathBuf) -> Self {
        Self { root: Some(root) }
    }

    fn root(&self) -> Result<&Path, AdapterError> {
        self.root
            .as_deref()
            .ok_or_else(|| AdapterError::message("home directory not found"))
    }

    fn mcp_path(&self) -> Result<PathBuf, AdapterError> {
        Ok(self.root()?.join("mcp.json"))
    }

    fn mcp_servers(&self) -> Vec<crate::dto::McpServerDto> {
        let Ok(path) = self.mcp_path() else {
            return Vec::new();
        };
        let Ok(text) = fs::read_to_string(path) else {
            return Vec::new();
        };
        parse_antigravity_json(&text)
            .into_iter()
            .map(|mut server| {
                // Every configured server is live as far as Cursor's config file can tell.
                server.enabled = true;
                server.togglable = false;
                server
            })
            .collect()
    }

    fn discover_plugins(&self) -> Vec<DiscoveredPlugin> {
        let Ok(root) = self.root() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let plugins = root.join("plugins");
        for plugin in scan_plugin_children(&plugins.join("local"), SOURCE_LOCAL) {
            if seen.insert(plugin.id.clone()) {
                out.push(plugin);
            }
        }
        let cache = plugins.join("cache");
        if let Ok(entries) = fs::read_dir(&cache) {
            for entry in entries.flatten() {
                let marketplace = entry.path();
                if !marketplace.is_dir() {
                    continue;
                }
                let source = marketplace
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("cache");
                for plugin in scan_plugin_children(&marketplace, source) {
                    if seen.insert(plugin.id.clone()) {
                        out.push(plugin);
                    }
                }
            }
        }
        out
    }
}

impl AgentAdapter for CursorAdapter {
    fn info(&self) -> AgentInfo {
        let mut info = agent_info(AgentId::Cursor);
        info.install_git = false;
        info.install_folder = false;
        info.plugin_toggle = false;
        info
    }

    fn item_roots(&self, scope: &ItemScope) -> Result<ItemRoots, AdapterError> {
        match scope {
            ItemScope::Global => Ok(ItemRoots {
                skills: self.root()?.join("skills"),
                agents: None,
            }),
            ItemScope::Project { project_path } => Ok(crate::project::project_item_roots(
                Path::new(project_path),
                AgentId::Cursor,
            )),
        }
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        let mut plugins = Vec::new();
        let mut plugin_skill_paths = HashSet::new();
        for discovered in self.discover_plugins() {
            let version = crate::plugin_meta::installed_hint(&discovered.path, None).version;
            let skills: Vec<SkillDto> = scan_plugin_skills(&discovered.path)
                .into_iter()
                .map(|skill| {
                    plugin_skill_paths
                        .insert(normalize_skill_path(&skill.skill_md.to_string_lossy()));
                    cursor_skill(Some(&discovered.id), skill)
                })
                .collect();
            plugins.push(PluginDto {
                id: discovered.id,
                name: discovered.name,
                source: discovered.source,
                version,
                upstream: String::new(),
                out_of_sync: false,
                enabled: true,
                togglable: false,
                skills,
            });
        }

        let mut user_skills = Vec::new();
        if let Ok(root) = self.root() {
            for skill in scan_user_skills(&root.join("skills")) {
                let key = normalize_skill_path(&skill.skill_md.to_string_lossy());
                if plugin_skill_paths.contains(&key) {
                    continue;
                }
                user_skills.push(cursor_skill(None, skill));
            }
        }

        let mut tab = AgentTabDto {
            plugins,
            user_skills,
            mcp_servers: self.mcp_servers(),
        };
        sort_tab(&mut tab);
        Ok(tab)
    }

    fn set_mcp_enabled(&self, _mcp_id: &str, _enabled: bool) -> Result<AgentTabDto, AdapterError> {
        Err(AdapterError::message(MCP_READ_ONLY))
    }
}

fn cursor_skill(plugin_id: Option<&str>, skill: ScannedSkill) -> SkillDto {
    SkillDto {
        id: normalize_skill_path(&skill.skill_md.to_string_lossy()),
        plugin_id: plugin_id.map(str::to_string),
        name: skill.name,
        description: skill.description,
        enabled: true,
        togglable: false,
        origin: String::new(),
    }
}

fn plugin_manifest(dir: &Path) -> Option<PathBuf> {
    let cursor = dir.join(".cursor-plugin").join("plugin.json");
    if cursor.is_file() {
        return Some(cursor);
    }
    let root = dir.join("plugin.json");
    if root.is_file() {
        return Some(root);
    }
    None
}

fn read_plugin_name(manifest: &Path) -> Option<String> {
    let text = fs::read_to_string(manifest).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Marker Cursor writes once a marketplace checkout finished downloading.
const CACHE_COMPLETE: &str = ".cache-complete";

/// The directory holding a plugin's files: `dir` itself when it carries a manifest (local
/// plugins, symlinked repos), else the best of its per-version checkouts — Cursor stores
/// marketplace installs as `cache/<marketplace>/<plugin>/<commit>/` and keeps old commits.
fn plugin_install_dir(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    if let Some(manifest) = plugin_manifest(dir) {
        return Some((dir.to_path_buf(), manifest));
    }
    let checkouts = fs::read_dir(dir).ok()?;
    checkouts
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| plugin_manifest(&path).map(|manifest| (path, manifest)))
        .max_by_key(|(path, manifest)| checkout_rank(path, manifest))
}

/// Complete checkouts beat partial ones, then the highest manifest version, then the most
/// recently written manifest.
fn checkout_rank(checkout: &Path, manifest: &Path) -> (bool, Vec<u64>, std::time::SystemTime) {
    let complete = checkout.join(CACHE_COMPLETE).is_file();
    let version = crate::plugin_meta::installed_hint(checkout, None).version;
    let version_key = version
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect();
    let written = fs::metadata(manifest)
        .and_then(|meta| meta.modified())
        .unwrap_or(std::time::UNIX_EPOCH);
    (complete, version_key, written)
}

fn scan_plugin_children(parent: &Path, source: &str) -> Vec<DiscoveredPlugin> {
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let plugin_dir = entry.path();
        if !plugin_dir.is_dir() {
            continue;
        }
        let Some((path, manifest)) = plugin_install_dir(&plugin_dir) else {
            continue;
        };
        let folder = plugin_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("plugin")
            .to_string();
        let name = read_plugin_name(&manifest).unwrap_or_else(|| folder.clone());
        out.push(DiscoveredPlugin {
            id: format!("{name}@{source}"),
            name,
            source: source.to_string(),
            path,
        });
    }
    out
}

#[cfg(test)]
mod tests;
