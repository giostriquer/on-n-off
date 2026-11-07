use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::adapter::{AgentAdapter, ItemRoots};
use crate::capabilities::{Capabilities, Change};
use crate::change::{switch, Checked, Step};
use crate::cli::{argv, AgentCli};
use crate::drift::Marketplace;
use crate::dto::{AdapterError, AgentId, AgentTabDto, ErrorKind, PluginDto, SkillDto};
use crate::install_source::InstallSource;
use crate::paths::{claude_root, plugin_id_parts};
use crate::project::{relative, McpFile, ProjectRecord, Projects};
use crate::scanner::{scan_plugin_skills, scan_user_skills, ScannedSkill, SkillFiles};
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

struct Documents {
    settings: PathBuf,
    installed: PathBuf,
    marketplaces: PathBuf,
}

impl Documents {
    fn all(&self) -> Vec<PathBuf> {
        let Self {
            settings,
            installed,
            marketplaces,
        } = self;
        vec![settings.clone(), installed.clone(), marketplaces.clone()]
    }
}

pub struct ClaudeAdapter {
    root: Option<PathBuf>,
    claude_json: Option<PathBuf>,
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
            cli: AgentCli::new(AgentId::Claude.binary_name()),
        }
    }

    #[cfg(test)]
    pub fn at(root: PathBuf) -> Self {
        Self::at_with_cli(root, AgentCli::new("on-n-off-no-such-claude.exe"))
    }

    #[cfg(test)]
    pub fn at_with_cli(root: PathBuf, cli: AgentCli) -> Self {
        let claude_json = root.join(".claude.json");
        Self {
            root: Some(root),
            claude_json: Some(claude_json),
            cli,
        }
    }

    fn root(&self) -> Result<&Path, AdapterError> {
        self.root
            .as_deref()
            .ok_or_else(|| AdapterError::message("home directory not found"))
    }

    fn documents(&self) -> Result<Documents, AdapterError> {
        let root = self.root()?;
        let plugins = root.join("plugins");
        Ok(Documents {
            settings: root.join("settings.json"),
            installed: plugins.join("installed_plugins.json"),
            marketplaces: plugins.join("known_marketplaces.json"),
        })
    }

    fn settings(&self) -> Result<ClaudeSettings, AdapterError> {
        let path = self.documents()?.settings;
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
        let path = self.documents()?.installed;
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

    fn mcp_servers(
        &self,
        plugins: &[crate::plugin_files::PluginSource],
    ) -> Vec<crate::dto::McpServerDto> {
        let config: serde_json::Value = self
            .claude_json
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let mut servers = crate::mcp::claude_json_servers(&config);
        servers.extend(crate::claude_mcp::plugin_servers(plugins));
        servers.extend(crate::claude_mcp::local_servers(&config));
        servers
    }
}

impl AgentAdapter for ClaudeAdapter {
    fn supports_accounts(&self) -> bool {
        true
    }
    fn id(&self) -> AgentId {
        AgentId::Claude
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            plugin_switch: Change::cli("claude plugin enable/disable"),
            skill_switch: Change::file("file patch · ~/.claude/settings.json skillOverrides"),
            mcp_switch: Change::file("file patch · ~/.claude.json disabled"),
            install: Change::cli("claude plugin install · marketplace add"),
            refused_sources: BTreeMap::new(),
            uninstall: Change::cli("claude plugin uninstall"),
            uninstall_from: None,
            update: Change::cli("claude plugin update"),
            reads_hooks: true,
        }
    }

    fn global_item_roots(&self) -> Result<ItemRoots, AdapterError> {
        let root = self.root()?;
        Ok(ItemRoots {
            skills: root.join("skills"),
            agents: Some(root.join("agents")),
        })
    }

    fn projects(&self) -> Projects {
        Projects {
            items: ItemRoots {
                skills: relative(&[".claude", "skills"]),
                agents: Some(relative(&[".claude", "agents"])),
            },
            more_skills: vec![relative(&[".agents", "skills"])],
            skill_files: SkillFiles::Folders,
            plugins: None,
            mcp: vec![McpFile::mcp_json()],
            record: self.claude_json.clone().map(ProjectRecord::Claude),
        }
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        let settings = self.settings()?;
        let mut plugins = Vec::new();
        let mut enabled_plugins = Vec::new();
        for (id, install_path, inventory_version) in self.installed()? {
            let (name, source) = plugin_id_parts(&id);
            let enabled = settings
                .enabled_plugins
                .get(&id)
                .copied()
                .unwrap_or_else(|| plugin_default_enabled(&install_path));
            if enabled {
                enabled_plugins.push(crate::plugin_files::PluginSource {
                    id: id.clone(),
                    name: name.clone(),
                    root: install_path.clone(),
                });
            }
            let skills = scan_plugin_skills(&install_path)
                .into_iter()
                .map(|skill| claude_plugin_skill(&id, skill))
                .collect();
            plugins.push(PluginDto {
                id,
                name,
                source,
                version: crate::drift::installed(&install_path, inventory_version.as_deref()),
                upstream: String::new(),
                out_of_sync: false,
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
            mcp_servers: self.mcp_servers(&enabled_plugins),
            hooks: crate::hooks::claude_hooks(self.root()?, &enabled_plugins),
        };
        sort_tab(&mut tab);
        Ok(tab)
    }

    fn marketplaces(&self) -> Vec<Marketplace> {
        let (Ok(root), Ok(documents)) = (self.root(), self.documents()) else {
            return Vec::new();
        };
        let Ok(text) = fs::read_to_string(documents.marketplaces) else {
            return Vec::new();
        };
        let Ok(known) = serde_json::from_str::<HashMap<String, KnownMarketplace>>(&text) else {
            return Vec::new();
        };
        known
            .into_iter()
            .map(|(name, entry)| {
                let location = entry
                    .install_location
                    .clone()
                    .unwrap_or_else(|| root.join("plugins").join("marketplaces").join(&name));
                Marketplace::checkout(name, location, entry.origin())
            })
            .collect()
    }

    fn step(&self, change: &Checked) -> Result<Option<Step<'_>>, AdapterError> {
        let documents = self.documents()?;
        let settings = documents.settings.clone();
        let inventory = || documents.all();
        Ok(Some(match change {
            Checked::Plugin { plugin, on } => Step::cli(
                &self.cli,
                inventory(),
                vec![argv(&["plugin", switch(*on), "-s", "user", &plugin.id])],
            ),
            Checked::Skill { skill, on } => {
                let (name, on) = (skill.id.clone(), *on);
                Step::json(settings, move |root| skill_override(root, &name, on))
            }
            Checked::Mcp { server, on } => {
                let (id, on) = (server.id.clone(), *on);
                let path = self
                    .claude_json
                    .clone()
                    .ok_or_else(|| AdapterError::message("home directory not found"))?;
                Step::json(path, move |root| mcp_enabled(root, &id, on))
            }
            Checked::Install(source @ InstallSource::NpxSkills { .. }) => {
                Step::npx(source.clone(), "claude-code")
            }
            Checked::Install(InstallSource::Plugin(id)) => Step::cli(
                &self.cli,
                inventory(),
                vec![argv(&["plugin", "install", "-s", "user", "-y", id])],
            ),
            Checked::Install(source) => Step::cli(
                &self.cli,
                inventory(),
                vec![argv(&[
                    "plugin",
                    "marketplace",
                    "add",
                    "--scope",
                    "user",
                    &source.as_cli_source(),
                ])],
            ),
            Checked::Uninstall(plugin) => Step::cli(
                &self.cli,
                inventory(),
                vec![argv(&[
                    "plugin",
                    "uninstall",
                    "-s",
                    "user",
                    "-y",
                    &plugin.id,
                ])],
            ),
            Checked::Update(plugin) => Step::cli(
                &self.cli,
                inventory(),
                InstallSource::plugin_marketplace(&plugin.id)
                    .map(|marketplace| argv(&["plugin", "marketplace", "update", marketplace]))
                    .into_iter()
                    .chain([argv(&["plugin", "update", "-s", "user", "-y", &plugin.id])])
                    .collect(),
            ),
        }))
    }
}

fn skill_override(root: &mut JsonValue, name: &str, on: bool) -> Result<(), AdapterError> {
    let object = root
        .as_object_mut()
        .ok_or_else(|| AdapterError::message("settings.json root must be an object"))?;
    let overrides = object
        .entry("skillOverrides")
        .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
    let overrides = overrides
        .as_object_mut()
        .ok_or_else(|| AdapterError::message("skillOverrides must be an object"))?;
    overrides.insert(
        name.to_string(),
        JsonValue::String(if on { "on" } else { "off" }.to_string()),
    );
    Ok(())
}

fn mcp_enabled(root: &mut JsonValue, id: &str, on: bool) -> Result<(), AdapterError> {
    let object = root
        .as_object_mut()
        .ok_or_else(|| AdapterError::message("claude.json root must be an object"))?;
    let servers = object
        .get_mut("mcpServers")
        .and_then(JsonValue::as_object_mut)
        .ok_or_else(|| AdapterError::message("mcpServers missing"))?;
    let entry = servers
        .get_mut(id)
        .and_then(JsonValue::as_object_mut)
        .ok_or_else(|| AdapterError::message(format!("mcp server not found: {id}")))?;
    entry.insert("disabled".to_string(), JsonValue::Bool(!on));
    let list = object
        .entry("disabledMcpServers")
        .or_insert_with(|| JsonValue::Array(Vec::new()));
    let list = list
        .as_array_mut()
        .ok_or_else(|| AdapterError::message("disabledMcpServers must be an array"))?;
    list.retain(|item| item.as_str() != Some(id));
    if !on {
        list.push(JsonValue::String(id.to_string()));
    }
    Ok(())
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
