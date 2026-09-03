use std::collections::{HashMap, HashSet};
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
use crate::mcp::{parse_codex_map, CodexMcpEntry};
use crate::paths::{
    agents_skills_root, codex_root, newest_dir, normalize_skill_path, plugin_id_parts,
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

    fn build_tab(&self, enrich_remote: bool) -> Result<AgentTabDto, AdapterError> {
        let config = self.config()?;
        let enable_by_path = skill_enable_map(&config);
        let catalogs: HashMap<String, HashMap<String, crate::plugin_meta::VersionHint>> = self
            .marketplace_roots(&config)
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
            if enrich_remote {
                if let Some(hint) = catalog.as_mut() {
                    crate::plugin_meta::fill_remote_version(hint);
                }
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
}

impl AgentAdapter for CodexAdapter {
    fn info(&self) -> AgentInfo {
        agent_info(AgentId::Codex)
    }

    fn item_roots(&self, scope: &ItemScope) -> Result<ItemRoots, AdapterError> {
        match scope {
            ItemScope::Global => Ok(ItemRoots {
                skills: self.root()?.join("skills"),
                agents: None,
            }),
            ItemScope::Project { project_path } => Ok(crate::project::project_item_roots(
                Path::new(project_path),
                AgentId::Codex,
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
mod tests;
