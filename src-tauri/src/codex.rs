use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;

use crate::adapter::AgentAdapter;
use crate::cli::{run_npx_skills, AgentCli, INSTALL_TIMEOUT};
use crate::config_io::ConfigIo;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto, ErrorKind, PluginDto, SkillDto};
use crate::install_source::{parse_install_source, InstallSource};
use crate::mcp::{parse_codex_map, CodexMcpEntry};
use crate::paths::{
    agent_info, agents_skills_root, codex_root, newest_dir, normalize_skill_path, plugin_id_parts,
};
use crate::scanner::{scan_plugin_skills, scan_skill_md, scan_user_skills, ScannedSkill};
use crate::sort::sort_tab;

#[derive(Debug, Deserialize, Default)]
struct CodexConfig {
    #[serde(default)]
    plugins: HashMap<String, CodexPluginEntry>,
    #[serde(default)]
    skills: CodexSkills,
    #[serde(default)]
    mcp_servers: HashMap<String, CodexMcpEntry>,
    #[serde(default)]
    marketplaces: HashMap<String, CodexMarketplace>,
}

#[derive(Debug, Deserialize, Default)]
struct CodexMarketplace {
    #[serde(default)]
    source_type: Option<String>,
    #[serde(default)]
    source: Option<String>,
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

fn marketplace_path(root: &Path, name: &str, entry: &CodexMarketplace) -> Option<PathBuf> {
    let source = entry.source.as_deref().unwrap_or("");
    let source_type = entry.source_type.as_deref().unwrap_or("");
    if source_type == "local" || looks_like_path(source) {
        return Some(crate::plugin_meta::strip_verbatim(source));
    }
    if source_type == "git"
        || source.starts_with("http")
        || source.to_ascii_lowercase().ends_with(".git")
    {
        return Some(root.join(".tmp").join("marketplaces").join(name));
    }
    None
}

fn looks_like_path(source: &str) -> bool {
    let source = source.trim();
    source.starts_with(r"\\")
        || source.starts_with('/')
        || (source.len() > 1 && source.as_bytes().get(1) == Some(&b':'))
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
        Self::at_with_cli(
            root,
            agents_skills,
            AgentCli::new("on-n-off-no-such-codex.exe"),
        )
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

    fn marketplace_roots(&self, config: &CodexConfig) -> HashMap<String, (PathBuf, String)> {
        let Ok(root) = self.root() else {
            return HashMap::new();
        };
        let mut roots = HashMap::new();
        for (name, entry) in &config.marketplaces {
            if let Some(path) = marketplace_path(root, name, entry) {
                roots.insert(
                    name.clone(),
                    (path, entry.source.clone().unwrap_or_default()),
                );
            }
        }
        roots
    }

    fn plugin_cache_dir(&self, plugin_id: &str) -> Option<PathBuf> {
        let (name, marketplace) = plugin_id.split_once('@')?;
        let cache = self
            .root()
            .ok()?
            .join("plugins")
            .join("cache")
            .join(marketplace)
            .join(name);
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
        let catalogs: HashMap<String, HashMap<String, crate::plugin_meta::VersionHint>> = self
            .marketplace_roots(&config)
            .into_iter()
            .map(|(name, (path, origin))| {
                let mut hints = crate::plugin_meta::catalog_hints(&path);
                crate::plugin_meta::apply_remote_marketplace_versions(&mut hints, &origin, &path);
                (name, hints)
            })
            .collect();
        let mut plugins = Vec::new();
        let plugin_rows: Vec<_> = config.plugins.iter().collect();
        let mut plugin_skill_paths = HashSet::new();
        for (id, entry) in plugin_rows {
            let (name, source) = plugin_id_parts(id);
            let cache = self.plugin_cache_dir(id);
            let installed = cache
                .as_ref()
                .map(|dir| crate::plugin_meta::installed_hint(dir, None))
                .unwrap_or_default();
            let mut catalog = catalogs
                .get(&source)
                .and_then(|hints| hints.get(&name))
                .cloned();
            if let Some(hint) = catalog.as_mut() {
                crate::plugin_meta::fill_remote_version(hint);
            }
            let (version, upstream, out_of_sync) =
                crate::plugin_meta::resolve_versions(&installed, catalog.as_ref());
            let skills: Vec<SkillDto> = cache
                .map(|dir| {
                    scan_plugin_skills(&dir)
                        .into_iter()
                        .map(|skill| {
                            plugin_skill_paths
                                .insert(normalize_skill_path(&skill.skill_md.to_string_lossy()));
                            codex_skill(Some(id), skill, &enable_by_path)
                        })
                        .collect()
                })
                .unwrap_or_default();
            plugins.push(PluginDto {
                id: id.clone(),
                name,
                source,
                version,
                upstream,
                out_of_sync,
                enabled: entry.enabled,
                togglable: true,
                skills,
            });
        }

        let mut user_skills = Vec::new();
        let mut seen = HashSet::new();
        let mut skill_dirs = vec![self.agents_skills.clone()];
        if let Ok(root) = self.root() {
            let codex_skills = root.join("skills");
            if codex_skills != self.agents_skills {
                skill_dirs.push(codex_skills);
            }
        }
        for dir in &skill_dirs {
            for skill in scan_user_skills(dir) {
                let key = normalize_skill_path(&skill.skill_md.to_string_lossy());
                if plugin_skill_paths.contains(&key) || !seen.insert(key) {
                    continue;
                }
                user_skills.push(codex_skill(None, skill, &enable_by_path));
            }
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
        let mut tab = AgentTabDto {
            plugins,
            user_skills,
            mcp_servers: parse_codex_map(&config.mcp_servers),
        };
        sort_tab(&mut tab);
        Ok(tab)
    }

    fn list_projects(&self) -> Vec<crate::dto::ProjectDto> {
        let Ok(root) = self.root() else {
            return Vec::new();
        };
        let text = fs::read_to_string(root.join("config.toml")).unwrap_or_default();
        crate::project::inspect_projects(
            crate::project::parse_codex_projects(&text),
            AgentId::Codex,
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
        self.io.patch_toml_skill_enabled(
            AgentId::Codex,
            &self.root()?.join("config.toml"),
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
        self.io.patch_toml_mcp_enabled(
            AgentId::Codex,
            &self.root()?.join("config.toml"),
            mcp_id,
            enabled,
        )?;
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
        if parsed.is_npx_skills() {
            run_npx_skills(&parsed, "codex")?;
            return self.list_tab();
        }
        self.io
            .backup_file(AgentId::Codex, &self.root()?.join("config.toml"))?;
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
        self.io
            .backup_file(AgentId::Codex, &self.root()?.join("config.toml"))?;
        self.cli
            .run_args(&InstallSource::codex_uninstall_argv(plugin_id))?;
        self.list_tab()
    }

    fn update_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let _guard = self
            .write
            .lock()
            .map_err(|_| AdapterError::message("write lock poisoned"))?;
        self.list_tab()?.ensure_plugin(plugin_id)?;
        self.io
            .backup_file(AgentId::Codex, &self.root()?.join("config.toml"))?;
        if let Some(marketplace) = InstallSource::plugin_marketplace(plugin_id) {
            self.cli.run_args_timed(
                &InstallSource::codex_marketplace_upgrade_argv(marketplace),
                INSTALL_TIMEOUT,
            )?;
        }
        self.cli.run_args_timed(
            &InstallSource::codex_update_argv(plugin_id),
            INSTALL_TIMEOUT,
        )?;
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

fn codex_skill(
    plugin_id: Option<&str>,
    skill: ScannedSkill,
    enable_by_path: &HashMap<String, bool>,
) -> SkillDto {
    let id = normalize_skill_path(&skill.skill_md.to_string_lossy());
    let enabled = enable_by_path.get(&id).copied().unwrap_or(true);
    SkillDto {
        id,
        plugin_id: plugin_id.map(str::to_string),
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

    #[test]
    fn uppercase_git_marketplace_source_uses_the_clone_cache() {
        let root = PathBuf::from(r"C:\Users\me\.codex");
        let entry = CodexMarketplace {
            source_type: None,
            source: Some("git@github.com:example/workshop.GIT".into()),
        };

        assert_eq!(
            marketplace_path(&root, "workshop", &entry),
            Some(root.join(".tmp").join("marketplaces").join("workshop"))
        );
    }

    fn fixture() -> (PathBuf, PathBuf) {
        let home = crate::paths::scratch_dir("on-n-off-codex-home");
        let root = home.join(".codex");
        let agents_skills = home.join(".agents").join("skills");
        let plugin = root.join("plugins/cache/workshop/workbench/0.22.1");
        fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
        fs::write(
            plugin.join(".codex-plugin/plugin.json"),
            r#"{"name":"workbench","version":"0.22.1"}"#,
        )
        .unwrap();
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
        fs::write(
            &extra,
            "---\nname: conoswiki-feed\ndescription: Feed ConosWiki\n---\n",
        )
        .unwrap();
        let marketplace = root.join(".tmp/marketplaces/workshop");
        fs::create_dir_all(marketplace.join(".claude-plugin")).unwrap();
        fs::write(
            marketplace.join(".claude-plugin/marketplace.json"),
            r#"{"plugins":[{"name":"workbench","version":"0.23.0","source":"./plugins/workbench"}]}"#,
        )
        .unwrap();
        fs::write(
            root.join("config.toml"),
            format!(
                "[marketplaces.workshop]\nsource_type = \"git\"\nsource = \"https://example.invalid/workshop.git\"\n\n[plugins.\"workbench@workshop\"]\nenabled = true\n\n[plugins.\"toolkit@workshop\"]\nenabled = false\n\n[[skills.config]]\npath = '{}'\nenabled = false\n\n[mcp_servers.github]\ncommand = \"npx\"\nargs = [\"-y\", \"@modelcontextprotocol/server-github\"]\n\n[mcp_servers.docs]\nurl = \"https://docs.example/mcp\"\nenabled = false\n",
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
        assert_eq!(tab.plugins[0].version, "");
        assert_eq!(tab.plugins[1].version, "0.22.1");
        assert_eq!(tab.plugins[0].upstream, "");
        assert_eq!(tab.plugins[1].upstream, "0.23.0");
        assert!(tab.plugins[1].out_of_sync);
        assert_eq!(tab.plugins[1].skills.len(), 1);
        let names: Vec<_> = tab
            .user_skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect();
        assert_eq!(names, ["conoswiki-feed", "loom-feed"]);
        let wiki = tab
            .user_skills
            .iter()
            .find(|skill| skill.name == "conoswiki-feed")
            .unwrap();
        assert!(!wiki.enabled);
        let mcp_ids: Vec<_> = tab
            .mcp_servers
            .iter()
            .map(|server| server.id.as_str())
            .collect();
        assert_eq!(mcp_ids, ["docs", "github"]);
        assert!(!tab.mcp_servers[0].enabled);
        assert_eq!(tab.mcp_servers[0].system, "http");
        assert!(tab.mcp_servers[1].enabled);
        assert!(tab.mcp_servers[1].togglable);
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
        assert!(tab.mcp_servers.is_empty());
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

        let tab = adapter
            .set_skill_enabled(&loom_id, false)
            .expect("toggle loom");
        let loom = tab
            .user_skills
            .iter()
            .find(|skill| skill.name == "loom-feed")
            .unwrap();
        assert!(!loom.enabled);
        let text = fs::read_to_string(root.join("config.toml")).unwrap();
        assert!(text.contains("[plugins.\"workbench@workshop\"]"));
        assert!(text.contains("[plugins.\"toolkit@workshop\"]"));
        assert!(text.contains("[[skills.config]]"));
        assert!(text.contains("loom-feed"));

        let tab = adapter
            .set_skill_enabled(&wiki_id, true)
            .expect("toggle wiki");
        let wiki = tab
            .user_skills
            .iter()
            .find(|skill| skill.name == "conoswiki-feed")
            .unwrap();
        assert!(wiki.enabled);
        assert!(root
            .join("_backups/codex")
            .read_dir()
            .unwrap()
            .next()
            .is_some());
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn toggling_plugin_patches_toml_enabled_only() {
        let (root, agents_skills) = fixture();
        let adapter = CodexAdapter::at(root.clone(), agents_skills);
        let tab = adapter
            .set_plugin_enabled("workbench@workshop", false)
            .expect("disable");
        let plugin = tab
            .plugins
            .iter()
            .find(|plugin| plugin.id == "workbench@workshop")
            .unwrap();
        assert!(!plugin.enabled);
        let text = fs::read_to_string(root.join("config.toml")).unwrap();
        assert!(text.contains("[plugins.\"toolkit@workshop\"]"));
        assert!(text.contains("[[skills.config]]"));
        assert!(root
            .join("_backups/codex")
            .read_dir()
            .unwrap()
            .next()
            .is_some());
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn toggling_mcp_patches_toml_enabled_only() {
        let (root, agents_skills) = fixture();
        let adapter = CodexAdapter::at(root.clone(), agents_skills);
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
        let text = fs::read_to_string(root.join("config.toml")).unwrap();
        assert!(text.contains("[plugins.\"workbench@workshop\"]"), "{text}");
        assert!(text.contains("[[skills.config]]"), "{text}");
        assert!(text.contains("[mcp_servers.docs]"), "{text}");
        let tab = adapter.set_mcp_enabled("docs", true).expect("on");
        assert!(
            tab.mcp_servers
                .iter()
                .find(|server| server.id == "docs")
                .unwrap()
                .enabled
        );
        let text = fs::read_to_string(root.join("config.toml")).unwrap();
        assert!(
            text.contains("url = \"https://docs.example/mcp\""),
            "{text}"
        );
        assert!(root
            .join("_backups/codex")
            .read_dir()
            .unwrap()
            .next()
            .is_some());
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
    fn update_upgrades_marketplace_then_readds_plugin() {
        let (root, agents_skills) = fixture();
        let dir = root.join("_cli");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("codex.cmd"),
            "@echo off\r\necho %* >> \"%~dp0args.txt\"\r\nexit /b 0\r\n",
        )
        .unwrap();
        let adapter = CodexAdapter::at_with_cli(
            root.clone(),
            agents_skills,
            AgentCli::new(dir.join("codex.cmd").to_string_lossy().as_ref()),
        );
        adapter.update_plugin("workbench@workshop").expect("update");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(
            args.contains("plugin marketplace upgrade --json workshop"),
            "{args}"
        );
        assert!(
            args.contains("plugin add --json workbench@workshop"),
            "{args}"
        );
        assert!(root
            .join("_backups/codex")
            .read_dir()
            .unwrap()
            .next()
            .is_some());
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn install_and_uninstall_use_official_argv() {
        let (root, agents_skills) = fixture();
        let adapter =
            CodexAdapter::at_with_cli(root.clone(), agents_skills, codex_argv_stub(&root, 0, ""));
        adapter.install_plugin("workbench@workshop").expect("add");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(
            args.contains("plugin add --json workbench@workshop"),
            "{args}"
        );
        adapter.install_plugin("acme/tools@main").expect("market");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(
            args.contains("plugin marketplace add --json acme/tools --ref main"),
            "{args}"
        );
        adapter
            .uninstall_plugin("workbench@workshop")
            .expect("remove");
        let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
        assert!(
            args.contains("plugin remove --json workbench@workshop"),
            "{args}"
        );
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn install_cli_failure_does_not_mutate_config() {
        let (root, agents_skills) = fixture();
        let before = fs::read_to_string(root.join("config.toml")).unwrap();
        let adapter = CodexAdapter::at_with_cli(
            root.clone(),
            agents_skills,
            codex_argv_stub(&root, 3, "nope"),
        );
        let err = adapter.install_plugin("acme/tools").expect_err("cli");
        assert!(err.message.contains("nope"));
        assert_eq!(
            fs::read_to_string(root.join("config.toml")).unwrap(),
            before
        );
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }
}
