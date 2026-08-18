use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::adapter::AgentAdapter;
use crate::cli_locate::agent_info;
use crate::config_io::ConfigIo;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto, PluginDto, SkillDto};
use crate::mcp::parse_antigravity_json;
use crate::paths::{cursor_root, normalize_skill_path};
use crate::scanner::{scan_plugin_skills, scan_user_skills, ScannedSkill};
use crate::sort::sort_tab;

const SOURCE_LOCAL: &str = "local";

#[derive(Debug, Clone)]
struct DiscoveredPlugin {
    id: String,
    name: String,
    source: String,
    path: PathBuf,
}

pub struct CursorAdapter {
    root: Option<PathBuf>,
    io: ConfigIo,
    write: Mutex<()>,
}

impl CursorAdapter {
    pub fn new() -> Self {
        Self {
            root: cursor_root().ok(),
            io: ConfigIo::production(),
            write: Mutex::new(()),
        }
    }

    #[cfg(test)]
    pub fn at(root: PathBuf) -> Self {
        let io = ConfigIo::at(root.join("_backups"));
        Self {
            root: Some(root),
            io,
            write: Mutex::new(()),
        }
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
        let mut info = agent_info(AgentId::Cursor, "cursor-agent");
        info.install_git = false;
        info.install_folder = false;
        info.plugin_toggle = false;
        info
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

    fn set_mcp_enabled(&self, mcp_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_mcp_togglable(mcp_id)?;
        let path = self.mcp_path()?;
        self.io
            .patch_antigravity_mcp_enabled(AgentId::Cursor, &path, mcp_id, enabled)?;
        self.list_tab()
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

fn scan_plugin_children(parent: &Path, source: &str) -> Vec<DiscoveredPlugin> {
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(manifest) = plugin_manifest(&path) else {
            continue;
        };
        let folder = path
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
mod tests {
    use super::*;

    fn write_skill(dir: &Path, name: &str, description: &str) {
        fs::create_dir_all(dir.join(name)).unwrap();
        fs::write(
            dir.join(name).join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n"),
        )
        .unwrap();
    }

    fn write_plugin(
        dir: &Path,
        folder: &str,
        name: &str,
        skill: Option<&str>,
        cursor_manifest: bool,
    ) {
        let plugin = dir.join(folder);
        fs::create_dir_all(&plugin).unwrap();
        let manifest = if cursor_manifest {
            let nested = plugin.join(".cursor-plugin");
            fs::create_dir_all(&nested).unwrap();
            nested.join("plugin.json")
        } else {
            plugin.join("plugin.json")
        };
        fs::write(
            &manifest,
            format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
        )
        .unwrap();
        if let Some(skill_name) = skill {
            write_skill(&plugin.join("skills"), skill_name, "From plugin");
        }
    }

    #[test]
    fn empty_home_lists_nothing() {
        let root = crate::paths::scratch_dir("on-n-off-cursor-empty");
        fs::create_dir_all(&root).unwrap();
        let tab = CursorAdapter::at(root.clone()).list_tab().expect("list");
        assert!(tab.plugins.is_empty());
        assert!(tab.user_skills.is_empty());
        assert!(tab.mcp_servers.is_empty());
        let info = CursorAdapter::at(root.clone()).info();
        assert_eq!(info.id, AgentId::Cursor);
        assert_eq!(info.display_name, "Cursor");
        assert!(!info.plugin_toggle);
        assert!(!info.install_git);
        assert!(!info.install_folder);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_local_and_cache_plugins_user_skills_and_mcp() {
        let root = crate::paths::scratch_dir("on-n-off-cursor-list");
        write_plugin(
            &root.join("plugins").join("local"),
            "reviewer",
            "reviewer",
            Some("pr-review"),
            true,
        );
        write_plugin(
            &root.join("plugins").join("cache").join("workshop"),
            "workbench",
            "workbench",
            None,
            false,
        );
        write_skill(&root.join("skills"), "notes", "User skill");
        write_skill(&root.join("skills-cursor"), "builtin", "Managed builtin");
        fs::write(
            root.join("mcp.json"),
            r#"{"mcpServers":{"docs":{"url":"https://docs.example/mcp"},"github":{"command":"npx","args":["-y","gh"],"disabled":true}}}"#,
        )
        .unwrap();

        let tab = CursorAdapter::at(root.clone()).list_tab().expect("list");
        let local = tab
            .plugins
            .iter()
            .find(|plugin| plugin.id == "reviewer@local")
            .expect("local plugin");
        assert_eq!(local.source, "local");
        assert!(local.enabled);
        assert!(!local.togglable);
        assert_eq!(local.skills.len(), 1);
        assert_eq!(local.skills[0].name, "pr-review");
        assert!(!local.skills[0].togglable);
        let cached = tab
            .plugins
            .iter()
            .find(|plugin| plugin.id == "workbench@workshop")
            .expect("cached plugin");
        assert_eq!(cached.source, "workshop");
        assert!(!cached.togglable);
        assert_eq!(tab.user_skills.len(), 1);
        assert_eq!(tab.user_skills[0].name, "notes");
        assert!(!tab.user_skills[0].togglable);
        assert!(!tab.user_skills.iter().any(|skill| skill.name == "builtin"));
        assert_eq!(tab.mcp_servers.len(), 2);
        let docs = tab
            .mcp_servers
            .iter()
            .find(|server| server.id == "docs")
            .unwrap();
        assert!(docs.enabled);
        assert!(docs.togglable);
        let github = tab
            .mcp_servers
            .iter()
            .find(|server| server.id == "github")
            .unwrap();
        assert!(!github.enabled);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_toggle_patches_disabled_and_backs_up() {
        let root = crate::paths::scratch_dir("on-n-off-cursor-mcp-toggle");
        fs::write(
            root.join("mcp.json"),
            r#"{"mcpServers":{"github":{"command":"npx","args":["-y","gh"]}}}"#,
        )
        .unwrap();
        let adapter = CursorAdapter::at(root.clone());
        let tab = adapter.set_mcp_enabled("github", false).expect("toggle");
        assert!(!tab.mcp_servers[0].enabled);
        let text = fs::read_to_string(root.join("mcp.json")).unwrap();
        assert!(text.contains("\"disabled\": true") || text.contains("\"disabled\":true"));
        let backups = root.join("_backups").join("cursor");
        let count = fs::read_dir(&backups)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert!(count >= 1, "expected a cursor mcp.json backup");
        let err = adapter
            .set_mcp_enabled("missing", false)
            .expect_err("missing");
        assert!(
            err.message.contains("mcp server not found") || err.message.contains("not togglable")
        );
        let _ = fs::remove_dir_all(root);
    }
}
