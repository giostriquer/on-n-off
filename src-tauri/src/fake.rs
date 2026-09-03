use std::sync::Mutex;

use crate::adapter::AgentAdapter;
use crate::dto::{
    AdapterError, AgentId, AgentInfo, AgentTabDto, McpServerDto, PluginDto, SkillDto,
};
use crate::sort::sort_tab;

pub struct FakeAdapter {
    id: AgentId,
    cli_ok: bool,
    cli_error: Option<String>,
    install_git: bool,
    install_folder: bool,
    tab: Mutex<AgentTabDto>,
}

impl FakeAdapter {
    pub fn claude() -> Self {
        Self {
            id: AgentId::Claude,
            cli_ok: true,
            cli_error: None,
            install_git: true,
            install_folder: true,
            tab: Mutex::new(claude_seed()),
        }
    }

    #[allow(dead_code)]
    pub fn codex() -> Self {
        Self {
            id: AgentId::Codex,
            cli_ok: true,
            cli_error: None,
            install_git: true,
            install_folder: true,
            tab: Mutex::new(codex_seed()),
        }
    }

    fn lock_tab(&self) -> Result<std::sync::MutexGuard<'_, AgentTabDto>, AdapterError> {
        self.tab
            .lock()
            .map_err(|_| AdapterError::message("fake adapter lock poisoned"))
    }
}

impl AgentAdapter for FakeAdapter {
    fn info(&self) -> AgentInfo {
        AgentInfo {
            id: self.id,
            display_name: self.id.display_name().to_string(),
            cli_ok: self.cli_ok,
            cli_error: self.cli_error.clone(),
            install_git: self.install_git,
            install_folder: self.install_folder,
            plugin_toggle: self.cli_ok,
        }
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.lock_tab()?.clone();
        sort_tab(&mut tab);
        Ok(tab)
    }

    fn set_plugin_enabled(
        &self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.lock_tab()?;
        let plugin = tab
            .plugins
            .iter_mut()
            .find(|plugin| plugin.id == plugin_id)
            .ok_or_else(|| AdapterError::message(format!("plugin not found: {plugin_id}")))?;
        plugin.enabled = enabled;
        Ok(tab.clone())
    }

    fn set_skill_enabled(
        &self,
        skill_id: &str,
        enabled: bool,
    ) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.lock_tab()?;
        tab.ensure_togglable(skill_id)?;
        let skill = find_skill_mut(&mut tab, skill_id)
            .ok_or_else(|| AdapterError::message(format!("skill not found: {skill_id}")))?;
        skill.enabled = enabled;
        Ok(tab.clone())
    }

    fn set_mcp_enabled(&self, mcp_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.lock_tab()?;
        tab.ensure_mcp_togglable(mcp_id)?;
        let server = tab
            .mcp_servers
            .iter_mut()
            .find(|server| server.id == mcp_id)
            .ok_or_else(|| AdapterError::message(format!("mcp server not found: {mcp_id}")))?;
        server.enabled = enabled;
        Ok(tab.clone())
    }

    fn install_plugin(&self, source: &str) -> Result<AgentTabDto, AdapterError> {
        let parsed = crate::install_source::parse_install_source(source)?;
        let mut tab = self.lock_tab()?;
        if let crate::install_source::InstallSource::NpxSkills {
            source: skill_source,
            skill: skill_name,
        } = &parsed
        {
            let name = skill_name
                .clone()
                .unwrap_or_else(|| plugin_name_from_source(skill_source));
            if tab.user_skills.iter().any(|row| row.name == name) {
                return Err(AdapterError::message(format!(
                    "skill already installed: {name}"
                )));
            }
            tab.user_skills.push(skill(
                &name,
                None,
                &name,
                "Installed via npx skills add",
                true,
                true,
            ));
            return Ok(tab.clone());
        }
        let source = parsed.as_cli_source();
        let name = plugin_name_from_source(&source);
        let id = format!("{name}@local");
        if tab.plugins.iter().any(|plugin| plugin.id == id) {
            return Err(AdapterError::message(format!(
                "plugin already installed: {id}"
            )));
        }
        tab.plugins.push(PluginDto {
            id,
            name,
            source: source.to_string(),
            version: String::new(),
            upstream: String::new(),
            out_of_sync: false,
            enabled: true,
            togglable: true,
            skills: Vec::new(),
        });
        Ok(tab.clone())
    }

    fn uninstall_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.lock_tab()?;
        let before = tab.plugins.len();
        tab.plugins.retain(|plugin| plugin.id != plugin_id);
        if tab.plugins.len() == before {
            return Err(AdapterError::message(format!(
                "plugin not found: {plugin_id}"
            )));
        }
        Ok(tab.clone())
    }

    fn update_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.lock_tab()?;
        let plugin = tab
            .plugins
            .iter_mut()
            .find(|plugin| plugin.id == plugin_id)
            .ok_or_else(|| AdapterError::message(format!("plugin not found: {plugin_id}")))?;
        if !plugin.upstream.is_empty() {
            plugin.version = plugin.upstream.clone();
        }
        plugin.out_of_sync = false;
        Ok(tab.clone())
    }
}

fn find_skill_mut<'a>(tab: &'a mut AgentTabDto, skill_id: &str) -> Option<&'a mut SkillDto> {
    for plugin in &mut tab.plugins {
        if let Some(skill) = plugin.skills.iter_mut().find(|skill| skill.id == skill_id) {
            return Some(skill);
        }
    }
    tab.user_skills
        .iter_mut()
        .find(|skill| skill.id == skill_id)
}

fn plugin_name_from_source(source: &str) -> String {
    source
        .trim_end_matches(".git")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(source)
        .split('@')
        .next()
        .unwrap_or(source)
        .to_string()
}

fn skill(
    id: &str,
    plugin_id: Option<&str>,
    name: &str,
    description: &str,
    enabled: bool,
    togglable: bool,
) -> SkillDto {
    SkillDto {
        id: id.to_string(),
        plugin_id: plugin_id.map(str::to_string),
        name: name.to_string(),
        description: description.to_string(),
        enabled,
        togglable,
        origin: String::new(),
    }
}

fn plugin(id: &str, name: &str, source: &str, enabled: bool, skills: Vec<SkillDto>) -> PluginDto {
    PluginDto {
        id: id.to_string(),
        name: name.to_string(),
        source: source.to_string(),
        version: String::new(),
        upstream: String::new(),
        out_of_sync: false,
        enabled,
        togglable: true,
        skills,
    }
}

fn claude_seed() -> AgentTabDto {
    AgentTabDto {
        plugins: vec![
            plugin(
                "workbench@workshop",
                "workbench",
                "workshop",
                true,
                vec![
                    skill(
                        "workbench@workshop:brainstorming",
                        Some("workbench@workshop"),
                        "brainstorming",
                        "Turn ideas into designs",
                        true,
                        false,
                    ),
                    skill(
                        "workbench@workshop:systematic-debugging",
                        Some("workbench@workshop"),
                        "systematic-debugging",
                        "Debug from evidence",
                        true,
                        false,
                    ),
                ],
            ),
            plugin(
                "toolkit@workshop",
                "toolkit",
                "workshop",
                true,
                vec![skill(
                    "toolkit@workshop:arch-map",
                    Some("toolkit@workshop"),
                    "arch-map",
                    "Map a codebase",
                    true,
                    false,
                )],
            ),
            plugin(
                "superpowers@claude-plugins-official",
                "superpowers",
                "official",
                false,
                vec![],
            ),
        ],
        user_skills: vec![skill(
            "statusline",
            None,
            "statusline",
            "Custom status line",
            true,
            true,
        )],
        mcp_servers: vec![McpServerDto {
            id: "github".to_string(),
            name: "github".to_string(),
            system: "stdio".to_string(),
            source: "npx -y @modelcontextprotocol/server-github".to_string(),
            enabled: true,
            togglable: true,
            origin: String::new(),
        }],
    }
}

#[allow(dead_code)]
fn codex_seed() -> AgentTabDto {
    AgentTabDto {
        plugins: vec![
            plugin(
                "workbench@workshop",
                "workbench",
                "workshop",
                true,
                vec![skill(
                    r"C:\fake-home\.codex\plugins\workbench\skills\brainstorming\SKILL.md",
                    Some("workbench@workshop"),
                    "brainstorming",
                    "Turn ideas into designs",
                    true,
                    true,
                )],
            ),
            plugin("toolkit@workshop", "toolkit", "workshop", true, vec![]),
        ],
        user_skills: vec![
            skill(
                r"C:\fake-home\.codex\skills\conoswiki-feed\SKILL.md",
                None,
                "conoswiki-feed",
                "Feed ConosWiki",
                false,
                true,
            ),
            skill(
                r"C:\fake-home\.codex\skills\loom-feed\SKILL.md",
                None,
                "loom-feed",
                "Feed Loom",
                false,
                true,
            ),
        ],
        mcp_servers: vec![],
    }
}

#[cfg(test)]
mod tests;
