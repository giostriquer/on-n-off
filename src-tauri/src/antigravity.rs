use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::adapter::AgentAdapter;
use crate::cli::{run_npx_skills, AgentCli, INSTALL_TIMEOUT};
use crate::cli_locate::agent_info;
use crate::config_io::ConfigIo;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto, PluginDto, SkillDto};
use crate::install_source::{parse_install_source, InstallSource};
use crate::mcp::parse_antigravity_json;
use crate::paths::{
    antigravity_cli_plugins, antigravity_cli_root, antigravity_cli_skills,
    antigravity_config_plugins, antigravity_mcp_config, normalize_skill_path, plugin_id_parts,
};
use crate::scanner::{scan_antigravity_skills, scan_plugin_skills, ScannedSkill};
use crate::sort::sort_tab;

const SOURCE_CLI: &str = "cli";
const SOURCE_CONFIG: &str = "config";

#[derive(Debug, Clone)]
struct DiscoveredPlugin {
    id: String,
    name: String,
    source: String,
    path: PathBuf,
}

pub struct AntigravityAdapter {
    gemini: Option<PathBuf>,
    io: ConfigIo,
    write: Mutex<()>,
    cli: AgentCli,
}

impl AntigravityAdapter {
    pub fn new() -> Self {
        Self {
            gemini: crate::paths::gemini_root().ok(),
            io: ConfigIo::production(),
            write: Mutex::new(()),
            cli: AgentCli::new("agy"),
        }
    }

    #[cfg(test)]
    pub fn at(gemini: PathBuf) -> Self {
        Self::at_with_cli(gemini, AgentCli::new("on-n-off-no-such-agy.exe"))
    }

    #[cfg(test)]
    pub fn at_with_cli(gemini: PathBuf, cli: AgentCli) -> Self {
        let io = ConfigIo::at(gemini.join("_backups"));
        Self {
            gemini: Some(gemini),
            io,
            write: Mutex::new(()),
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

    fn enablement(&self) -> HashMap<String, bool> {
        let Ok(root) = self.cli_root() else {
            return HashMap::new();
        };
        let mut map = HashMap::new();
        for name in ["config.json", "plugins.json", "settings.json"] {
            let path = root.join(name);
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
    fn info(&self) -> AgentInfo {
        agent_info(AgentId::Antigravity, "agy")
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        let enablement = self.enablement();
        let mut plugins = Vec::new();
        let mut plugin_skill_paths = HashSet::new();

        for discovered in self.discover_plugins()? {
            let enabled = enablement
                .get(&discovered.name)
                .or_else(|| enablement.get(&discovered.id))
                .copied()
                .unwrap_or(true);
            let skills: Vec<SkillDto> = scan_plugin_skills(&discovered.path)
                .into_iter()
                .map(|skill| {
                    plugin_skill_paths
                        .insert(normalize_skill_path(&skill.skill_md.to_string_lossy()));
                    antigravity_skill(Some(&discovered.id), skill)
                })
                .collect();
            plugins.push(PluginDto {
                id: discovered.id,
                name: discovered.name,
                source: discovered.source.clone(),
                version: String::new(),
                upstream: String::new(),
                out_of_sync: false,
                enabled,
                togglable: discovered.source == SOURCE_CLI,
                skills,
            });
        }

        let mut user_skills = Vec::new();
        let mut seen = HashSet::new();
        if let Ok(dir) = self.skills_dir() {
            for skill in scan_antigravity_skills(&dir) {
                let key = normalize_skill_path(&skill.skill_md.to_string_lossy());
                if plugin_skill_paths.contains(&key) || !seen.insert(key) {
                    continue;
                }
                user_skills.push(antigravity_skill(None, skill));
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

    fn set_plugin_enabled(
        &self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        let _plugin = self.list_tab()?.ensure_plugin_togglable(plugin_id)?;
        let (name, _) = plugin_id_parts(plugin_id);
        let action = if enabled { "enable" } else { "disable" };
        let _ = self
            .io
            .backup_file(AgentId::Antigravity, &self.cli_root()?.join("config.json"));
        self.cli.run(&["plugin", action, &name])?;
        self.list_tab()
    }

    fn set_mcp_enabled(&self, mcp_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_mcp_togglable(mcp_id)?;
        let path = self.mcp_path()?;
        self.io
            .patch_antigravity_mcp_enabled(AgentId::Antigravity, &path, mcp_id, enabled)?;
        self.list_tab()
    }

    fn install_plugin(&self, source: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        let parsed = parse_install_source(source)?;
        if parsed.is_npx_skills() {
            run_npx_skills(&parsed, "antigravity")?;
            return self.list_tab();
        }
        let _ = self
            .io
            .backup_file(AgentId::Antigravity, &self.cli_root()?.join("config.json"));
        self.cli
            .run_args_timed(&parsed.agy_install_argv(), INSTALL_TIMEOUT)?;
        self.list_tab()
    }

    fn uninstall_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        let plugin = self.list_tab()?.ensure_plugin(plugin_id)?.clone();
        if plugin.source != SOURCE_CLI {
            return Err(AdapterError::message(format!(
                "plugin cannot be uninstalled here: {plugin_id}"
            )));
        }
        let _ = self
            .io
            .backup_file(AgentId::Antigravity, &self.cli_root()?.join("config.json"));
        self.cli
            .run_args(&InstallSource::agy_uninstall_argv(plugin_id))?;
        self.list_tab()
    }
}

fn antigravity_skill(plugin_id: Option<&str>, skill: ScannedSkill) -> SkillDto {
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

/// Scan workspace plugin folders for project overlay.
pub fn scan_workspace_plugins(project: &Path) -> Vec<PluginDto> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for rel in [
        Path::new(".agents").join("plugins"),
        Path::new("_agents").join("plugins"),
    ] {
        let dir = project.join(&rel);
        for discovered in scan_plugin_dirs(&dir, "workspace") {
            if !seen.insert(discovered.id.clone()) {
                continue;
            }
            let skills = scan_plugin_skills(&discovered.path)
                .into_iter()
                .map(|skill| {
                    let mut dto = antigravity_skill(Some(&discovered.id), skill);
                    dto.origin = crate::project::ORIGIN_PROJECT.to_string();
                    dto
                })
                .collect();
            out.push(PluginDto {
                id: format!("project:{}", discovered.id),
                name: discovered.name,
                source: discovered.source,
                version: String::new(),
                upstream: String::new(),
                out_of_sync: false,
                enabled: true,
                togglable: false,
                skills,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_stub::CliStub;

    fn write_plugin(root: &Path, name: &str, skill: Option<&str>) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.json"), format!(r#"{{"name":"{name}"}}"#)).unwrap();
        if let Some(skill_name) = skill {
            let skill_dir = dir.join("skills").join(skill_name);
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {skill_name}\ndescription: Test\n---\n"),
            )
            .unwrap();
        }
    }

    #[test]
    fn lists_cli_and_config_plugins_with_skills_and_mcp() {
        let gemini = crate::paths::scratch_dir("on-n-off-agy-list");
        write_plugin(
            &gemini.join("antigravity-cli").join("plugins"),
            "alpha",
            Some("pack"),
        );
        write_plugin(&gemini.join("config").join("plugins"), "beta", None);
        fs::create_dir_all(gemini.join("antigravity-cli").join("skills")).unwrap();
        fs::write(
            gemini
                .join("antigravity-cli")
                .join("skills")
                .join("solo.md"),
            "---\nname: solo\ndescription: Flat\n---\n",
        )
        .unwrap();
        fs::create_dir_all(gemini.join("config")).unwrap();
        fs::write(
            gemini.join("config").join("mcp_config.json"),
            r#"{"mcpServers":{"docs":{"serverUrl":"https://docs.example/mcp/"}}}"#,
        )
        .unwrap();
        fs::write(
            gemini.join("antigravity-cli").join("config.json"),
            r#"{"plugins":{"alpha":{"enabled":false}}}"#,
        )
        .unwrap();

        let tab = AntigravityAdapter::at(gemini.clone())
            .list_tab()
            .expect("list");
        let alpha = tab.plugins.iter().find(|p| p.id == "alpha@cli").unwrap();
        assert!(!alpha.enabled);
        assert_eq!(alpha.skills.len(), 1);
        let beta = tab.plugins.iter().find(|p| p.id == "beta@config").unwrap();
        assert!(beta.enabled);
        assert!(tab.user_skills.iter().any(|s| s.name == "solo"));
        assert!(!tab.user_skills[0].togglable);
        assert_eq!(tab.mcp_servers.len(), 1);
        assert_eq!(tab.mcp_servers[0].id, "docs");
        let _ = fs::remove_dir_all(gemini);
    }

    #[test]
    fn enable_uses_agy_for_cli_plugins_and_blocks_config() {
        let gemini = crate::paths::scratch_dir("on-n-off-agy-toggle");
        write_plugin(
            &gemini.join("antigravity-cli").join("plugins"),
            "alpha",
            None,
        );
        write_plugin(&gemini.join("config").join("plugins"), "beta", None);
        let argv = gemini.join("argv.txt");
        let cli = CliStub::new("agy").log_args("argv.txt", false).cli(&gemini);

        let adapter = AntigravityAdapter::at_with_cli(gemini.clone(), cli);
        adapter
            .set_plugin_enabled("alpha@cli", false)
            .expect("toggle");
        let logged = fs::read_to_string(&argv).unwrap();
        assert!(logged.contains("plugin"), "{logged}");
        assert!(logged.contains("disable"), "{logged}");
        assert!(logged.contains("alpha"), "{logged}");

        let err = adapter
            .set_plugin_enabled("beta@config", false)
            .expect_err("config not togglable");
        assert!(err.message.contains("not togglable"));
        let _ = fs::remove_dir_all(gemini);
    }

    #[test]
    fn merge_enablement_reads_disabled_list_and_imports() {
        let mut map = HashMap::new();
        merge_enablement(
            &mut map,
            r#"{
                "disabledPlugins": ["a"],
                "imports": [{"name":"b","disabled":true},{"name":"c"}]
            }"#,
        );
        assert_eq!(map.get("a"), Some(&false));
        assert_eq!(map.get("b"), Some(&false));
        assert_eq!(map.get("c"), Some(&true));
    }

    #[test]
    fn workspace_plugin_scan_prefixes_project() {
        let root = crate::paths::scratch_dir("on-n-off-agy-ws");
        write_plugin(&root.join(".agents").join("plugins"), "ws", Some("local"));
        let plugins = scan_workspace_plugins(&root);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "project:ws@workspace");
        assert_eq!(plugins[0].skills[0].origin, crate::project::ORIGIN_PROJECT);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_toggle_patches_disabled() {
        let gemini = crate::paths::scratch_dir("on-n-off-agy-mcp-toggle");
        fs::create_dir_all(gemini.join("config")).unwrap();
        fs::write(
            gemini.join("config").join("mcp_config.json"),
            r#"{"mcpServers":{"github":{"command":"npx","args":["-y","gh"]}}}"#,
        )
        .unwrap();
        let adapter = AntigravityAdapter::at(gemini.clone());
        let tab = adapter.set_mcp_enabled("github", false).expect("toggle");
        assert!(!tab.mcp_servers[0].enabled);
        let text = fs::read_to_string(gemini.join("config").join("mcp_config.json")).unwrap();
        assert!(text.contains("\"disabled\": true") || text.contains("\"disabled\":true"));
        let _ = fs::remove_dir_all(gemini);
    }

    #[test]
    fn uninstall_refuses_config_plugins() {
        let gemini = crate::paths::scratch_dir("on-n-off-agy-uninstall");
        write_plugin(&gemini.join("config").join("plugins"), "beta", None);
        let err = AntigravityAdapter::at(gemini.clone())
            .uninstall_plugin("beta@config")
            .expect_err("config");
        assert!(err.message.contains("cannot be uninstalled"));
        let _ = fs::remove_dir_all(gemini);
    }
}
