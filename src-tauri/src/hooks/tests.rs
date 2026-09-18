use std::fs;
use std::path::PathBuf;

use super::*;
use crate::adapter::AgentAdapter;

fn ids(hooks: &[HookDto]) -> Vec<&str> {
    hooks.iter().map(|hook| hook.id.as_str()).collect()
}

fn commands(hooks: &[HookDto]) -> Vec<&str> {
    hooks.iter().map(|hook| hook.command.as_str()).collect()
}

/// A plugin folder with a manifest and, optionally, one hook file beside it.
fn plugin_dir(prefix: &str, manifest_dir: &str, manifest: &str) -> PathBuf {
    let root = crate::paths::scratch_dir(prefix);
    fs::create_dir_all(root.join(manifest_dir)).unwrap();
    fs::write(root.join(manifest_dir).join("plugin.json"), manifest).unwrap();
    root
}

fn write_hook_file(root: &Path, rel: &str, body: &str) {
    let mut path = root.to_path_buf();
    for part in rel.split('/') {
        path.push(part);
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn claude_user_settings_hooks_become_one_row_per_handler() {
    let hooks = claude_settings_hooks(
        r#"{
            "enabledPlugins": { "acme@webapp": true },
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            { "type": "command", "command": "/Users/me/bin/guard.sh", "timeout": 5 },
                            { "type": "command", "command": "/Users/me/bin/log.sh" }
                        ]
                    }
                ],
                "Stop": [
                    { "hooks": [{ "type": "command", "command": "acme notify --stop" }] }
                ]
            }
        }"#,
    );

    assert_eq!(
        ids(&hooks),
        [
            ":settings.json:pre_tool_use:0:0",
            ":settings.json:pre_tool_use:0:1",
            ":settings.json:stop:0:0",
        ]
    );
    assert_eq!(
        commands(&hooks),
        [
            "/Users/me/bin/guard.sh",
            "/Users/me/bin/log.sh",
            "acme notify --stop",
        ]
    );
    // The event stays in Claude's own spelling even though the id is snake_cased.
    assert_eq!(hooks[0].event, "PreToolUse");
    assert_eq!(hooks[0].matcher, "Bash");
    assert_eq!(hooks[0].handler, "command");
    assert_eq!(hooks[0].source, "settings.json");
    assert_eq!(hooks[0].plugin_id, None);
    assert_eq!(hooks[0].description, "");
    assert!(hooks[0].enabled);
    // An entry with no matcher matches everything the event fires for.
    assert_eq!(hooks[2].matcher, "");
}

#[test]
fn claude_plugin_hooks_file_is_read_with_its_description() {
    let root = plugin_dir(
        "on-n-off-hooks-claude-default",
        ".claude-plugin",
        r#"{ "name": "acme", "description": "The plugin's own description" }"#,
    );
    write_hook_file(
        &root,
        "hooks/hooks.json",
        r#"{
            "description": "Acme terminal notifications",
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "startup|resume",
                        "hooks": [{ "type": "command", "command": "${CLAUDE_PLUGIN_ROOT}/scripts/start.sh" }]
                    }
                ]
            }
        }"#,
    );

    let hooks = claude_plugin_hooks("acme@webapp", "acme", &root);

    assert_eq!(
        ids(&hooks),
        ["acme@webapp:hooks/hooks.json:session_start:0:0"]
    );
    assert_eq!(hooks[0].source, "acme");
    assert_eq!(hooks[0].plugin_id.as_deref(), Some("acme@webapp"));
    // The hook file's own description wins over the plugin's.
    assert_eq!(hooks[0].description, "Acme terminal notifications");
    assert_eq!(hooks[0].matcher, "startup|resume");
    // Unexpanded: expanding it would show a path the file does not contain.
    assert_eq!(hooks[0].command, "${CLAUDE_PLUGIN_ROOT}/scripts/start.sh");
}

#[test]
fn claude_plugin_manifest_names_one_hook_file() {
    let root = plugin_dir(
        "on-n-off-hooks-claude-path",
        ".claude-plugin",
        r#"{ "name": "acme", "description": "Acme for webapp", "hooks": "./hooks/claude.json" }"#,
    );
    // The default file exists too and must be ignored once the manifest names another.
    write_hook_file(
        &root,
        "hooks/hooks.json",
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "wrong" }] }] } }"#,
    );
    write_hook_file(
        &root,
        "hooks/claude.json",
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme hook Stop" }] }] } }"#,
    );

    let hooks = claude_plugin_hooks("acme@webapp", "acme", &root);

    assert_eq!(ids(&hooks), ["acme@webapp:hooks/claude.json:stop:0:0"]);
    assert_eq!(commands(&hooks), ["acme hook Stop"]);
    // No description in the hook file, so the plugin's own stands in.
    assert_eq!(hooks[0].description, "Acme for webapp");
}

#[test]
fn claude_plugin_manifest_names_several_hook_files() {
    let root = plugin_dir(
        "on-n-off-hooks-claude-paths",
        ".claude-plugin",
        r#"{ "name": "acme", "hooks": ["./hooks/first.json", "hooks/second.json", "../escape.json"] }"#,
    );
    write_hook_file(
        &root,
        "hooks/first.json",
        r#"{ "hooks": { "SessionStart": [{ "hooks": [{ "type": "command", "command": "acme first" }] }] } }"#,
    );
    write_hook_file(
        &root,
        "hooks/second.json",
        r#"{ "hooks": { "SessionEnd": [{ "hooks": [{ "type": "command", "command": "acme second" }] }] } }"#,
    );

    let hooks = claude_plugin_hooks("acme@webapp", "acme", &root);

    assert_eq!(
        ids(&hooks),
        [
            "acme@webapp:hooks/first.json:session_start:0:0",
            "acme@webapp:hooks/second.json:session_end:0:0",
        ]
    );
    assert_eq!(commands(&hooks), ["acme first", "acme second"]);
}

#[test]
fn claude_plugin_manifest_can_hold_the_events_inline() {
    let root = plugin_dir(
        "on-n-off-hooks-claude-inline",
        ".claude-plugin",
        r#"{
            "name": "acme",
            "description": "Acme for webapp",
            "hooks": {
                "hooks": {
                    "Stop": [{ "hooks": [{ "type": "mcp_tool", "server": "acme", "tool": "turn_ended" }] }]
                }
            }
        }"#,
    );

    let hooks = claude_plugin_hooks("acme@webapp", "acme", &root);

    assert_eq!(ids(&hooks), ["acme@webapp:plugin.json#hooks[0]:stop:0:0"]);
    assert_eq!(hooks[0].handler, "mcp_tool");
    assert_eq!(hooks[0].command, "acme · turn_ended");
    assert_eq!(hooks[0].description, "Acme for webapp");
}

#[test]
fn a_disabled_plugin_contributes_no_hooks() {
    let root = crate::paths::scratch_dir("on-n-off-hooks-enablement");
    for (name, command) in [("on", "acme on"), ("off", "acme off")] {
        let plugin = root.join("plugins/cache/webapp").join(name).join("1.0.0");
        fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
        fs::write(
            plugin.join(".claude-plugin/plugin.json"),
            format!(r#"{{"name":"{name}"}}"#),
        )
        .unwrap();
        write_hook_file(
            &plugin,
            "hooks/hooks.json",
            &format!(
                r#"{{ "hooks": {{ "Stop": [{{ "hooks": [{{ "type": "command", "command": "{command}" }}] }}] }} }}"#
            ),
        );
    }
    fs::write(
        root.join("plugins/installed_plugins.json"),
        serde_json::json!({
            "version": 2,
            "plugins": {
                "on@webapp": [{ "scope": "user", "installPath": root.join("plugins/cache/webapp/on/1.0.0") }],
                "off@webapp": [{ "scope": "user", "installPath": root.join("plugins/cache/webapp/off/1.0.0") }]
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("settings.json"),
        r#"{
            "enabledPlugins": { "on@webapp": true, "off@webapp": false },
            "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme user" }] }] }
        }"#,
    )
    .unwrap();

    let tab = crate::claude::ClaudeAdapter::at(root)
        .list_local_tab()
        .unwrap();

    assert_eq!(commands(&tab.hooks), ["acme on", "acme user"]);
}

#[test]
fn codex_hooks_file_rows_carry_the_files_description() {
    let hooks = codex_hooks_file(
        r#"{
            "description": "Acme's observation leg for Codex",
            "hooks": {
                "SessionStart": [{ "hooks": [{ "type": "command", "command": "acme hook SessionStart", "timeout": 5 }] }],
                "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "acme hook UserPromptSubmit" }] }]
            }
        }"#,
    );

    assert_eq!(
        ids(&hooks),
        [
            ":hooks.json:session_start:0:0",
            ":hooks.json:user_prompt_submit:0:0",
        ]
    );
    assert_eq!(hooks[0].source, "hooks.json");
    assert_eq!(hooks[0].description, "Acme's observation leg for Codex");
    assert_eq!(hooks[0].plugin_id, None);
    assert!(hooks[0].enabled);
}

#[test]
fn codex_config_table_rows_skip_the_state_table() {
    let hooks = codex_config_hooks(
        r#"
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "acme notify --stop"

[hooks.state."acme@webapp:hooks/codex.json:stop:0:0"]
trusted_hash = "sha256:0000"

[mcp_servers.acme]
command = "npx"
"#,
    );

    assert_eq!(ids(&hooks), [":config.toml:stop:0:0"]);
    assert_eq!(hooks[0].source, "config.toml");
    assert_eq!(hooks[0].command, "acme notify --stop");
}

#[test]
fn the_legacy_notify_key_is_a_hook_of_its_own() {
    let hooks = codex_config_hooks(
        "notify = [\"/Users/me/bin/acme-notify\", \"turn-ended\"]\nmodel = \"acme-1\"\n",
    );

    assert_eq!(ids(&hooks), [":notify:notification:0:0"]);
    assert_eq!(hooks[0].event, "Notification");
    assert_eq!(hooks[0].handler, "command");
    assert_eq!(hooks[0].command, "/Users/me/bin/acme-notify turn-ended");
    assert_eq!(hooks[0].source, "config.toml");
    assert_eq!(hooks[0].description, "Legacy notify key.");
}

#[test]
fn codex_plugin_manifest_names_a_hook_file() {
    let root = plugin_dir(
        "on-n-off-hooks-codex-path",
        ".codex-plugin",
        r#"{ "name": "acme", "description": "Acme for Codex", "hooks": "./hooks/codex.json" }"#,
    );
    // The Claude hook file sits beside it and must not be read as Codex's.
    write_hook_file(
        &root,
        "hooks/hooks.json",
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme hook Stop --tool claude" }] }] } }"#,
    );
    write_hook_file(
        &root,
        "hooks/codex.json",
        r#"{ "hooks": { "SessionStart": [{ "hooks": [{ "type": "command", "command": "acme hook SessionStart --tool codex" }] }] } }"#,
    );

    let hooks = codex_plugin_hooks("acme@webapp", "acme", &root);

    assert_eq!(
        ids(&hooks),
        ["acme@webapp:hooks/codex.json:session_start:0:0"]
    );
    assert_eq!(commands(&hooks), ["acme hook SessionStart --tool codex"]);
}

#[test]
fn codex_plugin_manifest_can_hold_a_bare_event_map() {
    let root = plugin_dir(
        "on-n-off-hooks-codex-inline",
        ".codex-plugin",
        r#"{
            "name": "acme",
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "mcp_tool", "server": "acme_repl", "tool": "turn_ended" }] }]
            }
        }"#,
    );

    let hooks = codex_plugin_hooks("acme@webapp", "acme", &root);

    assert_eq!(ids(&hooks), ["acme@webapp:plugin.json#hooks[0]:stop:0:0"]);
    assert_eq!(hooks[0].command, "acme_repl · turn_ended");
}

#[test]
fn codex_plugin_without_a_hooks_key_has_no_hooks() {
    let root = plugin_dir(
        "on-n-off-hooks-codex-none",
        ".codex-plugin",
        r#"{ "name": "acme" }"#,
    );
    write_hook_file(
        &root,
        "hooks/hooks.json",
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme hook Stop --tool claude" }] }] } }"#,
    );

    assert!(codex_plugin_hooks("acme@webapp", "acme", &root).is_empty());
}

#[test]
fn hook_state_switches_one_entry_off() {
    let root = plugin_dir(
        "on-n-off-hooks-codex-state",
        ".codex-plugin",
        r#"{ "name": "acme", "hooks": "./hooks/codex.json" }"#,
    );
    write_hook_file(
        &root,
        "hooks/codex.json",
        r#"{ "hooks": {
            "SessionStart": [{ "hooks": [{ "type": "command", "command": "acme hook SessionStart" }] }],
            "Stop": [{ "hooks": [{ "type": "command", "command": "acme hook Stop" }] }]
        } }"#,
    );
    let mut hooks = codex_plugin_hooks("acme@webapp", "acme", &root);

    apply_codex_state(
        &mut hooks,
        r#"
[hooks.state."acme@webapp:hooks/codex.json:stop:0:0"]
enabled = false
trusted_hash = "sha256:0000"

[hooks.state."acme@webapp:hooks/codex.json:session_start:0:0"]
trusted_hash = "sha256:0001"
"#,
    );

    let state: Vec<(&str, bool)> = hooks
        .iter()
        .map(|hook| (hook.event.as_str(), hook.enabled))
        .collect();
    // A state entry without an `enabled` key is trusted, not disabled.
    assert_eq!(state, [("SessionStart", true), ("Stop", false)]);
}

#[test]
fn the_codex_tab_merges_every_source_and_applies_the_state() {
    let home = crate::paths::scratch_dir("on-n-off-hooks-codex-tab");
    let root = home.join(".codex");
    let plugin = root.join("plugins/cache/webapp/acme/1.0.0");
    fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
    fs::write(
        plugin.join(".codex-plugin/plugin.json"),
        r#"{ "name": "acme", "hooks": "./hooks/codex.json" }"#,
    )
    .unwrap();
    write_hook_file(
        &plugin,
        "hooks/codex.json",
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme hook Stop" }] }] } }"#,
    );
    fs::write(
        root.join("hooks.json"),
        r#"{ "hooks": { "SessionStart": [{ "hooks": [{ "type": "command", "command": "acme user start" }] }] } }"#,
    )
    .unwrap();
    fs::write(
        root.join("config.toml"),
        "notify = [\"/Users/me/bin/acme-notify\", \"turn-ended\"]\n\n\
         [plugins.\"acme@webapp\"]\nenabled = true\n\n\
         [hooks.state.\"acme@webapp:hooks/codex.json:stop:0:0\"]\nenabled = false\n",
    )
    .unwrap();

    let tab = crate::codex::CodexAdapter::at(root, home.join(".agents/skills"))
        .list_local_tab()
        .unwrap();

    let rows: Vec<(&str, &str, bool)> = tab
        .hooks
        .iter()
        .map(|hook| (hook.source.as_str(), hook.command.as_str(), hook.enabled))
        .collect();
    assert_eq!(
        rows,
        [
            ("acme", "acme hook Stop", false),
            ("config.toml", "/Users/me/bin/acme-notify turn-ended", true),
            ("hooks.json", "acme user start", true),
        ]
    );
}

#[test]
fn malformed_files_yield_no_rows_instead_of_an_error() {
    assert!(claude_settings_hooks("{ not json").is_empty());
    assert!(claude_settings_hooks("{}").is_empty());
    assert!(codex_hooks_file("{ not json").is_empty());
    assert!(codex_config_hooks("[hooks").is_empty());
    assert!(codex_config_hooks("").is_empty());

    let root = plugin_dir(
        "on-n-off-hooks-malformed",
        ".claude-plugin",
        r#"{ "name": "acme" }"#,
    );
    write_hook_file(&root, "hooks/hooks.json", "{ not json");
    assert!(claude_plugin_hooks("acme@webapp", "acme", &root).is_empty());

    let broken = plugin_dir(
        "on-n-off-hooks-malformed-manifest",
        ".claude-plugin",
        "{ not",
    );
    write_hook_file(
        &broken,
        "hooks/hooks.json",
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme" }] }] } }"#,
    );
    // An unreadable manifest still leaves the default file in place.
    assert_eq!(
        ids(&claude_plugin_hooks("acme@webapp", "acme", &broken)),
        ["acme@webapp:hooks/hooks.json:stop:0:0"]
    );

    // A state table that will not parse leaves every row as it was.
    let mut hooks = claude_settings_hooks(
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme" }] }] } }"#,
    );
    apply_codex_state(&mut hooks, "[hooks.state");
    assert!(hooks[0].enabled);
}

#[test]
fn ids_stay_unique_and_stable_across_reads() {
    let root = plugin_dir(
        "on-n-off-hooks-ids",
        ".claude-plugin",
        r#"{ "name": "acme", "hooks": ["hooks/first.json", "hooks/second.json"] }"#,
    );
    write_hook_file(
        &root,
        "hooks/first.json",
        r#"{ "hooks": { "Stop": [
            { "matcher": "Bash", "hooks": [
                { "type": "command", "command": "acme one" },
                { "type": "command", "command": "acme two" }
            ] },
            { "hooks": [{ "type": "command", "command": "acme three" }] }
        ] } }"#,
    );
    write_hook_file(
        &root,
        "hooks/second.json",
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme four" }] }] } }"#,
    );

    let first = claude_plugin_hooks("acme@webapp", "acme", &root);
    let again = claude_plugin_hooks("acme@webapp", "acme", &root);

    assert_eq!(
        ids(&first),
        [
            "acme@webapp:hooks/first.json:stop:0:0",
            "acme@webapp:hooks/first.json:stop:0:1",
            "acme@webapp:hooks/first.json:stop:1:0",
            "acme@webapp:hooks/second.json:stop:0:0",
        ]
    );
    assert_eq!(first, again);
    let mut unique: Vec<&str> = ids(&first);
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), first.len());
}

#[test]
fn a_description_or_command_written_across_lines_becomes_one_row_line() {
    let file = r#"{
        "description": "Acme hooks.\nAliases: @acme\n\n  Runs on every turn.",
        "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme run \\\n  --quiet" }] }] }
    }"#;
    let hooks = codex_hooks_file(file);
    assert_eq!(hooks.len(), 1);
    assert_eq!(
        hooks[0].description,
        "Acme hooks. Aliases: @acme Runs on every turn."
    );
    assert_eq!(hooks[0].command, "acme run \\ --quiet");
}

