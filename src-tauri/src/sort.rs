use std::cmp::Ordering;

use crate::dto::{AgentTabDto, McpServerDto, PluginDto, SkillDto};

pub fn cmp_plugin_then_name(
    a_plugin: &str,
    a_name: &str,
    b_plugin: &str,
    b_name: &str,
) -> Ordering {
    a_plugin
        .to_ascii_lowercase()
        .cmp(&b_plugin.to_ascii_lowercase())
        .then_with(|| {
            a_name
                .to_ascii_lowercase()
                .cmp(&b_name.to_ascii_lowercase())
        })
        .then_with(|| a_plugin.cmp(b_plugin))
        .then_with(|| a_name.cmp(b_name))
}

pub fn skill_plugin_key(skill: &SkillDto) -> &str {
    skill.plugin_id.as_deref().unwrap_or("")
}

pub fn sort_plugins(plugins: &mut [PluginDto]) {
    for plugin in plugins.iter_mut() {
        sort_skills(&mut plugin.skills);
    }
    plugins.sort_by(|a, b| cmp_plugin_then_name(&a.source, &a.name, &b.source, &b.name));
}

pub fn sort_skills(skills: &mut [SkillDto]) {
    skills.sort_by(|a, b| {
        cmp_plugin_then_name(skill_plugin_key(a), &a.name, skill_plugin_key(b), &b.name)
    });
}

pub fn sort_mcps(servers: &mut [McpServerDto]) {
    servers.sort_by(|a, b| cmp_plugin_then_name(&a.name, &a.system, &b.name, &b.system));
}

pub fn sort_tab(tab: &mut AgentTabDto) {
    sort_plugins(&mut tab.plugins);
    sort_skills(&mut tab.user_skills);
    sort_mcps(&mut tab.mcp_servers);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{McpServerDto, PluginDto, SkillDto};

    fn plugin(name: &str, source: &str) -> PluginDto {
        PluginDto {
            id: format!("{name}@{source}"),
            name: name.to_string(),
            source: source.to_string(),
            version: String::new(),
            upstream: String::new(),
            out_of_sync: false,
            enabled: true,
            togglable: true,
            skills: vec![],
        }
    }

    fn skill(name: &str, plugin_id: Option<&str>) -> SkillDto {
        SkillDto {
            id: name.to_string(),
            plugin_id: plugin_id.map(str::to_string),
            name: name.to_string(),
            description: String::new(),
            enabled: true,
            togglable: plugin_id.is_none(),
            origin: String::new(),
        }
    }

    fn mcp(name: &str, system: &str) -> McpServerDto {
        McpServerDto {
            id: name.to_string(),
            name: name.to_string(),
            system: system.to_string(),
            source: system.to_string(),
            enabled: true,
            togglable: true,
            origin: String::new(),
        }
    }

    #[test]
    fn plugins_sort_by_source_then_name_case_insensitive() {
        let mut plugins = vec![
            plugin("Zebra", "alpha"),
            plugin("apple", "beta"),
            plugin("Apple", "alpha"),
            plugin("warp", "claude-code-warp"),
        ];
        sort_plugins(&mut plugins);
        let keys: Vec<_> = plugins
            .iter()
            .map(|plugin| (plugin.source.as_str(), plugin.name.as_str()))
            .collect();
        assert_eq!(
            keys,
            [
                ("alpha", "Apple"),
                ("alpha", "Zebra"),
                ("beta", "apple"),
                ("claude-code-warp", "warp"),
            ]
        );
    }

    #[test]
    fn skills_sort_by_plugin_then_name() {
        let mut skills = vec![
            skill("zeta", Some("apple@x")),
            skill("Beta", None),
            skill("alpha", Some("apple@x")),
            skill("beta", Some("zebra@z")),
        ];
        sort_skills(&mut skills);
        let keys: Vec<_> = skills
            .iter()
            .map(|skill| (skill_plugin_key(skill), skill.name.as_str()))
            .collect();
        assert_eq!(
            keys,
            [
                ("", "Beta"),
                ("apple@x", "alpha"),
                ("apple@x", "zeta"),
                ("zebra@z", "beta")
            ]
        );
    }

    #[test]
    fn mcps_sort_by_name_then_transport() {
        let mut servers = vec![
            mcp("github", "stdio"),
            mcp("Docs", "http"),
            mcp("docs", "stdio"),
        ];
        sort_mcps(&mut servers);
        let keys: Vec<_> = servers
            .iter()
            .map(|server| (server.name.as_str(), server.system.as_str()))
            .collect();
        assert_eq!(
            keys,
            [("Docs", "http"), ("docs", "stdio"), ("github", "stdio")]
        );
    }
}
