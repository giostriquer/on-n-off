use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;

use crate::adapter::{AgentAdapter, ItemRoots};
use crate::cli::{run_npx_skills, AgentCli, INSTALL_TIMEOUT};
use crate::cli_locate::agent_info;
use crate::config_io::ConfigIo;
use crate::dto::{
    AdapterError, AgentId, AgentInfo, AgentTabDto, ErrorKind, ItemScope, PluginDto, SkillDto,
};
use crate::install_source::{parse_install_source, InstallSource};
use crate::mcp::parse_claude_json;
use crate::paths::{claude_root, plugin_id_parts};
use crate::scanner::{scan_plugin_skills, scan_user_skills, ScannedSkill};
use crate::sort::sort_tab;

#[derive(Debug, Deserialize)]
struct InstalledPluginsFile {
    plugins: HashMap<String, Vec<InstalledPluginEntry>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledPluginEntry {
    install_path: PathBuf,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ClaudeSettings {
    #[serde(default)]
    enabled_plugins: HashMap<String, bool>,
    #[serde(default)]
    skill_overrides: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PluginManifest {
    #[serde(default = "default_true")]
    default_enabled: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct KnownMarketplace {
    #[serde(default)]
    source: KnownMarketplaceSource,
    #[serde(default)]
    install_location: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Default)]
struct KnownMarketplaceSource {
    #[serde(default)]
    repo: String,
    #[serde(default)]
    url: String,
}

impl KnownMarketplace {
    fn origin(&self) -> String {
        let repo = self.source.repo.trim();
        if !repo.is_empty() {
            return repo.to_string();
        }
        self.source.url.trim().to_string()
    }
}

fn default_true() -> bool {
    true
}

pub struct ClaudeAdapter {
    root: Option<PathBuf>,
    claude_json: Option<PathBuf>,
    io: ConfigIo,
    write: Mutex<()>,
    cli: AgentCli,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        let root = claude_root().ok();
        let claude_json = crate::paths::user_home()
            .ok()
            .map(|home| home.join(".claude.json"));
        Self {
            root,
            claude_json,
            io: ConfigIo::production(),
            write: Mutex::new(()),
            cli: AgentCli::new("claude"),
        }
    }

    #[cfg(test)]
    pub fn at(root: PathBuf) -> Self {
        Self::at_with_cli(root, AgentCli::new("on-n-off-no-such-claude.exe"))
    }

    #[cfg(test)]
    pub fn at_with_cli(root: PathBuf, cli: AgentCli) -> Self {
        let claude_json = root.join(".claude.json");
        let io = ConfigIo::at(root.join("_backups"));
        Self {
            root: Some(root),
            claude_json: Some(claude_json),
            io,
            write: Mutex::new(()),
            cli,
        }
    }

    fn root(&self) -> Result<&Path, AdapterError> {
        self.root
            .as_deref()
            .ok_or_else(|| AdapterError::message("home directory not found"))
    }

    fn settings(&self) -> Result<ClaudeSettings, AdapterError> {
        let path = self.root()?.join("settings.json");
        if !path.exists() {
            return Ok(ClaudeSettings::default());
        }
        let text = fs::read_to_string(&path).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })?;
        serde_json::from_str(&text).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })
    }

    fn installed(&self) -> Result<Vec<(String, PathBuf, Option<String>)>, AdapterError> {
        let path = self.root()?.join("plugins").join("installed_plugins.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&path).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })?;
        let parsed: InstalledPluginsFile =
            serde_json::from_str(&text).map_err(|error| AdapterError {
                kind: ErrorKind::Parse,
                message: error.to_string(),
                path: Some(path.display().to_string()),
            })?;
        let mut plugins = Vec::new();
        for (id, entries) in parsed.plugins {
            let Some(entry) = entries
                .into_iter()
                .find(|entry| entry.scope.as_deref().is_none_or(|scope| scope == "user"))
            else {
                continue;
            };
            plugins.push((id, entry.install_path, entry.version));
        }
        Ok(plugins)
    }

    fn mcp_servers(&self) -> Vec<crate::dto::McpServerDto> {
        let Some(path) = self.claude_json.as_ref() else {
            return Vec::new();
        };
        let Ok(text) = fs::read_to_string(path) else {
            return Vec::new();
        };
        parse_claude_json(&text)
    }

    fn marketplace_roots(&self) -> HashMap<String, (PathBuf, String)> {
        let Ok(root) = self.root() else {
            return HashMap::new();
        };
        let mut roots = HashMap::new();
        let known_path = root.join("plugins").join("known_marketplaces.json");
        if let Ok(text) = fs::read_to_string(&known_path) {
            if let Ok(known) = serde_json::from_str::<HashMap<String, KnownMarketplace>>(&text) {
                for (name, entry) in known {
                    let location = entry
                        .install_location
                        .clone()
                        .unwrap_or_else(|| root.join("plugins").join("marketplaces").join(&name));
                    roots.insert(name, (location, entry.origin()));
                }
            }
        }
        roots
    }

    fn build_tab(&self, enrich_remote: bool) -> Result<AgentTabDto, AdapterError> {
        let settings = self.settings()?;
        let catalogs: HashMap<String, HashMap<String, crate::plugin_meta::VersionHint>> = self
            .marketplace_roots()
            .into_iter()
            .map(|(name, (path, origin))| {
                let mut hints = crate::plugin_meta::catalog_hints(&path);
                if enrich_remote {
                    crate::plugin_meta::apply_remote_marketplace_versions(
                        &mut hints, &origin, &path,
                    );
                }
                (name, hints)
            })
            .collect();
        let mut plugins = Vec::new();
        for (id, install_path, inventory_version) in self.installed()? {
            let (name, source) = plugin_id_parts(&id);
            let enabled = settings
                .enabled_plugins
                .get(&id)
                .copied()
                .unwrap_or_else(|| plugin_default_enabled(&install_path));
            let skills = scan_plugin_skills(&install_path)
                .into_iter()
                .map(|skill| claude_plugin_skill(&id, skill))
                .collect();
            let installed =
                crate::plugin_meta::installed_hint(&install_path, inventory_version.as_deref());
            let mut catalog = catalogs
                .get(&source)
                .and_then(|hints| hints.get(&name))
                .cloned();
            if enrich_remote {
                if let Some(hint) = catalog.as_mut() {
                    crate::plugin_meta::fill_remote_version(hint);
                }
            }
            let (version, upstream, out_of_sync) =
                crate::plugin_meta::resolve_versions(&installed, catalog.as_ref());
            plugins.push(PluginDto {
                id,
                name,
                source,
                version,
                upstream,
                out_of_sync,
                enabled,
                togglable: true,
                skills,
            });
        }
        let user_skills = scan_user_skills(&self.root()?.join("skills"))
            .into_iter()
            .map(|skill| claude_user_skill(skill, &settings.skill_overrides))
            .collect();
        let mut tab = AgentTabDto {
            plugins,
            user_skills,
            mcp_servers: self.mcp_servers(),
        };
        sort_tab(&mut tab);
        Ok(tab)
    }
}

impl AgentAdapter for ClaudeAdapter {
    fn info(&self) -> AgentInfo {
        agent_info(AgentId::Claude)
    }

    fn item_roots(&self, scope: &ItemScope) -> Result<ItemRoots, AdapterError> {
        match scope {
            ItemScope::Global => {
                let root = self.root()?;
                Ok(ItemRoots {
                    skills: root.join("skills"),
                    agents: Some(root.join("agents")),
                })
            }
            ItemScope::Project { project_path } => Ok(crate::project::project_item_roots(
                Path::new(project_path),
                AgentId::Claude,
            )),
        }
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        self.build_tab(true)
    }

    fn list_local_tab(&self) -> Result<AgentTabDto, AdapterError> {
        self.build_tab(false)
    }

    fn list_projects(&self) -> Vec<crate::dto::ProjectDto> {
        let Ok(home) = crate::paths::user_home() else {
            return Vec::new();
        };
        let text = fs::read_to_string(home.join(".claude.json")).unwrap_or_default();
        crate::project::inspect_projects(
            crate::project::parse_claude_projects(&text),
            AgentId::Claude,
        )
    }

    fn set_skill_enabled(
        &self,
        skill_id: &str,
        enabled: bool,
    ) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_togglable(skill_id)?;
        self.io.patch_json_skill_override(
            AgentId::Claude,
            &self.root()?.join("settings.json"),
            skill_id,
            enabled,
        )?;
        self.list_tab()
    }

    fn set_mcp_enabled(&self, mcp_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_mcp_togglable(mcp_id)?;
        let path = self
            .claude_json
            .as_ref()
            .ok_or_else(|| AdapterError::message("home directory not found"))?;
        self.io
            .patch_json_mcp_enabled(AgentId::Claude, path, mcp_id, enabled)?;
        self.list_tab()
    }

    fn set_plugin_enabled(
        &self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_plugin(plugin_id)?;
        let settings = self.root()?.join("settings.json");
        self.io.backup_file(AgentId::Claude, &settings)?;
        let action = if enabled { "enable" } else { "disable" };
        self.cli.run(&["plugin", action, "-s", "user", plugin_id])?;
        self.list_tab()
    }

    fn install_plugin(&self, source: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        let parsed = parse_install_source(source)?;
        if parsed.is_npx_skills() {
            run_npx_skills(&parsed, "claude-code")?;
            return self.list_tab();
        }
        self.io
            .backup_file(AgentId::Claude, &self.root()?.join("settings.json"))?;
        self.cli
            .run_args_timed(&parsed.claude_install_argv(), INSTALL_TIMEOUT)?;
        self.list_tab()
    }

    fn uninstall_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_plugin(plugin_id)?;
        self.io
            .backup_file(AgentId::Claude, &self.root()?.join("settings.json"))?;
        self.cli
            .run_args(&InstallSource::claude_uninstall_argv(plugin_id))?;
        self.list_tab()
    }

    fn update_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_plugin(plugin_id)?;
        self.io
            .backup_file(AgentId::Claude, &self.root()?.join("settings.json"))?;
        if let Some(marketplace) = InstallSource::plugin_marketplace(plugin_id) {
            self.cli.run_args_timed(
                &InstallSource::claude_marketplace_update_argv(marketplace),
                INSTALL_TIMEOUT,
            )?;
        }
        self.cli.run_args_timed(
            &InstallSource::claude_update_argv(plugin_id),
            INSTALL_TIMEOUT,
        )?;
        self.list_tab()
    }
}

fn plugin_default_enabled(install_path: &Path) -> bool {
    let path = install_path.join(".claude-plugin").join("plugin.json");
    let Ok(text) = fs::read_to_string(path) else {
        return true;
    };
    serde_json::from_str::<PluginManifest>(&text)
        .map(|manifest| manifest.default_enabled)
        .unwrap_or(true)
}

fn claude_plugin_skill(plugin_id: &str, skill: ScannedSkill) -> SkillDto {
    SkillDto {
        id: format!("{plugin_id}:{}", skill.name),
        plugin_id: Some(plugin_id.to_string()),
        name: skill.name,
        description: skill.description,
        enabled: true,
        togglable: false,
        origin: String::new(),
    }
}

fn claude_user_skill(skill: ScannedSkill, overrides: &HashMap<String, String>) -> SkillDto {
    let enabled = !matches!(overrides.get(&skill.name).map(String::as_str), Some("off"));
    SkillDto {
        id: skill.name.clone(),
        plugin_id: None,
        name: skill.name,
        description: skill.description,
        enabled,
        togglable: true,
        origin: String::new(),
    }
}

#[cfg(test)]
mod tests;
