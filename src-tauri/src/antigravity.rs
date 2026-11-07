use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::{AgentAdapter, ItemRoots};
use crate::capabilities::{Capabilities, Change};
use crate::change::{switch, Checked, Step};
use crate::cli::{argv, AgentCli};
use crate::dto::{AdapterError, AgentId, AgentTabDto, PluginDto, SkillDto};
use crate::install_source::InstallSource;
use crate::mcp::{parse_antigravity_json, McpFormat};
use crate::paths::{
    antigravity_cli_plugins, antigravity_cli_root, antigravity_cli_skills,
    antigravity_config_plugins, antigravity_mcp_config, normalize_skill_path, plugin_id_parts,
};
use crate::project::{relative, McpFile, PluginFolders, Projects};
use crate::scanner::{scan_antigravity_skills, scan_plugin_skills, ScannedSkill, SkillFiles};
use crate::sort::sort_tab;

const ENABLEMENT: [&str; 3] = ["config.json", "plugins.json", "settings.json"];

const SOURCE_CLI: &str = "cli";
const SOURCE_CONFIG: &str = "config";
const SOURCE_WORKSPACE: &str = "workspace";

#[derive(Debug, Clone)]
struct DiscoveredPlugin {
    id: String,
    name: String,
    source: String,
    path: PathBuf,
}

pub struct AntigravityAdapter {
    gemini: Option<PathBuf>,
    cli: AgentCli,
}

impl AntigravityAdapter {
    pub fn new() -> Self {
        Self {
            gemini: crate::paths::gemini_root().ok(),
            cli: AgentCli::new(AgentId::Antigravity.binary_name()),
        }
    }

    #[cfg(test)]
    pub fn at(gemini: PathBuf) -> Self {
        Self::at_with_cli(gemini, AgentCli::new("on-n-off-no-such-agy.exe"))
    }

    #[cfg(test)]
    pub fn at_with_cli(gemini: PathBuf, cli: AgentCli) -> Self {
        Self {
            gemini: Some(gemini),
            cli,
        }
    }

    fn cli_root(&self) -> Result<PathBuf, AdapterError> {
        match &self.gemini {
            Some(gemini) => Ok(gemini.join("antigravity-cli")),
            None => antigravity_cli_root(),
        }
    }

    fn config_plugins_dir(&self) -> Result<PathBuf, AdapterError> {
        match &self.gemini {
            Some(gemini) => Ok(gemini.join("config").join("plugins")),
            None => antigravity_config_plugins(),
        }
    }

    fn cli_plugins_dir(&self) -> Result<PathBuf, AdapterError> {
        match &self.gemini {
            Some(gemini) => Ok(gemini.join("antigravity-cli").join("plugins")),
            None => antigravity_cli_plugins(),
        }
    }

    fn mcp_path(&self) -> Result<PathBuf, AdapterError> {
        match &self.gemini {
            Some(gemini) => Ok(gemini.join("config").join("mcp_config.json")),
            None => antigravity_mcp_config(),
        }
    }

    fn skills_dir(&self) -> Result<PathBuf, AdapterError> {
        match &self.gemini {
            Some(gemini) => Ok(gemini.join("antigravity-cli").join("skills")),
            None => antigravity_cli_skills(),
        }
    }

    fn enablement_files(&self) -> Result<Vec<PathBuf>, AdapterError> {
        let root = self.cli_root()?;
        Ok(ENABLEMENT.iter().map(|name| root.join(name)).collect())
    }

    fn enablement(&self) -> HashMap<String, bool> {
        let Ok(files) = self.enablement_files() else {
            return HashMap::new();
        };
        let mut map = HashMap::new();
        for path in files {
            if let Ok(text) = fs::read_to_string(&path) {
                merge_enablement(&mut map, &text);
            }
        }
        map
    }

    fn discover_plugins(&self) -> Result<Vec<DiscoveredPlugin>, AdapterError> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for (dir, source) in [
            (self.cli_plugins_dir()?, SOURCE_CLI),
            (self.config_plugins_dir()?, SOURCE_CONFIG),
        ] {
            for plugin in scan_plugin_dirs(&dir, source) {
                if seen.insert(plugin.id.clone()) {
                    out.push(plugin);
                }
            }
        }
        Ok(out)
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
}

impl AgentAdapter for AntigravityAdapter {
    fn id(&self) -> AgentId {
        AgentId::Antigravity
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            plugin_switch: Change::cli("agy plugin enable/disable"),
            skill_switch: Change::not_implemented("skill toggle"),
            mcp_switch: Change::file("file patch · mcpServers.*.disabled"),
            install: Change::cli("agy plugin install"),
            refused_sources: BTreeMap::new(),
            uninstall: Change::cli("agy plugin uninstall"),
            uninstall_from: Some(vec![SOURCE_CLI.to_string()]),
            update: Change::not_implemented("update"),
            reads_hooks: false,
        }
    }

    fn global_item_roots(&self) -> Result<ItemRoots, AdapterError> {
        Ok(ItemRoots {
            skills: self.skills_dir()?,
            agents: None,
        })
    }

    fn projects(&self) -> Projects {
        Projects {
            items: ItemRoots {
                skills: relative(&[".agents", "skills"]),
                agents: None,
            },
            more_skills: Vec::new(),
            skill_files: SkillFiles::FoldersAndMarkdown,
            plugins: Some(PluginFolders {
                dirs: vec![
                    relative(&[".agents", "plugins"]),
                    relative(&["_agents", "plugins"]),
                ],
                read: workspace_plugins,
            }),
            mcp: vec![
                McpFile::mcp_json(),
                McpFile::at(&[".agents", "mcp_config.json"], McpFormat::AntigravityJson),
            ],
            record: None,
        }
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        let enablement = self.enablement();
        let mut plugins = Vec::new();
        let mut plugin_skill_ids = HashSet::new();

        for discovered in self.discover_plugins()? {
            let enabled = enablement
                .get(&discovered.name)
                .or_else(|| enablement.get(&discovered.id))
                .copied()
                .unwrap_or(true);
            let plugin = plugin_row(discovered, enabled);
            plugin_skill_ids.extend(plugin.skills.iter().map(|skill| skill.id.clone()));
            plugins.push(plugin);
        }

        let mut user_skills = Vec::new();
        let mut seen = HashSet::new();
        if let Ok(dir) = self.skills_dir() {
            for skill in scan_antigravity_skills(&dir) {
                let skill = antigravity_skill(None, skill);
                if plugin_skill_ids.contains(&skill.id) || !seen.insert(skill.id.clone()) {
                    continue;
                }
                user_skills.push(skill);
            }
        }

        let mut tab = AgentTabDto {
            plugins,
            user_skills,
            mcp_servers: self.mcp_servers(),
            hooks: Vec::new(),
        };
        sort_tab(&mut tab);
        Ok(tab)
    }

    fn step(&self, change: &Checked) -> Result<Option<Step<'_>>, AdapterError> {
        let enablement = self.enablement_files()?;
        let cli = |runs| Step::cli(&self.cli, enablement.clone(), runs);
        Ok(Some(match change {
            Checked::Plugin { plugin, on } => {
                let (name, _) = plugin_id_parts(&plugin.id);
                cli(vec![argv(&["plugin", switch(*on), &name])])
            }
            Checked::Mcp { server, on } => {
                let (id, on) = (server.id.clone(), *on);
                Step::json(self.mcp_path()?, move |root| mcp_enabled(root, &id, on))
            }
            Checked::Install(source @ InstallSource::NpxSkills { .. }) => {
                Step::npx(source.clone(), "antigravity")
            }
            Checked::Install(source) => {
                cli(vec![argv(&["plugin", "install", &source.as_cli_source()])])
            }
            Checked::Uninstall(plugin) => {
                let (name, _) = plugin_id_parts(&plugin.id);
                cli(vec![argv(&["plugin", "uninstall", &name])])
            }
            Checked::Skill { .. } | Checked::Update(_) => return Ok(None),
        }))
    }
}

fn mcp_enabled(root: &mut serde_json::Value, id: &str, on: bool) -> Result<(), AdapterError> {
    let object = root
        .as_object_mut()
        .ok_or_else(|| AdapterError::message("mcp_config.json root must be an object"))?;
    let servers = object
        .get_mut("mcpServers")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| AdapterError::message("mcpServers missing"))?;
    let entry = servers
        .get_mut(id)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| AdapterError::message(format!("mcp server not found: {id}")))?;
    entry.insert("disabled".to_string(), serde_json::Value::Bool(!on));
    Ok(())
}

fn antigravity_skill(plugin_id: Option<&str>, skill: ScannedSkill) -> SkillDto {
    SkillDto {
        id: normalize_skill_path(&skill.skill_md.to_string_lossy()),
        plugin_id: plugin_id.map(str::to_string),
        name: skill.name,
        description: skill.description,
        enabled: true,
        togglable: true,
        origin: String::new(),
    }
}

fn plugin_row(discovered: DiscoveredPlugin, enabled: bool) -> PluginDto {
    let skills = scan_plugin_skills(&discovered.path)
        .into_iter()
        .map(|skill| antigravity_skill(Some(&discovered.id), skill))
        .collect();
    PluginDto {
        togglable: discovered.source == SOURCE_CLI,
        source: discovered.source,
        id: discovered.id,
        name: discovered.name,
        version: String::new(),
        upstream: String::new(),
        out_of_sync: false,
        enabled,
        skills,
    }
}

fn scan_plugin_dirs(parent: &Path, source: &str) -> Vec<DiscoveredPlugin> {
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let marker = path.join("plugin.json");
        if !marker.is_file() {
            continue;
        }
        let folder = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("plugin")
            .to_string();
        let name = read_plugin_name(&marker).unwrap_or_else(|| folder.clone());
        out.push(DiscoveredPlugin {
            id: format!("{name}@{source}"),
            name,
            source: source.to_string(),
            path,
        });
    }
    out
}

fn read_plugin_name(plugin_json: &Path) -> Option<String> {
    let text = fs::read_to_string(plugin_json).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn merge_enablement(map: &mut HashMap<String, bool>, text: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    if let Some(plugins) = value.get("plugins").and_then(|value| value.as_object()) {
        for (name, entry) in plugins {
            if let Some(enabled) = entry.get("enabled").and_then(|value| value.as_bool()) {
                map.insert(name.clone(), enabled);
            } else if let Some(enabled) = entry.as_bool() {
                map.insert(name.clone(), enabled);
            }
        }
    }
    if let Some(list) = value
        .get("disabledPlugins")
        .and_then(|value| value.as_array())
    {
        for item in list {
            if let Some(name) = item.as_str() {
                map.insert(name.to_string(), false);
            }
        }
    }
    if let Some(list) = value
        .get("enabledPlugins")
        .and_then(|value| value.as_array())
    {
        for item in list {
            if let Some(name) = item.as_str() {
                map.insert(name.to_string(), true);
            }
        }
    }
    if let Some(imports) = value.get("imports").and_then(|value| value.as_array()) {
        for item in imports {
            let Some(name) = item.get("name").and_then(|value| value.as_str()) else {
                continue;
            };
            let enabled = item
                .get("enabled")
                .and_then(|value| value.as_bool())
                .unwrap_or_else(|| {
                    !item
                        .get("disabled")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                });
            map.insert(name.to_string(), enabled);
        }
    }
}

fn workspace_plugins(dir: &Path) -> Vec<PluginDto> {
    scan_plugin_dirs(dir, SOURCE_WORKSPACE)
        .into_iter()
        .map(|discovered| plugin_row(discovered, true))
        .collect()
}

#[cfg(test)]
mod tests;
