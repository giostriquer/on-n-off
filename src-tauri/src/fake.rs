use std::sync::Mutex;

use crate::adapter::AgentAdapter;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto, PluginDto, SkillDto};

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
        Ok(self.lock_tab()?.clone())
    }

    fn set_plugin_enabled(&self, plugin_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.lock_tab()?;
        let plugin = tab
            .plugins
            .iter_mut()
            .find(|plugin| plugin.id == plugin_id)
            .ok_or_else(|| AdapterError::message(format!("plugin not found: {plugin_id}")))?;
        plugin.enabled = enabled;
        Ok(tab.clone())
    }

    fn set_skill_enabled(&self, skill_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.lock_tab()?;
        tab.ensure_togglable(skill_id)?;
        let skill = find_skill_mut(&mut tab, skill_id)
            .ok_or_else(|| AdapterError::message(format!("skill not found: {skill_id}")))?;
        skill.enabled = enabled;
        Ok(tab.clone())
    }

    fn install_plugin(&self, source: &str) -> Result<AgentTabDto, AdapterError> {
        let source = source.trim();
        if source.is_empty() {
            return Err(AdapterError::message("install source is empty"));
        }
        let mut tab = self.lock_tab()?;
        let name = plugin_name_from_source(source);
        let id = format!("{name}@local");
        if tab.plugins.iter().any(|plugin| plugin.id == id) {
            return Err(AdapterError::message(format!("plugin already installed: {id}")));
        }
        tab.plugins.push(PluginDto {
            id,
            name,
            source: source.to_string(),
            enabled: true,
            skills: Vec::new(),
        });
        Ok(tab.clone())
    }

    fn uninstall_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.lock_tab()?;
        let before = tab.plugins.len();
        tab.plugins.retain(|plugin| plugin.id != plugin_id);
        if tab.plugins.len() == before {
            return Err(AdapterError::message(format!("plugin not found: {plugin_id}")));
        }
        Ok(tab.clone())
    }
}

fn find_skill_mut<'a>(tab: &'a mut AgentTabDto, skill_id: &str) -> Option<&'a mut SkillDto> {
    for plugin in &mut tab.plugins {
        if let Some(skill) = plugin.skills.iter_mut().find(|skill| skill.id == skill_id) {
            return Some(skill);
        }
    }
    tab.user_skills.iter_mut().find(|skill| skill.id == skill_id)
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
    }
}

fn plugin(id: &str, name: &str, source: &str, enabled: bool, skills: Vec<SkillDto>) -> PluginDto {
    PluginDto {
        id: id.to_string(),
        name: name.to_string(),
        source: source.to_string(),
        enabled,
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
            plugin("superpowers@claude-plugins-official", "superpowers", "official", false, vec![]),
        ],
        user_skills: vec![skill(
            "statusline",
            None,
            "statusline",
            "Custom status line",
            true,
            true,
        )],
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_seed_lists_stable_plugin_ids_and_unique_skill_ids() {
        let adapter = FakeAdapter::claude();
        let tab = adapter.list_tab().expect("list");

        let ids: Vec<&str> = tab.plugins.iter().map(|plugin| plugin.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "workbench@workshop",
                "toolkit@workshop",
                "superpowers@claude-plugins-official"
            ]
        );
        assert!(tab.plugins[0].enabled);
        assert!(!tab.plugins[2].enabled);

        let brainstorming = &tab.plugins[0].skills[0];
        assert_eq!(brainstorming.id, "workbench@workshop:brainstorming");
        assert!(!brainstorming.togglable);
        assert_eq!(tab.user_skills[0].id, "statusline");
        assert!(tab.user_skills[0].togglable);
    }

    #[test]
    fn toggling_a_locked_plugin_skill_fails_without_changing_state() {
        let adapter = FakeAdapter::claude();
        let err = adapter
            .set_skill_enabled("workbench@workshop:brainstorming", false)
            .expect_err("locked skill");
        assert!(err.message.contains("not togglable"));
        let tab = adapter.list_tab().expect("list");
        assert!(tab.plugins[0].skills[0].enabled);
    }

    #[test]
    fn toggling_a_user_skill_returns_the_full_tab() {
        let adapter = FakeAdapter::claude();
        let tab = adapter.set_skill_enabled("statusline", false).expect("toggle");
        assert!(!tab.user_skills[0].enabled);
        assert!(tab.plugins[0].enabled);
        assert!(tab.plugins[0].skills[0].enabled);
    }
}
