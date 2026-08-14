use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;

use crate::adapter::AgentAdapter;
use crate::cli::{run_npx_skills, AgentCli, INSTALL_TIMEOUT};
use crate::config_io::ConfigIo;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto, ErrorKind, PluginDto, SkillDto};
use crate::install_source::{parse_install_source, InstallSource};
use crate::mcp::parse_claude_json;
use crate::paths::{agent_info, claude_root, plugin_id_parts};
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
}

impl AgentAdapter for ClaudeAdapter {
    fn info(&self) -> AgentInfo {
        agent_info(AgentId::Claude, "claude")
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        let settings = self.settings()?;
        let catalogs: HashMap<String, HashMap<String, crate::plugin_meta::VersionHint>> = self
            .marketplace_roots()
            .into_iter()
            .map(|(name, (path, origin))| {
                let mut hints = crate::plugin_meta::catalog_hints(&path);
                crate::plugin_meta::apply_remote_marketplace_versions(&mut hints, &origin, &path);
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
            if let Some(hint) = catalog.as_mut() {
                crate::plugin_meta::fill_remote_version(hint);
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
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        let root = crate::paths::scratch_dir("on-n-off-claude");
        let plugin = root.join("plugins/cache/workshop/workbench/1.0.0");
        fs::create_dir_all(plugin.join("skills/brainstorming")).unwrap();
        fs::write(
            plugin.join("skills/brainstorming/SKILL.md"),
            "---\nname: brainstorming\ndescription: Turn ideas into designs\n---\n",
        )
        .unwrap();
        let opt_in = root.join("plugins/cache/opt/quiet/1.0.0");
        fs::create_dir_all(opt_in.join(".claude-plugin")).unwrap();
        fs::write(
            opt_in.join(".claude-plugin/plugin.json"),
            r#"{"name":"quiet","defaultEnabled":false}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("plugins/marketplaces/workshop/.claude-plugin")).unwrap();
        fs::write(
            root.join("plugins/marketplaces/workshop/.claude-plugin/marketplace.json"),
            r#"{"plugins":[{"name":"workbench","version":"1.1.0","source":"./plugins/workbench"}]}"#,
        )
        .unwrap();
        fs::write(
            root.join("plugins/known_marketplaces.json"),
            serde_json::json!({
                "workshop": {
                    "installLocation": root.join("plugins/marketplaces/workshop")
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(
            root.join("plugins/installed_plugins.json"),
            serde_json::json!({
                "version": 2,
                "plugins": {
                    "workbench@workshop": [{
                        "scope": "user",
                        "installPath": plugin,
                        "version": "1.0.0"
                    }],
                    "superpowers@claude-plugins-official": [{
                        "scope": "user",
                        "installPath": root.join("plugins/cache/missing")
                    }],
                    "quiet@opt": [{
                        "scope": "user",
                        "installPath": opt_in
                    }],
                    "project-only@team": [{
                        "scope": "project",
                        "installPath": root.join("plugins/cache/team/project-only/1.0.0")
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            root.join("settings.json"),
            serde_json::json!({
                "enabledPlugins": {
                    "workbench@workshop": true,
                    "superpowers@claude-plugins-official": false
                },
                "skillOverrides": { "statusline": "off" }
            })
            .to_string(),
        )
        .unwrap();
        fs::create_dir_all(root.join("skills/statusline")).unwrap();
        fs::write(
            root.join("skills/statusline/SKILL.md"),
            "---\nname: statusline\ndescription: Custom status line\n---\n",
        )
        .unwrap();
        fs::write(
            root.join(".claude.json"),
            serde_json::json!({
                "mcpServers": {
                    "github": {
                        "command": "npx",
                        "args": ["-y", "@modelcontextprotocol/server-github"]
                    },
                    "Docs": { "type": "http", "url": "https://docs.example/mcp" }
                }
            })
            .to_string(),
        )
        .unwrap();
        root
    }

    #[test]
    fn lists_user_scope_plugins_using_default_enabled_when_absent() {
        let root = fixture();
        let adapter = ClaudeAdapter::at(root.clone());
        let tab = adapter.list_tab().expect("list");
        let ids: Vec<_> = tab
            .plugins
            .iter()
            .map(|plugin| plugin.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "superpowers@claude-plugins-official",
                "quiet@opt",
                "workbench@workshop"
            ]
        );
        let quiet = tab
            .plugins
            .iter()
            .find(|plugin| plugin.id == "quiet@opt")
            .unwrap();
        let superpowers = tab
            .plugins
            .iter()
            .find(|plugin| plugin.id == "superpowers@claude-plugins-official")
            .unwrap();
        let workbench = tab
            .plugins
            .iter()
            .find(|plugin| plugin.id == "workbench@workshop")
            .unwrap();
        assert!(!quiet.enabled);
        assert!(!superpowers.enabled);
        assert!(workbench.enabled);
        assert_eq!(workbench.skills[0].id, "workbench@workshop:brainstorming");
        assert!(!workbench.skills[0].togglable);
        assert_eq!(quiet.version, "1.0.0");
        assert_eq!(superpowers.version, "");
        assert_eq!(workbench.version, "1.0.0");
        assert_eq!(quiet.upstream, "");
        assert_eq!(superpowers.upstream, "");
        assert_eq!(workbench.upstream, "1.1.0");
        assert!(workbench.out_of_sync);
        assert_eq!(tab.user_skills[0].id, "statusline");
        assert!(!tab.user_skills[0].enabled);
        let mcp_ids: Vec<_> = tab
            .mcp_servers
            .iter()
            .map(|server| server.id.as_str())
            .collect();
        assert_eq!(mcp_ids, ["Docs", "github"]);
        assert_eq!(tab.mcp_servers[0].system, "http");
        assert!(tab.mcp_servers[1].enabled);
        assert!(tab.mcp_servers[1].togglable);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_inventory_is_empty_not_a_parse_error() {
        let root = crate::paths::scratch_dir("on-n-off-claude-empty");
        let tab = ClaudeAdapter::at(root.clone()).list_tab().expect("list");
        assert!(tab.plugins.is_empty());
        assert!(tab.user_skills.is_empty());
        assert!(tab.mcp_servers.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn toggling_user_skill_patches_skill_overrides_and_refuses_plugin_skills() {
        let root = fixture();
        let adapter = ClaudeAdapter::at(root.clone());
        let err = adapter
            .set_skill_enabled("workbench@workshop:brainstorming", false)
            .expect_err("locked");
        assert!(err.message.contains("not togglable"));
        let settings = fs::read_to_string(root.join("settings.json")).unwrap();
        assert!(!settings.contains("brainstorming"));

        let tab = adapter
            .set_skill_enabled("statusline", true)
            .expect("toggle");
        assert!(tab.user_skills[0].enabled);
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("settings.json")).unwrap()).unwrap();
        assert_eq!(value["skillOverrides"]["statusline"], "on");
        assert_eq!(value["enabledPlugins"]["workbench@workshop"], true);
        assert!(root
            .join("_backups/claude")
            .read_dir()
            .unwrap()
            .next()
            .is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn toggling_mcp_patches_claude_json_without_dropping_other_servers() {
        let root = fixture();
        let adapter = ClaudeAdapter::at(root.clone());
        let err = adapter
            .set_mcp_enabled("missing", false)
            .expect_err("missing");
        assert!(err.message.contains("mcp server not found"));
        let tab = adapter.set_mcp_enabled("github", false).expect("toggle");
        let github = tab
            .mcp_servers
            .iter()
            .find(|server| server.id == "github")
            .unwrap();
        assert!(!github.enabled);
        assert!(github.togglable);
        let docs = tab
            .mcp_servers
            .iter()
            .find(|server| server.id == "Docs")
            .unwrap();
        assert!(docs.enabled);
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(value["mcpServers"]["github"]["disabled"], true);
        assert_eq!(value["mcpServers"]["github"]["command"], "npx");
        assert_eq!(
            value["mcpServers"]["Docs"]["url"],
            "https://docs.example/mcp"
        );
        assert_eq!(value["disabledMcpServers"], serde_json::json!(["github"]));
        let tab = adapter.set_mcp_enabled("github", true).expect("on");
        assert!(
            tab.mcp_servers
                .iter()
                .find(|server| server.id == "github")
                .unwrap()
                .enabled
        );
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(value["mcpServers"]["github"]["disabled"], false);
        assert_eq!(value["disabledMcpServers"], serde_json::json!([]));
        let _ = fs::remove_dir_all(root);
    }

    fn claude_stub(root: &Path, after_settings: &str, exit: i32, stderr: &str) -> AgentCli {
        let dir = root.join("_cli");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("after.json"), after_settings).unwrap();
        let body = if exit == 0 {
            "@echo off\r\ncopy /Y \"%~dp0after.json\" \"%~dp0..\\settings.json\" >nul\r\necho %* > \"%~dp0args.txt\"\r\nexit /b 0\r\n".to_string()
        } else {
            format!("@echo off\r\necho {stderr} 1>&2\r\nexit /b {exit}\r\n")
        };
        let bin = dir.join("claude.cmd");
        fs::write(&bin, body).unwrap();
        AgentCli::new(bin.to_string_lossy().as_ref())
    }

    #[test]
    fn plugin_disable_via_cli_refreshes_dto_and_backs_up() {
        let root = fixture();
        let after = serde_json::json!({
            "enabledPlugins": {
                "workbench@workshop": false,
                "superpowers@claude-plugins-official": false
            },
            "skillOverrides": { "statusline": "off" }
        })
        .to_string();
        let adapter = ClaudeAdapter::at_with_cli(root.clone(), claude_stub(&root, &after, 0, ""));
        let tab = adapter
            .set_plugin_enabled("workbench@workshop", false)
            .expect("disable");
        let plugin = tab
            .plugins
            .iter()
            .find(|plugin| plugin.id == "workbench@workshop")
            .unwrap();
        assert!(!plugin.enabled);
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(
            args.contains("plugin disable -s user workbench@workshop"),
            "{args}"
        );
        assert!(root
            .join("_backups/claude")
            .read_dir()
            .unwrap()
            .next()
            .is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plugin_cli_failure_does_not_mutate_settings() {
        let root = fixture();
        let before = fs::read_to_string(root.join("settings.json")).unwrap();
        let adapter = ClaudeAdapter::at_with_cli(root.clone(), claude_stub(&root, "{}", 2, "nope"));
        let err = adapter
            .set_plugin_enabled("workbench@workshop", false)
            .expect_err("cli");
        assert!(err.message.contains("nope"));
        assert_eq!(
            fs::read_to_string(root.join("settings.json")).unwrap(),
            before
        );
        let _ = fs::remove_dir_all(root);
    }

    fn claude_argv_stub(root: &Path, exit: i32, stderr: &str) -> AgentCli {
        let dir = root.join("_cli");
        fs::create_dir_all(&dir).unwrap();
        let body = if exit == 0 {
            "@echo off\r\necho %* > \"%~dp0args.txt\"\r\nexit /b 0\r\n".to_string()
        } else {
            format!("@echo off\r\necho {stderr} 1>&2\r\nexit /b {exit}\r\n")
        };
        let bin = dir.join("claude.cmd");
        fs::write(&bin, body).unwrap();
        AgentCli::new(bin.to_string_lossy().as_ref())
    }

    #[test]
    fn install_plugin_id_and_git_source_use_official_argv() {
        let root = fixture();
        let adapter = ClaudeAdapter::at_with_cli(root.clone(), claude_argv_stub(&root, 0, ""));
        adapter
            .install_plugin("workbench@workshop")
            .expect("plugin");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(
            args.contains("plugin install -s user -y workbench@workshop"),
            "{args}"
        );
        adapter.install_plugin("acme/tools").expect("git");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(
            args.contains("plugin marketplace add --scope user acme/tools"),
            "{args}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn install_cli_failure_does_not_mutate_settings() {
        let root = fixture();
        let before = fs::read_to_string(root.join("settings.json")).unwrap();
        let adapter =
            ClaudeAdapter::at_with_cli(root.clone(), claude_argv_stub(&root, 2, "denied"));
        let err = adapter.install_plugin("acme/tools").expect_err("cli");
        assert!(err.message.contains("denied"));
        assert_eq!(
            fs::read_to_string(root.join("settings.json")).unwrap(),
            before
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn update_refreshes_marketplace_then_plugin() {
        let root = fixture();
        let dir = root.join("_cli");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("claude.cmd"),
            "@echo off\r\necho %* >> \"%~dp0args.txt\"\r\nexit /b 0\r\n",
        )
        .unwrap();
        let adapter = ClaudeAdapter::at_with_cli(
            root.clone(),
            AgentCli::new(dir.join("claude.cmd").to_string_lossy().as_ref()),
        );
        adapter.update_plugin("workbench@workshop").expect("update");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(
            args.contains("plugin marketplace update workshop"),
            "{args}"
        );
        assert!(
            args.contains("plugin update -s user -y workbench@workshop"),
            "{args}"
        );
        assert!(root
            .join("_backups/claude")
            .read_dir()
            .unwrap()
            .next()
            .is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn uninstall_uses_official_argv_and_backs_up() {
        let root = fixture();
        let adapter = ClaudeAdapter::at_with_cli(root.clone(), claude_argv_stub(&root, 0, ""));
        adapter
            .uninstall_plugin("workbench@workshop")
            .expect("uninstall");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(
            args.contains("plugin uninstall -s user -y workbench@workshop"),
            "{args}"
        );
        assert!(root
            .join("_backups/claude")
            .read_dir()
            .unwrap()
            .next()
            .is_some());
        let _ = fs::remove_dir_all(root);
    }
}
