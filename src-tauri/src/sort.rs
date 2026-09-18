use std::cmp::Ordering;

use crate::dto::{AgentTabDto, HookDto, McpServerDto, PluginDto, SkillDto};

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

pub fn hook_plugin_key(hook: &HookDto) -> &str {
    hook.plugin_id.as_deref().unwrap_or("")
}

/// Source, then event, then the row's place in the file it came from. That last key needs no
/// field of its own: `sort_by` is stable and `hooks` emits rows in file order, so equal keys
/// keep the order the file has. The plugin id only separates two plugins that share a name.
pub fn sort_hooks(hooks: &mut [HookDto]) {
    hooks.sort_by(|a, b| {
        cmp_plugin_then_name(&a.source, &a.event, &b.source, &b.event)
            .then_with(|| hook_plugin_key(a).cmp(hook_plugin_key(b)))
    });
}

pub fn sort_tab(tab: &mut AgentTabDto) {
    sort_plugins(&mut tab.plugins);
    sort_skills(&mut tab.user_skills);
    sort_mcps(&mut tab.mcp_servers);
    sort_hooks(&mut tab.hooks);
}

#[cfg(test)]
mod tests;
