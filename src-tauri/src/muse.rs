use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::adapter::{AgentAdapter, ItemRoots};
use crate::capabilities::{Capabilities, Change};
use crate::change::{switch, Checked, Step};
use crate::cli::{argv, AgentCli};
use crate::drift::Marketplace;
use crate::dto::{AdapterError, AgentId, AgentTabDto, ErrorKind, PluginDto, SkillDto};
use crate::install_source::{InstallSource, SourceKind};
use crate::mcp::parse_antigravity_json;
use crate::paths::agents_skills_root;
use crate::paths::plugin_id_parts;
use crate::project::{relative, McpFile, Projects};
use crate::scanner::{scan_plugin_skills, scan_user_skills, ScannedSkill, SkillFiles};
use crate::sort::sort_tab;

pub const MCP_READ_ONLY: &str = "Muse manages MCP servers itself, so on-n-off lists them but cannot switch them here. Edit `mcpServers` in Muse’s settings.json, or use `muse mcp login <name>`.";

const MARKETPLACE_NEEDS_A_NAME: &str = "Muse marketplaces need a name: run `muse plugins marketplace add <name> <source>` in a terminal, then install <plugin>@<name> here.";

const NO_NPX_SKILLS: &str =
    "npx skills cannot target Muse; install the skill from a marketplace or a folder instead.";

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

pub struct MuseAdapter {
    config: Option<PathBuf>,
    data: Option<PathBuf>,
    agents_skills: Option<PathBuf>,
    cli: AgentCli,
}

impl MuseAdapter {
    pub fn new() -> Self {
        Self {
            config: crate::paths::muse_config_root().ok(),
            data: crate::paths::muse_data_root().ok(),
            agents_skills: agents_skills_root().ok(),
            cli: AgentCli::new(AgentId::Muse.binary_name()),
        }
    }

    #[cfg(test)]
    pub fn at(home: PathBuf) -> Self {
        Self::at_with_cli(home, AgentCli::new("on-n-off-no-such-muse.exe"))
    }

    #[cfg(test)]
    pub fn at_with_cli(home: PathBuf, cli: AgentCli) -> Self {
        Self {
            config: Some(crate::paths::muse_config_for(&home)),
            data: Some(crate::paths::muse_data_for(&home)),
            agents_skills: Some(home.join(".agents").join("skills")),
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

    fn documents(&self) -> Result<Documents, AdapterError> {
        let plugins = self.data_root()?.join("plugins");
        Ok(Documents {
            settings: self.config_root()?.join("settings.json"),
            installed: plugins.join("installed.json"),
            marketplaces: plugins.join("marketplaces.json"),
        })
    }

    fn settings_text(&self) -> Result<Option<String>, AdapterError> {
        let path = self.documents()?.settings;
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
        let path = self.documents()?.installed;
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

    fn snapshots(&self) -> Vec<(String, PathBuf, SnapshotFile)> {
        let (Ok(data), Ok(documents)) = (self.data_root(), self.documents()) else {
            return Vec::new();
        };
        let Ok(text) = fs::read_to_string(documents.marketplaces) else {
            return Vec::new();
        };
        let Ok(file) = serde_json::from_str::<MarketplacesFile>(&text) else {
            return Vec::new();
        };
        let mut snapshots = Vec::new();
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
            snapshots.push((name, source, snapshot));
        }
        snapshots
    }

    fn marketplace_hints(&self) -> HashMap<String, Vec<MarketplaceHint>> {
        let mut hints = HashMap::new();
        for (name, source, snapshot) in self.snapshots() {
            for plugin in snapshot.plugins {
                hints
                    .entry(plugin.name)
                    .or_insert_with(Vec::new)
                    .push(MarketplaceHint {
                        marketplace: name.clone(),
                        source: source.clone(),
                    });
            }
        }
        hints
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

    fn id(&self) -> AgentId {
        AgentId::Muse
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            plugin_switch: Change::cli("muse plugins enable/disable"),
            skill_switch: Change::cli("muse skills enable/disable"),
            mcp_switch: Change::refused("managed in Muse · read-only here", MCP_READ_ONLY),
            install: Change::cli("muse plugins install"),
            refused_sources: BTreeMap::from([
                (
                    SourceKind::GitMarketplace,
                    MARKETPLACE_NEEDS_A_NAME.to_string(),
                ),
                (SourceKind::MarketplaceSkill, NO_NPX_SKILLS.to_string()),
            ]),
            uninstall: Change::cli("muse plugins remove"),
            uninstall_from: None,
            update: Change::cli("muse plugins update"),
            reads_hooks: false,
        }
    }

    fn global_item_roots(&self) -> Result<ItemRoots, AdapterError> {
        Ok(ItemRoots {
            skills: self.config_root()?.join("skills"),
            agents: None,
        })
    }

    fn projects(&self) -> Projects {
        Projects {
            items: ItemRoots {
                skills: relative(&[".agents", "skills"]),
                agents: None,
            },
            more_skills: vec![
                relative(&[".claude", "skills"]),
                relative(&[".codex", "skills"]),
            ],
            skill_files: SkillFiles::Folders,
            plugins: None,
            mcp: vec![McpFile::mcp_json()],
            record: None,
        }
    }

    fn marketplaces(&self) -> Vec<Marketplace> {
        self.snapshots()
            .into_iter()
            .map(|(name, _, snapshot)| {
                Marketplace::listing(
                    name,
                    snapshot
                        .plugins
                        .into_iter()
                        .map(|plugin| (plugin.name, plugin.version)),
                )
            })
            .collect()
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
            let source =
                hint.map_or_else(|| SOURCE_LOCAL.to_string(), |hint| hint.marketplace.clone());
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
            plugins.push(PluginDto {
                id: dto_id,
                name: slug,
                source,
                version: crate::drift::installed(&record.cache_path, record.version.as_deref()),
                upstream: String::new(),
                out_of_sync: false,
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
            mcp_servers: settings_text
                .as_deref()
                .map(parse_antigravity_json)
                .unwrap_or_default(),
            hooks: Vec::new(),
        };
        sort_tab(&mut tab);
        Ok(tab)
    }

    fn step(&self, change: &Checked) -> Result<Option<Step<'_>>, AdapterError> {
        let documents = self.documents()?.all();
        let cli = |runs| Step::cli(&self.cli, documents, runs);
        let name = |id: &str| plugin_id_parts(id).0;
        Ok(Some(match change {
            Checked::Plugin { plugin, on } => cli(vec![argv(&[
                "plugins",
                switch(*on),
                &name(&plugin.id),
                "--json",
            ])]),
            Checked::Skill { skill, on } => {
                let scope = if skill.plugin_id.is_some() {
                    "plugin"
                } else {
                    "user"
                };
                cli(vec![argv(&[
                    "skills",
                    switch(*on),
                    &skill.id,
                    "--scope",
                    scope,
                    "--json",
                ])])
            }
            Checked::Install(InstallSource::LocalDir(path)) => cli(vec![argv(&[
                "plugins",
                "install",
                &path.display().to_string(),
                "--scope",
                "user",
                "--json",
            ])]),
            Checked::Install(source) => cli(vec![argv(&[
                "plugins",
                "install",
                &source.as_cli_source(),
                "--json",
            ])]),
            Checked::Uninstall(plugin) => cli(vec![argv(&[
                "plugins",
                "remove",
                &name(&plugin.id),
                "--json",
            ])]),
            Checked::Update(plugin) => match InstallSource::plugin_marketplace(&plugin.id) {
                Some(marketplace) => cli(vec![
                    argv(&["plugins", "marketplace", "update", marketplace, "--json"]),
                    argv(&["plugins", "install", &plugin.id, "--json"]),
                ]),
                None => cli(vec![argv(&[
                    "plugins",
                    "update",
                    &name(&plugin.id),
                    "--json",
                ])]),
            },
            Checked::Mcp { .. } => return Ok(None),
        }))
    }
}

#[cfg(test)]
mod tests;
