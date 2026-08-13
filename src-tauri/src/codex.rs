use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;

use crate::adapter::AgentAdapter;
use crate::cli::{AgentCli, INSTALL_TIMEOUT};
use crate::config_io::ConfigIo;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto, ErrorKind, PluginDto, SkillDto};
use crate::install_source::{parse_install_source, InstallSource};
use crate::paths::{
    agent_info, agents_skills_root, codex_root, newest_dir, normalize_skill_path, plugin_id_parts,
};
use crate::scanner::{scan_plugin_skills, scan_skill_md, scan_user_skills, ScannedSkill};

#[derive(Debug, Deserialize, Default)]
struct CodexConfig {
    #[serde(default)]
    plugins: HashMap<String, CodexPluginEntry>,
    #[serde(default)]
    skills: CodexSkills,
}

#[derive(Debug, Deserialize, Default)]
struct CodexPluginEntry {
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Deserialize, Default)]
struct CodexSkills {
    #[serde(default)]
    config: Vec<CodexSkillRow>,
}

#[derive(Debug, Deserialize)]
struct CodexSkillRow {
    path: String,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

pub struct CodexAdapter {
    root: Option<PathBuf>,
    agents_skills: PathBuf,
    io: ConfigIo,
    write: Mutex<()>,
    cli: AgentCli,
}

impl CodexAdapter {
    pub fn new() -> Self {
        Self {
            root: codex_root().ok(),
            agents_skills: agents_skills_root().unwrap_or_else(|_| PathBuf::new()),
            io: ConfigIo::production(),
            write: Mutex::new(()),
            cli: AgentCli::new("codex"),
        }
    }

    #[cfg(test)]
    pub fn at(root: PathBuf, agents_skills: PathBuf) -> Self {
        Self::at_with_cli(root, agents_skills, AgentCli::new("on-n-off-no-such-codex.exe"))
    }

    #[cfg(test)]
    pub fn at_with_cli(root: PathBuf, agents_skills: PathBuf, cli: AgentCli) -> Self {
        let io = ConfigIo::at(root.join("_backups"));
        Self {
            root: Some(root),
            agents_skills,
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

    fn config(&self) -> Result<CodexConfig, AdapterError> {
        let path = self.root()?.join("config.toml");
        if !path.exists() {
            return Ok(CodexConfig::default());
        }
        let text = fs::read_to_string(&path).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })?;
        toml::from_str(&text).map_err(|error| AdapterError {
            kind: ErrorKind::Parse,
            message: error.to_string(),
            path: Some(path.display().to_string()),
        })
    }

    fn plugin_cache_dir(&self, plugin_id: &str) -> Option<PathBuf> {
        let (name, marketplace) = plugin_id.split_once('@')?;
        let cache = self.root().ok()?.join("plugins").join("cache").join(marketplace).join(name);
        newest_dir(&cache)
    }
}

impl AgentAdapter for CodexAdapter {
    fn info(&self) -> AgentInfo {
        agent_info(AgentId::Codex, "codex")
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        let config = self.config()?;
        let enable_by_path = skill_enable_map(&config);
        let mut plugins = Vec::new();
        let mut plugin_rows: Vec<_> = config.plugins.iter().collect();
        plugin_rows.sort_by(|a, b| a.0.cmp(b.0));
        let mut plugin_skill_paths = HashSet::new();
        for (id, entry) in plugin_rows {
            let (name, source) = plugin_id_parts(id);
            let skills: Vec<SkillDto> = self
                .plugin_cache_dir(id)
                .map(|dir| {
                    scan_plugin_skills(&dir)
                        .into_iter()
                        .map(|skill| {
                            plugin_skill_paths.insert(normalize_skill_path(&skill.skill_md.to_string_lossy()));
                            codex_skill(Some(id), skill, &enable_by_path)
                        })
                        .collect()
                })
                .unwrap_or_default();
            plugins.push(PluginDto {
                id: id.clone(),
                name,
                source,
                enabled: entry.enabled,
                skills,
            });
        }

        let mut user_skills = Vec::new();
        let mut seen = HashSet::new();
        for skill in scan_user_skills(&self.agents_skills) {
            let key = normalize_skill_path(&skill.skill_md.to_string_lossy());
            if plugin_skill_paths.contains(&key) || !seen.insert(key) {
                continue;
            }
            user_skills.push(codex_skill(None, skill, &enable_by_path));
        }
        for row in &config.skills.config {
            let path = PathBuf::from(&row.path);
            let skill_md = if path.is_dir() {
                path.join("SKILL.md")
            } else {
                path
            };
            let Some(skill) = scan_skill_md(&skill_md) else {
                continue;
            };
            let key = normalize_skill_path(&skill.skill_md.to_string_lossy());
            if plugin_skill_paths.contains(&key) || !seen.insert(key) {
                continue;
            }
            user_skills.push(codex_skill(None, skill, &enable_by_path));
        }
        user_skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(AgentTabDto { plugins, user_skills })
    }

    fn set_skill_enabled(&self, skill_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_togglable(skill_id)?;
        self.io.patch_toml_skill_enabled(
            AgentId::Codex,
            &self.root()?.join("config.toml"),
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
        self.io.patch_toml_plugin_enabled(
            AgentId::Codex,
            &self.root()?.join("config.toml"),
            plugin_id,
            enabled,
        )?;
        self.list_tab()
    }

    fn install_plugin(&self, source: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        let parsed = parse_install_source(source)?;
        self.io.backup_file(AgentId::Codex, &self.root()?.join("config.toml"))?;
        self.cli
            .run_args_timed(&parsed.codex_install_argv(), INSTALL_TIMEOUT)?;
        self.list_tab()
    }

    fn uninstall_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_plugin(plugin_id)?;
        self.io.backup_file(AgentId::Codex, &self.root()?.join("config.toml"))?;
        self.cli.run_args(&InstallSource::codex_uninstall_argv(plugin_id))?;
        self.list_tab()
    }
}

fn skill_enable_map(config: &CodexConfig) -> HashMap<String, bool> {
    config
        .skills
        .config
        .iter()
        .map(|row| (normalize_skill_path(&row.path), row.enabled))
        .collect()
}

fn codex_skill(plugin_id: Option<&str>, skill: ScannedSkill, enable_by_path: &HashMap<String, bool>) -> SkillDto {
    let id = normalize_skill_path(&skill.skill_md.to_string_lossy());
    let enabled = enable_by_path.get(&id).copied().unwrap_or(true);
    SkillDto {
        id,
        plugin_id: plugin_id.map(str::to_string),
        name: skill.name,
        description: skill.description,
        enabled,
        togglable: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, PathBuf) {
        let home = crate::paths::scratch_dir("on-n-off-codex-home");
        let root = home.join(".codex");
        let agents_skills = home.join(".agents").join("skills");
        let plugin = root.join("plugins/cache/workshop/workbench/0.22.1");
        fs::create_dir_all(plugin.join("skills/brainstorming")).unwrap();
        fs::write(
            plugin.join("skills/brainstorming/SKILL.md"),
            "---\nname: brainstorming\ndescription: Turn ideas into designs\n---\n",
        )
        .unwrap();
        fs::create_dir_all(agents_skills.join("loom-feed")).unwrap();
        fs::write(
            agents_skills.join("loom-feed/SKILL.md"),
            "---\nname: loom-feed\ndescription: Feed Loom\n---\n",
        )
        .unwrap();
        let extra = home.join("elsewhere/conoswiki-feed/SKILL.md");
        fs::create_dir_all(extra.parent().unwrap()).unwrap();
        fs::write(&extra, "---\nname: conoswiki-feed\ndescription: Feed ConosWiki\n---\n").unwrap();
        fs::write(
            root.join("config.toml"),
            format!(
                "[plugins.\"workbench@workshop\"]\nenabled = true\n\n[plugins.\"toolkit@workshop\"]\nenabled = false\n\n[[skills.config]]\npath = '{}'\nenabled = false\n",
                extra.display()
            ),
        )
        .unwrap();
        (root, agents_skills)
    }

    #[test]
    fn lists_plugins_and_user_skills_from_agents_dir_and_config_paths() {
        let (root, agents_skills) = fixture();
        let adapter = CodexAdapter::at(root.clone(), agents_skills);
        let tab = adapter.list_tab().expect("list");
        assert_eq!(tab.plugins[0].id, "toolkit@workshop");
        assert!(!tab.plugins[0].enabled);
        assert_eq!(tab.plugins[1].id, "workbench@workshop");
        assert!(tab.plugins[1].enabled);
        assert_eq!(tab.plugins[1].skills.len(), 1);
        let names: Vec<_> = tab.user_skills.iter().map(|skill| skill.name.as_str()).collect();
        assert_eq!(names, ["conoswiki-feed", "loom-feed"]);
        let wiki = tab.user_skills.iter().find(|skill| skill.name == "conoswiki-feed").unwrap();
        assert!(!wiki.enabled);
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn missing_inventory_is_empty_not_a_parse_error() {
        let home = crate::paths::scratch_dir("on-n-off-codex-empty");
        fs::create_dir_all(home.join(".codex")).unwrap();
        let tab = CodexAdapter::at(home.join(".codex"), home.join(".agents/skills"))
            .list_tab()
            .expect("list");
        assert!(tab.plugins.is_empty());
        assert!(tab.user_skills.is_empty());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn toggling_skill_upserts_config_and_keeps_plugins() {
        let (root, agents_skills) = fixture();
        let adapter = CodexAdapter::at(root.clone(), agents_skills);
        let tab = adapter.list_tab().expect("list");
        let loom = tab
            .user_skills
            .iter()
            .find(|skill| skill.name == "loom-feed")
            .unwrap();
        assert!(loom.enabled);
        let loom_id = loom.id.clone();
        let wiki_id = tab
            .user_skills
            .iter()
            .find(|skill| skill.name == "conoswiki-feed")
            .unwrap()
            .id
            .clone();

        let tab = adapter.set_skill_enabled(&loom_id, false).expect("toggle loom");
        let loom = tab.user_skills.iter().find(|skill| skill.name == "loom-feed").unwrap();
        assert!(!loom.enabled);
        let text = fs::read_to_string(root.join("config.toml")).unwrap();
        assert!(text.contains("[plugins.\"workbench@workshop\"]"));
        assert!(text.contains("[plugins.\"toolkit@workshop\"]"));
        assert!(text.contains("[[skills.config]]"));
        assert!(text.contains("loom-feed"));

        let tab = adapter.set_skill_enabled(&wiki_id, true).expect("toggle wiki");
        let wiki = tab
            .user_skills
            .iter()
            .find(|skill| skill.name == "conoswiki-feed")
            .unwrap();
        assert!(wiki.enabled);
        assert!(root.join("_backups/codex").read_dir().unwrap().next().is_some());
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn toggling_plugin_patches_toml_enabled_only() {
        let (root, agents_skills) = fixture();
        let adapter = CodexAdapter::at(root.clone(), agents_skills);
        let tab = adapter
            .set_plugin_enabled("workbench@workshop", false)
            .expect("disable");
        let plugin = tab.plugins.iter().find(|plugin| plugin.id == "workbench@workshop").unwrap();
        assert!(!plugin.enabled);
        let text = fs::read_to_string(root.join("config.toml")).unwrap();
        assert!(text.contains("[plugins.\"toolkit@workshop\"]"));
        assert!(text.contains("[[skills.config]]"));
        assert!(root.join("_backups/codex").read_dir().unwrap().next().is_some());
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    fn codex_argv_stub(root: &Path, exit: i32, stderr: &str) -> AgentCli {
        let dir = root.join("_cli");
        fs::create_dir_all(&dir).unwrap();
        let body = if exit == 0 {
            "@echo off\r\necho %* > \"%~dp0args.txt\"\r\nexit /b 0\r\n".to_string()
        } else {
            format!("@echo off\r\necho {stderr} 1>&2\r\nexit /b {exit}\r\n")
        };
        let bin = dir.join("codex.cmd");
        fs::write(&bin, body).unwrap();
        AgentCli::new(bin.to_string_lossy().as_ref())
    }

    #[test]
    fn install_and_uninstall_use_official_argv() {
        let (root, agents_skills) = fixture();
        let adapter = CodexAdapter::at_with_cli(root.clone(), agents_skills, codex_argv_stub(&root, 0, ""));
        adapter.install_plugin("workbench@workshop").expect("add");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(args.contains("plugin add --json workbench@workshop"), "{args}");
        adapter.install_plugin("acme/tools@main").expect("market");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(args.contains("plugin marketplace add --json acme/tools --ref main"), "{args}");
        adapter.uninstall_plugin("workbench@workshop").expect("remove");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(args.contains("plugin remove --json workbench@workshop"), "{args}");
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn install_cli_failure_does_not_mutate_config() {
        let (root, agents_skills) = fixture();
        let before = fs::read_to_string(root.join("config.toml")).unwrap();
        let adapter = CodexAdapter::at_with_cli(root.clone(), agents_skills, codex_argv_stub(&root, 3, "nope"));
        let err = adapter.install_plugin("acme/tools").expect_err("cli");
        assert!(err.message.contains("nope"));
        assert_eq!(fs::read_to_string(root.join("config.toml")).unwrap(), before);
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }
}
