use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;

use crate::adapter::{AgentAdapter, ItemRoots};
use crate::cli::{AgentCli, INSTALL_TIMEOUT};
use crate::config_io::ConfigIo;
use crate::dto::{
    AdapterError, AgentId, AgentInfo, AgentTabDto, ErrorKind, ItemScope, McpServerDto, PluginDto,
    SkillDto,
};
use crate::install_source::{parse_install_source, InstallSource};
use crate::mcp::parse_antigravity_json;
use crate::paths::agents_skills_root;
use crate::paths::plugin_id_parts;
use crate::scanner::{scan_plugin_skills, scan_user_skills, ScannedSkill};
use crate::sort::sort_tab;

pub const MCP_READ_ONLY: &str = "Muse manages MCP servers itself: edit `mcpServers` in its settings.json or use `muse mcp login <server>`; on-n-off only lists them.";

const SOURCE_LOCAL: &str = "local";
const ACTIVATION_OFF: &str = "off";

#[derive(Debug, Deserialize, Default)]
struct InstalledFile {
    #[serde(default)]
    plugins: BTreeMap<String, InstalledRecord>,
}

#[derive(Debug, Deserialize, Default)]
struct InstalledRecord {
    #[serde(default)]
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    source: RecordSource,
    #[serde(default)]
    cache_path: PathBuf,
}

#[derive(Debug, Deserialize, Default)]
struct RecordSource {
    #[serde(default)]
    path: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Default)]
struct MarketplacesFile {
    #[serde(default)]
    marketplaces: BTreeMap<String, MarketplaceRecord>,
}

#[derive(Debug, Deserialize, Default)]
struct MarketplaceRecord {
    #[serde(default)]
    source: RecordSource,
    #[serde(default)]
    snapshot_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Default)]
struct SnapshotFile {
    #[serde(default)]
    plugins: Vec<SnapshotPlugin>,
}

#[derive(Debug, Deserialize, Default)]
struct SnapshotPlugin {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: Option<String>,
}

struct MarketplaceHint {
    marketplace: String,
    source: PathBuf,
    version: Option<String>,
}

fn default_true() -> bool {
    true
}

pub struct MuseAdapter {
    config: Option<PathBuf>,
    data: Option<PathBuf>,
    agents_skills: Option<PathBuf>,
    io: ConfigIo,
    write: Mutex<()>,
    cli: AgentCli,
}

impl MuseAdapter {
    pub fn new() -> Self {
        Self {
            config: crate::paths::muse_config_root().ok(),
            data: crate::paths::muse_data_root().ok(),
            agents_skills: agents_skills_root().ok(),
            io: ConfigIo::production(),
            write: Mutex::new(()),
            cli: AgentCli::new(AgentId::Muse.binary_name()),
        }
    }

    #[cfg(test)]
    pub fn at(home: PathBuf) -> Self {
        Self::at_with_cli(home, AgentCli::new("on-n-off-no-such-muse.exe"))
    }

    #[cfg(test)]
    pub fn at_with_cli(home: PathBuf, cli: AgentCli) -> Self {
        let io = ConfigIo::at(home.join("_backups"));
        Self {
            config: Some(crate::paths::muse_config_for(&home)),
            data: Some(crate::paths::muse_data_for(&home)),
            agents_skills: Some(home.join(".agents").join("skills")),
            io,
            write: Mutex::new(()),
            cli,
        }
    }

    fn config_root(&self) -> Result<&Path, AdapterError> {
        self.config
            .as_deref()
            .ok_or_else(|| AdapterError::message("home directory not found"))
    }

    fn data_root(&self) -> Result<&Path, AdapterError> {
        self.data
            .as_deref()
            .ok_or_else(|| AdapterError::message("home directory not found"))
    }

    fn settings_text(&self) -> Result<Option<String>, AdapterError> {
        let path = self.config_root()?.join("settings.json");
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })?;
        serde_json::from_str::<serde_json::Value>(&text).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })?;
        Ok(Some(text))
    }

    fn installed(&self) -> Result<Vec<(String, InstalledRecord)>, AdapterError> {
        let path = self.data_root()?.join("plugins").join("installed.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&path).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })?;
        let parsed: InstalledFile = serde_json::from_str(&text).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })?;
        Ok(parsed
            .plugins
            .into_iter()
            .map(|(key, mut record)| {
                if record.id.trim().is_empty() {
                    record.id = key.clone();
                }
                (key, record)
            })
            .collect())
    }

    fn marketplace_hints(&self) -> HashMap<String, Vec<MarketplaceHint>> {
        let mut hints = HashMap::new();
        let Ok(data) = self.data_root() else {
            return hints;
        };
        let Ok(text) = fs::read_to_string(data.join("plugins").join("marketplaces.json")) else {
            return hints;
        };
        let Ok(file) = serde_json::from_str::<MarketplacesFile>(&text) else {
            return hints;
        };
        for (name, record) in file.marketplaces {
            let Some(source) = record.source.path else {
                continue;
            };
            let Some(snapshot_path) = &record.snapshot_path else {
                continue;
            };
            let snapshot_path = if snapshot_path.is_absolute() {
                snapshot_path.clone()
            } else {
                data.join(snapshot_path)
            };
            let Ok(text) = fs::read_to_string(&snapshot_path) else {
                continue;
            };
            let Ok(snapshot) = serde_json::from_str::<SnapshotFile>(&text) else {
                continue;
            };
            for plugin in snapshot.plugins {
                hints
                    .entry(plugin.name)
                    .or_insert_with(Vec::new)
                    .push(MarketplaceHint {
                        marketplace: name.clone(),
                        source: source.clone(),
                        version: plugin.version,
                    });
            }
        }
        hints
    }

    fn mcp_servers(&self, settings_text: Option<&str>) -> Vec<McpServerDto> {
        let Some(text) = settings_text else {
            return Vec::new();
        };
        parse_antigravity_json(text)
            .into_iter()
            .map(|mut server| {
                server.togglable = false;
                server
            })
            .collect()
    }
}

fn activation_map(settings_text: Option<&str>) -> HashMap<String, HashMap<String, String>> {
    let Some(text) = settings_text else {
        return HashMap::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return HashMap::new();
    };
    let Some(activation) = value
        .get("skills")
        .and_then(|skills| skills.get("activation"))
        .and_then(|activation| activation.as_object())
    else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for (scope, entries) in activation {
        let mut map = HashMap::new();
        if let Some(entries) = entries.as_object() {
            for (key, state) in entries {
                if let Some(state) = state.as_str() {
                    map.insert(key.clone(), state.to_string());
                }
            }
        }
        out.insert(scope.clone(), map);
    }
    out
}

fn is_enabled(
    activation: &HashMap<String, HashMap<String, String>>,
    scope: &str,
    display_path: &str,
) -> bool {
    !matches!(
        activation
            .get(scope)
            .and_then(|entries| entries.get(display_path))
            .map(String::as_str),
        Some(ACTIVATION_OFF)
    )
}

fn display_rel(root: &Path, skill_md: &Path) -> Option<String> {
    let rel = skill_md.strip_prefix(root).ok()?;
    let parts: Vec<_> = rel
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn muse_plugin_skill(
    dto_plugin_id: &str,
    slug: &str,
    plugin_enabled: bool,
    root: &Path,
    skill: ScannedSkill,
    activation: &HashMap<String, HashMap<String, String>>,
) -> SkillDto {
    let key = display_rel(root, &skill.skill_md)
        .map(|rel| format!("plugin://{slug}/{rel}"))
        .unwrap_or_default();
    SkillDto {
        id: format!("plugin:{slug}:{}", skill.name),
        plugin_id: Some(dto_plugin_id.to_string()),
        name: skill.name,
        description: skill.description,
        enabled: plugin_enabled && is_enabled(activation, "plugin", &key),
        togglable: plugin_enabled,
        origin: String::new(),
    }
}

fn muse_user_skill(
    display_path: String,
    skill: ScannedSkill,
    activation: &HashMap<String, HashMap<String, String>>,
) -> SkillDto {
    SkillDto {
        id: skill.name.clone(),
        plugin_id: None,
        name: skill.name,
        description: skill.description,
        enabled: is_enabled(activation, "user", &display_path),
        togglable: true,
        origin: String::new(),
    }
}

impl AgentAdapter for MuseAdapter {
    fn supports_accounts(&self) -> bool {
        true
    }

    fn info(&self) -> AgentInfo {
        let mut info = crate::cli_locate::agent_info(AgentId::Muse);
        info.install_git = false;
        info
    }

    fn item_roots(&self, scope: &ItemScope) -> Result<ItemRoots, AdapterError> {
        match scope {
            ItemScope::Global => Ok(ItemRoots {
                skills: self.config_root()?.join("skills"),
                agents: None,
            }),
            ItemScope::Project { project_path } => Ok(crate::project::project_item_roots(
                Path::new(project_path),
                AgentId::Muse,
            )),
        }
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        let settings_text = self.settings_text()?;
        let activation = activation_map(settings_text.as_deref());
        let hints = self.marketplace_hints();
        let records = self.installed()?;

        let mut plugins = Vec::new();
        for (_, record) in records {
            let slug = record.id.clone();
            let hint = hints.get(&slug).and_then(|candidates| {
                candidates.iter().find(|hint| {
                    record
                        .source
                        .path
                        .as_ref()
                        .is_some_and(|source| source.starts_with(&hint.source))
                })
            });
            let (source, upstream) = hint.map_or_else(
                || (SOURCE_LOCAL.to_string(), String::new()),
                |hint| {
                    (
                        hint.marketplace.clone(),
                        hint.version.clone().unwrap_or_default(),
                    )
                },
            );
            let dto_id = format!("{slug}@{source}");
            let skills = scan_plugin_skills(&record.cache_path)
                .into_iter()
                .map(|skill| {
                    muse_plugin_skill(
                        &dto_id,
                        &slug,
                        record.enabled,
                        &record.cache_path,
                        skill,
                        &activation,
                    )
                })
                .collect();
            let installed =
                crate::plugin_meta::installed_hint(&record.cache_path, record.version.as_deref());
            let (version, _, _) = crate::plugin_meta::resolve_versions(&installed, None);
            let out_of_sync = crate::plugin_meta::out_of_sync(&version, &upstream);
            plugins.push(PluginDto {
                id: dto_id,
                name: slug,
                source,
                version,
                upstream,
                out_of_sync,
                enabled: record.enabled,
                togglable: true,
                skills,
            });
        }

        let mut user_skills = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        if let Ok(config) = self.config_root() {
            for skill in scan_user_skills(&config.join("skills")) {
                let display = display_rel(config, &skill.skill_md)
                    .map(|rel| format!("$CONFIG_DIR/{rel}"))
                    .unwrap_or_default();
                if seen.insert(skill.name.clone()) {
                    user_skills.push(muse_user_skill(display, skill, &activation));
                }
            }
        }
        if let Some(agents_skills) = &self.agents_skills {
            for skill in scan_user_skills(agents_skills) {
                let folder = skill
                    .skill_md
                    .parent()
                    .and_then(|dir| dir.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let display = format!("$HOME/.agents/skills/{folder}/SKILL.md");
                if seen.insert(skill.name.clone()) {
                    user_skills.push(muse_user_skill(display, skill, &activation));
                }
            }
        }

        let mut tab = AgentTabDto {
            plugins,
            user_skills,
            mcp_servers: self.mcp_servers(settings_text.as_deref()),
            hooks: Vec::new(),
        };
        sort_tab(&mut tab);
        Ok(tab)
    }

    fn set_mcp_enabled(&self, _mcp_id: &str, _enabled: bool) -> Result<AgentTabDto, AdapterError> {
        Err(AdapterError::message(MCP_READ_ONLY))
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
        self.list_tab()?.ensure_plugin_togglable(plugin_id)?;
        self.backup_state()?;
        let (name, _) = plugin_id_parts(plugin_id);
        let action = if enabled { "enable" } else { "disable" };
        self.cli.run(&["plugins", action, &name, "--json"])?;
        self.list_tab()
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
        let tab = self.list_tab()?;
        tab.ensure_togglable(skill_id)?;
        let scope = if tab
            .skill(skill_id)
            .is_some_and(|skill| skill.plugin_id.is_some())
        {
            "plugin"
        } else {
            "user"
        };
        self.backup_state()?;
        let action = if enabled { "enable" } else { "disable" };
        self.cli
            .run(&["skills", action, skill_id, "--scope", scope, "--json"])?;
        self.list_tab()
    }

    fn install_plugin(&self, source: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        let parsed = parse_install_source(source)?;
        match &parsed {
            InstallSource::LocalDir(_) | InstallSource::Plugin(_) => {}
            InstallSource::NpxSkills { .. } => {
                return Err(AdapterError::message(
                    "npx skills cannot target Muse; install the skill from a marketplace or a folder instead.",
                ));
            }
            InstallSource::GitUrl(_) | InstallSource::GitHub { .. } => {
                return Err(AdapterError::message(
                    "Muse marketplaces need a name: run `muse plugins marketplace add <name> <source>` in a terminal, then install <plugin>@<name> here.",
                ));
            }
        }
        self.backup_state()?;
        self.cli
            .run_args_timed(&parsed.muse_install_argv(), INSTALL_TIMEOUT)?;
        self.list_tab()
    }

    fn uninstall_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_plugin(plugin_id)?;
        self.backup_state()?;
        self.cli
            .run_args(&InstallSource::muse_uninstall_argv(plugin_id))?;
        self.list_tab()
    }

    fn update_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_plugin(plugin_id)?;
        self.backup_state()?;
        match InstallSource::plugin_marketplace(plugin_id) {
            Some(marketplace) => {
                self.cli.run_args_timed(
                    &InstallSource::muse_marketplace_update_argv(marketplace),
                    INSTALL_TIMEOUT,
                )?;
                let source = InstallSource::Plugin(plugin_id.to_string());
                self.cli
                    .run_args_timed(&source.muse_install_argv(), INSTALL_TIMEOUT)?;
            }
            None => {
                self.cli
                    .run_args_timed(&InstallSource::muse_update_argv(plugin_id), INSTALL_TIMEOUT)?;
            }
        }
        self.list_tab()
    }
}

impl MuseAdapter {
    fn backup_state(&self) -> Result<(), AdapterError> {
        let settings = self.config_root()?.join("settings.json");
        self.io.backup_file(AgentId::Muse, &settings)?;
        let installed = self.data_root()?.join("plugins").join("installed.json");
        self.io.backup_file(AgentId::Muse, &installed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
