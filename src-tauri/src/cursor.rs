use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::AgentAdapter;
use crate::cli_locate::agent_info;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto, PluginDto, SkillDto};
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
        // Cursor keeps on/off in its own state and never reads a `disabled` key, so every
        // configured server is listed as on and none can be switched from here.
        for server in &tab.mcp_servers {
            assert!(server.enabled, "{} should read as configured/on", server.id);
            assert!(!server.togglable, "{} must not be togglable", server.id);
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn versioned_cache_lists_the_newest_complete_version_once() {
        // Cursor installs marketplace plugins as cache/<marketplace>/<plugin>/<commit>/, keeping
        // older checkouts around and marking finished downloads with `.cache-complete`.
        let root = crate::paths::scratch_dir("on-n-off-cursor-versioned");
        let plugin = root
            .join("plugins")
            .join("cache")
            .join("workshop")
            .join("workbench");
        for (sha, version, skill, complete) in [
            ("1111111", "0.20.0", "old-skill", true),
            ("2222222", "0.22.1", "review", true),
            ("3333333", "0.23.0", "half-downloaded", false),
        ] {
            let checkout = plugin.join(sha);
            let manifest_dir = checkout.join(".cursor-plugin");
            fs::create_dir_all(&manifest_dir).unwrap();
            fs::write(
                manifest_dir.join("plugin.json"),
                format!(r#"{{"name":"workbench","version":"{version}"}}"#),
            )
            .unwrap();
            write_skill(&checkout.join("skills"), skill, "From plugin");
            if complete {
                fs::write(checkout.join(".cache-complete"), "").unwrap();
            }
        }
        let tab = CursorAdapter::at(root.clone()).list_tab().expect("list");
        assert_eq!(tab.plugins.len(), 1, "{:?}", tab.plugins);
        let workbench = &tab.plugins[0];
        assert_eq!(workbench.id, "workbench@workshop");
        assert_eq!(workbench.version, "0.22.1");
        assert_eq!(workbench.skills.len(), 1);
        assert_eq!(workbench.skills[0].name, "review");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_toggle_is_refused_and_leaves_the_file_alone() {
        let root = crate::paths::scratch_dir("on-n-off-cursor-mcp-toggle");
        let original = r#"{"mcpServers":{"github":{"command":"npx","args":["-y","gh"]}}}"#;
        fs::write(root.join("mcp.json"), original).unwrap();
        let adapter = CursorAdapter::at(root.clone());
        let err = adapter
            .set_mcp_enabled("github", false)
            .expect_err("Cursor MCP servers are read-only here");
        assert!(err.message.contains("Cursor"), "{}", err.message);
        assert!(err.message.contains("agent mcp"), "{}", err.message);
        assert_eq!(fs::read_to_string(root.join("mcp.json")).unwrap(), original);
        assert!(
            !root.join("_backups").exists(),
            "no backup for a refused write"
        );
        let _ = fs::remove_dir_all(root);
    }
}
