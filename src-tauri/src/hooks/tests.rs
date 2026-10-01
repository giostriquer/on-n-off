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

fn config_hooks(text: &str) -> Vec<HookDto> {
    codex_config_hooks(&parse_toml(text))
}

fn apply_state(hooks: &mut [HookDto], config_toml: &str) {
    apply_codex_state(hooks, &parse_toml(config_toml));
}

fn plugin_dir(prefix: &str, manifest_dir: &str, manifest: &str) -> PathBuf {
    let root = crate::paths::scratch_dir(prefix).join("plugin");
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
    assert_eq!(hooks[0].event, "PreToolUse");
    assert_eq!(hooks[0].matcher, "Bash");
    assert_eq!(hooks[0].handler, "command");
    assert_eq!(hooks[0].source, "settings.json");
    assert_eq!(hooks[0].plugin_id, None);
    assert_eq!(hooks[0].description, "");
    assert!(hooks[0].enabled);
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
    assert_eq!(hooks[0].description, "Acme terminal notifications");
    assert_eq!(hooks[0].matcher, "startup|resume");
    assert_eq!(hooks[0].command, "${CLAUDE_PLUGIN_ROOT}/scripts/start.sh");
}

#[test]
fn claude_plugin_manifest_names_one_hook_file() {
    let root = plugin_dir(
        "on-n-off-hooks-claude-path",
        ".claude-plugin",
        r#"{ "name": "acme", "description": "Acme for webapp", "hooks": "./hooks/claude.json" }"#,
    );
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
    fs::write(
        root.parent().unwrap().join("escape.json"),
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme escaped" }] }] } }"#,
    )
    .unwrap();

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

#[cfg(unix)]
#[test]
fn a_hook_file_named_with_a_drive_is_refused() {
    let root = plugin_dir(
        "on-n-off-hooks-claude-drive",
        ".claude-plugin",
        r#"{ "name": "acme", "hooks": ["C:escape.json", "hooks/kept.json"] }"#,
    );
    write_hook_file(
        &root,
        "C:escape.json",
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme escaped" }] }] } }"#,
    );
    write_hook_file(
        &root,
        "hooks/kept.json",
        r#"{ "hooks": { "SessionEnd": [{ "hooks": [{ "type": "command", "command": "acme kept" }] }] } }"#,
    );

    let hooks = claude_plugin_hooks("acme@webapp", "acme", &root);

    assert_eq!(commands(&hooks), ["acme kept"]);
}

#[test]
fn one_hook_file_named_twice_is_read_once() {
    let root = plugin_dir(
        "on-n-off-hooks-claude-twice",
        ".claude-plugin",
        r#"{ "name": "acme", "hooks": ["hooks/a.json", "./hooks/a.json"] }"#,
    );
    write_hook_file(
        &root,
        "hooks/a.json",
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme once" }] }] } }"#,
    );

    let hooks = claude_plugin_hooks("acme@webapp", "acme", &root);
    assert_eq!(ids(&hooks), ["acme@webapp:hooks/a.json:stop:0:0"]);
}

#[test]
fn two_event_keys_that_snake_case_alike_share_one_state_key() {
    let mut hooks = codex_hooks_file(
        r#"{ "hooks": {
            "Stop": [{ "hooks": [{ "type": "command", "command": "acme upper" }] }],
            "stop": [{ "hooks": [{ "type": "command", "command": "acme lower" }] }]
        } }"#,
    );

    assert_eq!(commands(&hooks), ["acme upper", "acme lower"]);
    assert_eq!(
        ids(&hooks),
        [":hooks.json:stop:0:0", ":hooks.json:stop:0:0"]
    );
    apply_state(
        &mut hooks,
        "[hooks.state.\":hooks.json:stop:0:0\"]\nenabled = false\n",
    );
    assert!(hooks.iter().all(|hook| !hook.enabled));
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
fn a_disabled_claude_plugin_contributes_no_hooks() {
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
fn only_the_providers_with_hook_files_say_they_read_hooks() {
    let home = crate::paths::scratch_dir("on-n-off-hooks-capability");
    let claude = crate::claude::ClaudeAdapter::at(home.join(".claude"));
    let codex = crate::codex::CodexAdapter::at(home.join(".codex"), home.join(".agents/skills"));
    let antigravity = crate::antigravity::AntigravityAdapter::at(home.join(".gemini"));
    let cursor = crate::cursor::CursorAdapter::at(home.join(".cursor"));

    assert!(claude.reads_hooks() && claude.info().reads_hooks);
    assert!(codex.reads_hooks() && codex.info().reads_hooks);
    assert!(!antigravity.reads_hooks() && !antigravity.info().reads_hooks);
    assert!(!cursor.reads_hooks() && !cursor.info().reads_hooks);
}

#[test]
fn a_disabled_codex_plugin_contributes_no_hooks() {
    let home = crate::paths::scratch_dir("on-n-off-hooks-codex-enablement");
    let root = home.join(".codex");
    for (name, command) in [("on", "acme on"), ("off", "acme off")] {
        let plugin = root.join("plugins/cache/webapp").join(name).join("1.0.0");
        fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
        fs::write(
            plugin.join(".codex-plugin/plugin.json"),
            format!(r#"{{ "name": "{name}", "hooks": "./hooks/codex.json" }}"#),
        )
        .unwrap();
        write_hook_file(
            &plugin,
            "hooks/codex.json",
            &format!(
                r#"{{ "hooks": {{ "Stop": [{{ "hooks": [{{ "type": "command", "command": "{command}" }}] }}] }} }}"#
            ),
        );
    }
    fs::write(
        root.join("config.toml"),
        "[plugins.\"on@webapp\"]\nenabled = true\n\n[plugins.\"off@webapp\"]\nenabled = false\n",
    )
    .unwrap();

    let tab = crate::codex::CodexAdapter::at(root, home.join(".agents/skills"))
        .list_local_tab()
        .unwrap();

    assert_eq!(commands(&tab.hooks), ["acme on"]);
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
    let hooks = config_hooks(
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

    assert!(
        config_hooks("[[hooks.state]]\ntype = \"command\"\ncommand = \"acme state\"\n").is_empty()
    );
}

#[test]
fn the_legacy_notify_key_is_a_hook_of_its_own() {
    let hooks = config_hooks(
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
fn notify_can_be_one_string_and_an_empty_one_is_no_row() {
    let hooks = config_hooks("notify = \"/Users/me/bin/acme-notify\"\n");
    assert_eq!(commands(&hooks), ["/Users/me/bin/acme-notify"]);

    assert!(config_hooks("notify = []\n").is_empty());
    assert!(config_hooks("notify = \"   \"\n").is_empty());
    assert!(config_hooks("notify = 7\n").is_empty());
}

#[test]
fn a_bare_toml_handler_is_a_row_and_a_handler_without_a_type_runs_as_a_command() {
    let bare = config_hooks("[[hooks.Stop]]\ntype = \"command\"\ncommand = \"acme bare\"\n");
    assert_eq!(ids(&bare), [":config.toml:stop:0:0"]);
    assert_eq!(commands(&bare), ["acme bare"]);

    let untyped = codex_hooks_file(
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "command": "acme untyped" }] }] } }"#,
    );
    assert_eq!(untyped[0].handler, "command");
    assert_eq!(untyped[0].command, "acme untyped");
}

#[test]
fn a_handler_with_no_command_line_shows_whatever_it_names_instead() {
    let hooks = codex_hooks_file(
        r#"{ "hooks": { "Stop": [{ "hooks": [
            { "type": "http", "url": "https://acme.example/hooks/stop" },
            { "type": "mcp_tool", "server": "acme", "tool": "turn_ended" },
            { "type": "mcp_tool", "server": "acme" },
            { "type": "mcp_tool", "tool": "turn_ended" },
            { "type": "mcp_tool" }
        ] }] } }"#,
    );

    let rows: Vec<(&str, &str)> = hooks
        .iter()
        .map(|hook| (hook.handler.as_str(), hook.command.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("http", "https://acme.example/hooks/stop"),
            ("mcp_tool", "acme · turn_ended"),
            ("mcp_tool", "acme"),
            ("mcp_tool", "turn_ended"),
            ("mcp_tool", ""),
        ]
    );
}

#[test]
fn rows_keep_the_order_the_file_writes_its_events_in() {
    let hooks = codex_hooks_file(
        r#"{ "hooks": {
            "Stop": [{ "hooks": [{ "type": "command", "command": "acme stop" }] }],
            "PreToolUse": [{ "hooks": [{ "type": "command", "command": "acme pre" }] }],
            "Notification": [{ "hooks": [{ "type": "command", "command": "acme notify" }] }]
        } }"#,
    );

    assert_eq!(commands(&hooks), ["acme stop", "acme pre", "acme notify"]);
}

#[test]
fn codex_plugin_manifest_names_a_hook_file() {
    let root = plugin_dir(
        "on-n-off-hooks-codex-path",
        ".codex-plugin",
        r#"{ "name": "acme", "description": "Acme for Codex", "hooks": "./hooks/codex.json" }"#,
    );
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

    apply_state(
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
         [[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = \"command\"\ncommand = \"acme table stop\"\n\n\
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
            ("config.toml", "acme table stop", true),
            ("hooks.json", "acme user start", true),
        ]
    );
    let mut unique = ids(&tab.hooks);
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), tab.hooks.len());
}

#[test]
fn malformed_files_yield_no_rows_instead_of_an_error() {
    assert!(claude_settings_hooks("{ not json").is_empty());
    assert!(claude_settings_hooks("{}").is_empty());
    assert!(codex_hooks_file("{ not json").is_empty());
    assert!(config_hooks("[hooks").is_empty());
    assert!(config_hooks("").is_empty());

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
    assert_eq!(
        ids(&claude_plugin_hooks("acme@webapp", "acme", &broken)),
        ["acme@webapp:hooks/hooks.json:stop:0:0"]
    );

    let mut hooks = claude_settings_hooks(
        r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "acme" }] }] } }"#,
    );
    apply_state(&mut hooks, "[hooks.state");
    assert!(hooks[0].enabled);
}

#[test]
fn an_edit_that_moves_no_entry_leaves_every_id_alone() {
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
    let before: Vec<String> = first.iter().map(|hook| hook.id.clone()).collect();
    assert_eq!(
        before,
        [
            "acme@webapp:hooks/first.json:stop:0:0",
            "acme@webapp:hooks/first.json:stop:0:1",
            "acme@webapp:hooks/first.json:stop:1:0",
            "acme@webapp:hooks/second.json:stop:0:0",
        ]
    );

    write_hook_file(
        &root,
        "hooks/first.json",
        r#"{ "hooks": {
            "SessionStart": [{ "hooks": [{ "type": "command", "command": "acme start" }] }],
            "Stop": [
                { "matcher": "Bash", "hooks": [
                    { "type": "command", "command": "acme one --quiet" },
                    { "type": "command", "command": "acme two" }
                ] },
                { "hooks": [{ "type": "command", "command": "acme three" }] }
            ],
            "SessionEnd": [{ "hooks": [{ "type": "command", "command": "acme end" }] }]
        } }"#,
    );

    let after = claude_plugin_hooks("acme@webapp", "acme", &root);
    let surviving: Vec<&str> = ids(&after)
        .into_iter()
        .filter(|id| id.contains(":stop:"))
        .collect();
    assert_eq!(surviving, before);
    assert_eq!(
        after
            .iter()
            .find(|hook| hook.id == before[0])
            .map(|hook| hook.command.as_str()),
        Some("acme one --quiet")
    );
}

#[test]
fn a_description_becomes_one_line_and_a_command_keeps_the_one_it_has() {
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
    assert_eq!(hooks[0].command, "acme run \\\n  --quiet");
}
