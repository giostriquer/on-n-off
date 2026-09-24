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
        via: String::new(),
    }
}

fn hook(source: &str, event: &str, command: &str, plugin_id: Option<&str>) -> HookDto {
    HookDto {
        id: format!("{}:{source}:{event}:0:0", plugin_id.unwrap_or("")),
        event: event.to_string(),
        matcher: String::new(),
        handler: "command".to_string(),
        command: command.to_string(),
        source: source.to_string(),
        plugin_id: plugin_id.map(str::to_string),
        description: String::new(),
        enabled: true,
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

#[test]
fn hooks_sort_by_source_then_event_and_keep_their_file_order() {
    let mut hooks = vec![
        hook("settings.json", "Stop", "user one", None),
        hook("acme", "Stop", "plugin stop", Some("acme@webapp")),
        hook("settings.json", "Stop", "user two", None),
        hook("settings.json", "PreToolUse", "guard", None),
        hook("Acme", "SessionStart", "other start", Some("acme@other")),
    ];
    sort_hooks(&mut hooks);
    let keys: Vec<_> = hooks
        .iter()
        .map(|hook| (hook.source.as_str(), hook.command.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            ("Acme", "other start"),
            ("acme", "plugin stop"),
            ("settings.json", "guard"),
            // Two rows of one event stay in the order the file has them.
            ("settings.json", "user one"),
            ("settings.json", "user two"),
        ]
    );
}

#[test]
fn hooks_of_two_plugins_that_share_a_name_sort_by_plugin_id() {
    // Source is the plugin's *name*, so two marketplaces shipping an `acme` both land here; the
    // id is what separates them, and without it the two would interleave by whichever adapter
    // walked first.
    let mut hooks = vec![
        hook("acme", "Stop", "from webapp", Some("acme@webapp")),
        hook("acme", "Stop", "from other", Some("acme@other")),
    ];
    sort_hooks(&mut hooks);
    let keys: Vec<_> = hooks
        .iter()
        .map(|hook| (hook.plugin_id.as_deref(), hook.command.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            (Some("acme@other"), "from other"),
            (Some("acme@webapp"), "from webapp"),
        ]
    );
}
