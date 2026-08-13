use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;

use crate::adapter::AgentAdapter;
use crate::cli::AgentCli;
use crate::config_io::ConfigIo;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto, ErrorKind, PluginDto, SkillDto};
use crate::paths::{agent_info, claude_root, plugin_id_parts};
use crate::scanner::{scan_plugin_skills, scan_user_skills, ScannedSkill};

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

fn default_true() -> bool {
    true
}

pub struct ClaudeAdapter {
    root: Option<PathBuf>,
    io: ConfigIo,
    write: Mutex<()>,
    cli: AgentCli,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self {
            root: claude_root().ok(),
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
        let io = ConfigIo::at(root.join("_backups"));
        Self {
            root: Some(root),
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

    fn installed(&self) -> Result<Vec<(String, PathBuf)>, AdapterError> {
        let path = self.root()?.join("plugins").join("installed_plugins.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&path).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })?;
        let parsed: InstalledPluginsFile = serde_json::from_str(&text).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })?;
        let mut plugins = Vec::new();
        for (id, entries) in parsed.plugins {
            let Some(entry) = entries.into_iter().find(|entry| {
                entry.scope.as_deref().is_none_or(|scope| scope == "user")
            }) else {
                continue;
            };
            plugins.push((id, entry.install_path));
        }
        plugins.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(plugins)
    }
}

impl AgentAdapter for ClaudeAdapter {
    fn info(&self) -> AgentInfo {
        agent_info(AgentId::Claude, "claude")
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        let settings = self.settings()?;
        let mut plugins = Vec::new();
        for (id, install_path) in self.installed()? {
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
            plugins.push(PluginDto {
                id,
                name,
                source,
                enabled,
                skills,
            });
        }
        let user_skills = scan_user_skills(&self.root()?.join("skills"))
            .into_iter()
            .map(|skill| claude_user_skill(skill, &settings.skill_overrides))
            .collect();
        Ok(AgentTabDto { plugins, user_skills })
    }

    fn set_skill_enabled(&self, skill_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
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

    fn set_plugin_enabled(&self, plugin_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
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
    }
}

fn claude_user_skill(skill: ScannedSkill, overrides: &HashMap<String, String>) -> SkillDto {
    let enabled = match overrides.get(&skill.name).map(String::as_str) {
        Some("off") => false,
        _ => true,
    };
    SkillDto {
        id: skill.name.clone(),
        plugin_id: None,
        name: skill.name,
        description: skill.description,
        enabled,
        togglable: true,
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
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(
            root.join("plugins/installed_plugins.json"),
            serde_json::json!({
                "version": 2,
                "plugins": {
                    "workbench@workshop": [{
                        "scope": "user",
                        "installPath": plugin
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
        root
    }

    #[test]
    fn lists_user_scope_plugins_using_default_enabled_when_absent() {
        let root = fixture();
        let adapter = ClaudeAdapter::at(root.clone());
        let tab = adapter.list_tab().expect("list");
        let ids: Vec<_> = tab.plugins.iter().map(|plugin| plugin.id.as_str()).collect();
        assert_eq!(ids, ["quiet@opt", "superpowers@claude-plugins-official", "workbench@workshop"]);
        assert!(!tab.plugins[0].enabled);
        assert!(!tab.plugins[1].enabled);
        assert!(tab.plugins[2].enabled);
        assert_eq!(tab.plugins[2].skills[0].id, "workbench@workshop:brainstorming");
        assert!(!tab.plugins[2].skills[0].togglable);
        assert_eq!(tab.user_skills[0].id, "statusline");
        assert!(!tab.user_skills[0].enabled);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_inventory_is_empty_not_a_parse_error() {
        let root = crate::paths::scratch_dir("on-n-off-claude-empty");
        let tab = ClaudeAdapter::at(root.clone()).list_tab().expect("list");
        assert!(tab.plugins.is_empty());
        assert!(tab.user_skills.is_empty());
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

        let tab = adapter.set_skill_enabled("statusline", true).expect("toggle");
        assert!(tab.user_skills[0].enabled);
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("settings.json")).unwrap()).unwrap();
        assert_eq!(value["skillOverrides"]["statusline"], "on");
        assert_eq!(value["enabledPlugins"]["workbench@workshop"], true);
        assert!(root.join("_backups/claude").read_dir().unwrap().next().is_some());
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
        let plugin = tab.plugins.iter().find(|plugin| plugin.id == "workbench@workshop").unwrap();
        assert!(!plugin.enabled);
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(args.contains("plugin disable -s user workbench@workshop"), "{args}");
        assert!(root.join("_backups/claude").read_dir().unwrap().next().is_some());
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
        assert_eq!(fs::read_to_string(root.join("settings.json")).unwrap(), before);
        let _ = fs::remove_dir_all(root);
    }
}
