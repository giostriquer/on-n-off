use super::*;
use crate::change::{argv_of, row, Changes, Request};
use crate::cli_stub::CliStub;
use crate::config_io::{patched, Edit};

fn fixture() -> PathBuf {
    let root = crate::paths::scratch_dir("on-n-off-claude");
    let plugin = root.join("plugins/cache/workshop/workbench/1.0.0");
    fs::create_dir_all(plugin.join("skills/brainstorming")).unwrap();
    fs::write(
        plugin.join("skills/brainstorming/SKILL.md"),
        "---\nname: brainstorming\ndescription: Turn ideas into designs\n---\n",
    )
    .unwrap();
    let opt_in = root.join("plugins/cache/opt/quiet/1.0.0");
    fs::create_dir_all(opt_in.join(".claude-plugin")).unwrap();
    fs::write(
        opt_in.join(".claude-plugin/plugin.json"),
        r#"{"name":"quiet","defaultEnabled":false}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("plugins/marketplaces/workshop/.claude-plugin")).unwrap();
    fs::write(
        root.join("plugins/marketplaces/workshop/.claude-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"workbench","version":"1.1.0","source":"./plugins/workbench"}]}"#,
    )
    .unwrap();
    fs::write(
        root.join("plugins/known_marketplaces.json"),
        serde_json::json!({
            "workshop": {
                "installLocation": root.join("plugins/marketplaces/workshop")
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::create_dir_all(root.join("plugins")).unwrap();
    fs::write(
        root.join("plugins/installed_plugins.json"),
        serde_json::json!({
            "version": 2,
            "plugins": {
                "workbench@workshop": [{
                    "scope": "user",
                    "installPath": plugin,
                    "version": "1.0.0"
                }],
                "superpowers@claude-plugins-official": [{
                    "scope": "user",
                    "installPath": root.join("plugins/cache/missing")
                }],
                "quiet@opt": [{
                    "scope": "user",
                    "installPath": opt_in
                }],
                "project-only@team": [{
                    "scope": "project",
                    "installPath": root.join("plugins/cache/team/project-only/1.0.0")
                }]
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("settings.json"),
        serde_json::json!({
            "enabledPlugins": {
                "workbench@workshop": true,
                "superpowers@claude-plugins-official": false
            },
            "skillOverrides": { "statusline": "off" }
        })
        .to_string(),
    )
    .unwrap();
    fs::create_dir_all(root.join("skills/statusline")).unwrap();
    fs::write(
        root.join("skills/statusline/SKILL.md"),
        "---\nname: statusline\ndescription: Custom status line\n---\n",
    )
    .unwrap();
    fs::write(
        root.join(".claude.json"),
        serde_json::json!({
            "mcpServers": {
                "github": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-github"]
                },
                "Docs": { "type": "http", "url": "https://docs.example/mcp" }
            }
        })
        .to_string(),
    )
    .unwrap();
    root
}

#[test]
fn lists_user_scope_plugins_using_default_enabled_when_absent() {
    let root = fixture();
    let adapter = ClaudeAdapter::at(root.clone());
    let tab = crate::drift::read(&adapter, None, crate::drift::Reach::Local).expect("list");
    let ids: Vec<_> = tab
        .plugins
        .iter()
        .map(|plugin| plugin.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "superpowers@claude-plugins-official",
            "quiet@opt",
            "workbench@workshop"
        ]
    );
    let quiet = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "quiet@opt")
        .unwrap();
    let superpowers = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "superpowers@claude-plugins-official")
        .unwrap();
    let workbench = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert!(!quiet.enabled);
    assert!(!superpowers.enabled);
    assert!(workbench.enabled);
    assert_eq!(workbench.skills[0].id, "workbench@workshop:brainstorming");
    assert!(!workbench.skills[0].togglable);
    assert_eq!(quiet.version, "1.0.0");
    assert_eq!(superpowers.version, "");
    assert_eq!(workbench.version, "1.0.0");
    assert_eq!(quiet.upstream, "");
    assert_eq!(superpowers.upstream, "");
    assert_eq!(workbench.upstream, "1.1.0");
    assert!(workbench.out_of_sync);
    assert_eq!(tab.user_skills[0].id, "statusline");
    assert!(!tab.user_skills[0].enabled);
    let mcp_ids: Vec<_> = tab
        .mcp_servers
        .iter()
        .map(|server| server.id.as_str())
        .collect();
    assert_eq!(mcp_ids, ["Docs", "github"]);
    assert_eq!(tab.mcp_servers[0].system, "http");
    assert!(tab.mcp_servers[1].enabled);
    assert!(tab.mcp_servers[1].togglable);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tab_skips_remote_marketplace_enrichment() {
    let root = fixture();
    let known_path = root.join("plugins/known_marketplaces.json");
    let mut known: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&known_path).unwrap()).unwrap();
    known["workshop"]["source"] = serde_json::json!({ "repo": "example/workshop" });
    fs::write(&known_path, known.to_string()).unwrap();
    let adapter = ClaudeAdapter::at(root.clone());

    let local = crate::drift::with_fetch_text(
        |_| panic!("local inventory must not fetch remote metadata"),
        || crate::drift::read(&adapter, None, crate::drift::Reach::Local).expect("local list"),
    );
    let local_plugin = local
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert_eq!(local_plugin.upstream, "1.1.0");

    let memo = crate::drift::Memo::new();
    let enriched = crate::drift::with_fetch_text(
        |_| {
            Ok(Some(
                r#"{"plugins":[{"name":"workbench","version":"2.0.0","source":"./plugins/workbench"}]}"#
                    .into(),
            ))
        },
        || {
            crate::drift::read(&adapter, None, crate::drift::Reach::Remote(&memo))
                .expect("enriched list")
        },
    );
    let enriched_plugin = enriched
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert_eq!(enriched_plugin.upstream, "2.0.0");
    assert_eq!(enriched_plugin.enabled, local_plugin.enabled);
    assert_eq!(enriched_plugin.skills, local_plugin.skills);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_inventory_is_empty_not_a_parse_error() {
    let root = crate::paths::scratch_dir("on-n-off-claude-empty");
    let tab = ClaudeAdapter::at(root.clone()).list_tab().expect("list");
    assert!(tab.plugins.is_empty());
    assert!(tab.user_skills.is_empty());
    assert!(tab.mcp_servers.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn toggling_user_skill_patches_skill_overrides_and_refuses_plugin_skills() {
    let root = fixture();
    let adapter = ClaudeAdapter::at(root.clone());
    let changes = Changes::beside(&root);
    let err = changes
        .global(
            &adapter,
            Request::skill("workbench@workshop:brainstorming", false),
        )
        .expect_err("locked");
    assert!(err.message.contains("not togglable"));
    let settings = fs::read_to_string(root.join("settings.json")).unwrap();
    assert!(!settings.contains("brainstorming"));

    let tab = changes
        .global(&adapter, Request::skill("statusline", true))
        .expect("toggle");
    assert!(tab.user_skills[0].enabled);
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("settings.json")).unwrap()).unwrap();
    assert_eq!(value["skillOverrides"]["statusline"], "on");
    assert_eq!(value["enabledPlugins"]["workbench@workshop"], true);
    assert!(root
        .join("_backups/claude")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn toggling_mcp_patches_claude_json_without_dropping_other_servers() {
    let root = fixture();
    let adapter = ClaudeAdapter::at(root.clone());
    let changes = Changes::beside(&root);
    let err = changes
        .global(&adapter, Request::mcp("missing", false))
        .expect_err("missing");
    assert!(err.message.contains("mcp server not found"));
    let tab = changes
        .global(&adapter, Request::mcp("github", false))
        .expect("toggle");
    let github = tab
        .mcp_servers
        .iter()
        .find(|server| server.id == "github")
        .unwrap();
    assert!(!github.enabled);
    assert!(github.togglable);
    let docs = tab
        .mcp_servers
        .iter()
        .find(|server| server.id == "Docs")
        .unwrap();
    assert!(docs.enabled);
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(".claude.json")).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["github"]["disabled"], true);
    assert_eq!(value["mcpServers"]["github"]["command"], "npx");
    assert_eq!(
        value["mcpServers"]["Docs"]["url"],
        "https://docs.example/mcp"
    );
    assert_eq!(value["disabledMcpServers"], serde_json::json!(["github"]));
    let tab = changes
        .global(&adapter, Request::mcp("github", true))
        .expect("on");
    assert!(
        tab.mcp_servers
            .iter()
            .find(|server| server.id == "github")
            .unwrap()
            .enabled
    );
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(".claude.json")).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["github"]["disabled"], false);
    assert_eq!(value["disabledMcpServers"], serde_json::json!([]));
    let _ = fs::remove_dir_all(root);
}

fn claude_stub(root: &Path, after_settings: &str, exit: i32, stderr: &str) -> AgentCli {
    let dir = root.join("_cli");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("after.json"), after_settings).unwrap();
    if exit == 0 {
        CliStub::new("claude")
            .copy("after.json", "../settings.json")
            .log_args("args.txt", false)
            .cli(&dir)
    } else {
        CliStub::new("claude").stderr(stderr).exit(exit).cli(&dir)
    }
}

#[test]
fn plugin_disable_via_cli_refreshes_dto_and_backs_up() {
    let root = fixture();
    let after = serde_json::json!({
        "enabledPlugins": {
            "workbench@workshop": false,
            "superpowers@claude-plugins-official": false
        },
        "skillOverrides": { "statusline": "off" }
    })
    .to_string();
    let adapter = ClaudeAdapter::at_with_cli(root.clone(), claude_stub(&root, &after, 0, ""));
    let changes = Changes::beside(&root);
    let tab = changes
        .global(&adapter, Request::plugin("workbench@workshop", false))
        .expect("disable");
    let plugin = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert!(!plugin.enabled);
    let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
    assert!(
        args.contains("plugin disable -s user workbench@workshop"),
        "{args}"
    );
    assert!(root
        .join("_backups/claude")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn plugin_cli_failure_does_not_mutate_settings() {
    let root = fixture();
    let before = fs::read_to_string(root.join("settings.json")).unwrap();
    let adapter = ClaudeAdapter::at_with_cli(root.clone(), claude_stub(&root, "{}", 2, "nope"));
    let changes = Changes::beside(&root);
    let err = changes
        .global(&adapter, Request::plugin("workbench@workshop", false))
        .expect_err("cli");
    assert!(err.message.contains("nope"));
    assert_eq!(
        fs::read_to_string(root.join("settings.json")).unwrap(),
        before
    );
    let _ = fs::remove_dir_all(root);
}

fn claude_argv_stub(root: &Path, exit: i32, stderr: &str) -> AgentCli {
    let dir = root.join("_cli");
    if exit == 0 {
        CliStub::new("claude").log_args("args.txt", false).cli(&dir)
    } else {
        CliStub::new("claude").stderr(stderr).exit(exit).cli(&dir)
    }
}

#[test]
fn install_plugin_id_and_git_source_use_official_argv() {
    let root = fixture();
    let adapter = ClaudeAdapter::at_with_cli(root.clone(), claude_argv_stub(&root, 0, ""));
    let changes = Changes::beside(&root);
    changes
        .global(&adapter, Request::install("workbench@workshop"))
        .expect("plugin");
    let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
    assert!(
        args.contains("plugin install -s user -y workbench@workshop"),
        "{args}"
    );
    changes
        .global(&adapter, Request::install("acme/tools"))
        .expect("git");
    let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
    assert!(
        args.contains("plugin marketplace add --scope user acme/tools"),
        "{args}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn install_cli_failure_does_not_mutate_settings() {
    let root = fixture();
    let before = fs::read_to_string(root.join("settings.json")).unwrap();
    let adapter = ClaudeAdapter::at_with_cli(root.clone(), claude_argv_stub(&root, 2, "denied"));
    let changes = Changes::beside(&root);
    let err = changes
        .global(&adapter, Request::install("acme/tools"))
        .expect_err("cli");
    assert!(err.message.contains("denied"));
    assert_eq!(
        fs::read_to_string(root.join("settings.json")).unwrap(),
        before
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn update_refreshes_marketplace_then_plugin() {
    let root = fixture();
    let adapter = ClaudeAdapter::at_with_cli(
        root.clone(),
        CliStub::new("claude")
            .log_args("args.txt", true)
            .cli(&root.join("_cli")),
    );
    let changes = Changes::beside(&root);
    changes
        .global(&adapter, Request::update("workbench@workshop"))
        .expect("update");
    let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
    assert!(
        args.contains("plugin marketplace update workshop"),
        "{args}"
    );
    assert!(
        args.contains("plugin update -s user -y workbench@workshop"),
        "{args}"
    );
    assert!(root
        .join("_backups/claude")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn uninstall_uses_official_argv_and_backs_up() {
    let root = fixture();
    let adapter = ClaudeAdapter::at_with_cli(root.clone(), claude_argv_stub(&root, 0, ""));
    let changes = Changes::beside(&root);
    changes
        .global(&adapter, Request::uninstall("workbench@workshop"))
        .expect("uninstall");
    let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
    assert!(
        args.contains("plugin uninstall -s user -y workbench@workshop"),
        "{args}"
    );
    assert!(root
        .join("_backups/claude")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let _ = fs::remove_dir_all(root);
}

fn write_plugin_mcp(plugin_root: &Path, body: serde_json::Value) {
    fs::create_dir_all(plugin_root).unwrap();
    fs::write(plugin_root.join(".mcp.json"), body.to_string()).unwrap();
}

#[test]
fn lists_the_mcp_servers_enabled_plugins_bring_read_only() {
    let root = fixture();
    write_plugin_mcp(
        &root.join("plugins/cache/workshop/workbench/1.0.0"),
        serde_json::json!({
            "docs-search": {
                "command": "${CLAUDE_PLUGIN_ROOT}/bin/search",
                "args": ["--stdio"]
            }
        }),
    );
    write_plugin_mcp(
        &root.join("plugins/cache/opt/quiet/1.0.0"),
        serde_json::json!({ "mcpServers": { "hidden": { "type": "http", "url": "https://quiet.example/mcp" } } }),
    );

    let tab = ClaudeAdapter::at(root.clone()).list_tab().expect("list");

    let plugin_rows: Vec<_> = tab
        .mcp_servers
        .iter()
        .filter(|server| server.origin == "plugin")
        .collect();
    assert_eq!(plugin_rows.len(), 1, "{:?}", tab.mcp_servers);
    let row = plugin_rows[0];
    assert_eq!(row.id, "plugin:workbench:docs-search");
    assert_eq!(row.name, "docs-search");
    assert_eq!(row.plugin_id.as_deref(), Some("workbench@workshop"));
    assert!(row.projects.is_empty());
    assert_eq!(row.source, "${CLAUDE_PLUGIN_ROOT}/bin/search --stdio");
    assert!(row.enabled);
    assert!(!row.togglable);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn lists_servers_kept_for_particular_projects_once_each() {
    let root = fixture();
    let docs = serde_json::json!({ "type": "http", "url": "https://docs.example/mcp" });
    fs::write(
        root.join(".claude.json"),
        serde_json::json!({
            "mcpServers": {},
            "projects": {
                "/Users/me/acme/webapp": { "mcpServers": { "library-docs": docs } },
                "/Users/me/acme/api": { "mcpServers": { "library-docs": docs } },
                "/Users/me/acme/notes": {
                    "mcpServers": { "scratchpad": { "command": "node", "args": ["pad.js"] } },
                    "disabledMcpServers": ["scratchpad"]
                },
                "/Users/me/acme/empty": { "mcpServers": {} }
            }
        })
        .to_string(),
    )
    .unwrap();

    let tab = ClaudeAdapter::at(root.clone()).list_tab().expect("list");

    let local: Vec<_> = tab
        .mcp_servers
        .iter()
        .filter(|server| server.origin == "local")
        .map(|server| {
            (
                server.id.as_str(),
                server.projects.join(" "),
                server.enabled,
                server.togglable,
            )
        })
        .collect();
    assert_eq!(
        local,
        [
            (
                "local:library-docs",
                "/Users/me/acme/api /Users/me/acme/webapp".to_string(),
                true,
                false
            ),
            (
                "local:scratchpad",
                "/Users/me/acme/notes".to_string(),
                false,
                false
            ),
        ]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_top_level_disabled_list_does_not_switch_off_a_plugin_server() {
    let root = fixture();
    write_plugin_mcp(
        &root.join("plugins/cache/workshop/workbench/1.0.0"),
        serde_json::json!({ "docs-search": { "command": "search" } }),
    );
    fs::write(
        root.join(".claude.json"),
        serde_json::json!({
            "mcpServers": {},
            "disabledMcpServers": ["plugin:workbench:docs-search"]
        })
        .to_string(),
    )
    .unwrap();

    let tab = ClaudeAdapter::at(root.clone()).list_tab().expect("list");

    let row = tab
        .mcp_servers
        .iter()
        .find(|server| server.id == "plugin:workbench:docs-search")
        .unwrap();
    assert!(row.enabled);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn toggling_a_plugin_or_per_project_server_is_refused_without_a_write() {
    let root = fixture();
    write_plugin_mcp(
        &root.join("plugins/cache/workshop/workbench/1.0.0"),
        serde_json::json!({ "docs-search": { "command": "search" } }),
    );
    let config = serde_json::json!({
        "mcpServers": {},
        "projects": { "/Users/me/acme/webapp": { "mcpServers": { "pad": { "command": "pad" } } } }
    })
    .to_string();
    fs::write(root.join(".claude.json"), &config).unwrap();
    let adapter = ClaudeAdapter::at(root.clone());

    let changes = Changes::beside(&root);
    for id in ["plugin:workbench:docs-search", "local:pad"] {
        assert!(
            changes.global(&adapter, Request::mcp(id, false)).is_err(),
            "{id}"
        );
    }

    assert_eq!(
        fs::read_to_string(root.join(".claude.json")).unwrap(),
        config
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn every_change_backs_up_the_file_it_rewrites_once_before_it_is_made() {
    let root = fixture();
    let adapter = ClaudeAdapter::at_with_cli(root.clone(), claude_argv_stub(&root, 0, ""));
    let backups = root.join("_backups");
    let kept = |file: &str| crate::backup::kept(&backups, AgentId::Claude, file).len();

    let changes = Changes::beside(&root);
    changes
        .global(&adapter, Request::skill("statusline", true))
        .unwrap();
    assert_eq!((kept("settings.json"), kept(".claude.json")), (1, 0));
    changes
        .global(&adapter, Request::mcp("github", false))
        .unwrap();
    assert_eq!((kept("settings.json"), kept(".claude.json")), (1, 1));
    changes
        .global(&adapter, Request::plugin("workbench@workshop", false))
        .unwrap();
    changes
        .global(&adapter, Request::install("workbench@workshop"))
        .unwrap();
    changes
        .global(&adapter, Request::uninstall("workbench@workshop"))
        .unwrap();
    changes
        .global(&adapter, Request::update("workbench@workshop"))
        .unwrap();
    assert_eq!((kept("settings.json"), kept(".claude.json")), (5, 1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn json_patch_only_changes_skill_overrides_and_keeps_unrelated_keys() {
    let root = crate::paths::scratch_dir("on-n-off-json");
    let path = root.join("settings.json");
    fs::write(
        &path,
        r#"{
  "model": "opus",
  "enabledPlugins": { "workbench@workshop": true },
  "skillOverrides": { "legacy": "name-only" }
}"#,
    )
    .unwrap();
    patched(
        AgentId::Claude,
        &path,
        Edit::json(|root| skill_override(root, "statusline", false)),
    )
    .unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["model"], "opus");
    assert_eq!(value["enabledPlugins"]["workbench@workshop"], true);
    assert_eq!(value["skillOverrides"]["legacy"], "name-only");
    assert_eq!(value["skillOverrides"]["statusline"], "off");
    assert!(root
        .join("backups/claude")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn json_mcp_toggle_sets_disabled_flag_and_list() {
    let root = crate::paths::scratch_dir("on-n-off-json-mcp");
    let path = root.join(".claude.json");
    fs::write(
        &path,
        r#"{
  "numStartups": 3,
  "mcpServers": {
    "github": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"] },
    "docs": { "type": "http", "url": "https://docs.example/mcp" }
  }
}"#,
    )
    .unwrap();
    patched(
        AgentId::Claude,
        &path,
        Edit::json(|root| mcp_enabled(root, "github", false)),
    )
    .unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["numStartups"], 3);
    assert_eq!(
        value["mcpServers"]["docs"]["url"],
        "https://docs.example/mcp"
    );
    assert_eq!(value["mcpServers"]["github"]["disabled"], true);
    assert_eq!(value["mcpServers"]["github"]["command"], "npx");
    assert_eq!(value["disabledMcpServers"], serde_json::json!(["github"]));
    patched(
        AgentId::Claude,
        &path,
        Edit::json(|root| mcp_enabled(root, "github", true)),
    )
    .unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["github"]["disabled"], false);
    assert_eq!(value["disabledMcpServers"], serde_json::json!([]));
    let err = patched(
        AgentId::Claude,
        &path,
        Edit::json(|root| mcp_enabled(root, "missing", false)),
    )
    .expect_err("missing");
    assert!(err.message.contains("mcp server not found"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_steps_use_the_argv_its_help_documents() {
    let root = crate::paths::scratch_dir("on-n-off-claude-argv");
    let adapter = ClaudeAdapter::at(root.clone());
    let plugin = row("workbench@workshop");
    let git = InstallSource::GitHub {
        owner: "acme".into(),
        repo: "tools".into(),
        ref_name: Some("main".into()),
    };
    assert_eq!(
        argv_of(
            &adapter,
            &Checked::Install(InstallSource::Plugin(plugin.id.clone()))
        ),
        [[
            "plugin",
            "install",
            "-s",
            "user",
            "-y",
            "workbench@workshop"
        ]]
    );
    assert_eq!(
        argv_of(&adapter, &Checked::Install(git)),
        [[
            "plugin",
            "marketplace",
            "add",
            "--scope",
            "user",
            "acme/tools@main"
        ]]
    );
    assert_eq!(
        argv_of(&adapter, &Checked::Uninstall(plugin.clone())),
        [[
            "plugin",
            "uninstall",
            "-s",
            "user",
            "-y",
            "workbench@workshop"
        ]]
    );
    assert_eq!(
        argv_of(&adapter, &Checked::Update(plugin.clone())),
        [
            argv(&["plugin", "marketplace", "update", "workshop"]),
            argv(&["plugin", "update", "-s", "user", "-y", "workbench@workshop"])
        ]
    );
    let npx = InstallSource::NpxSkills {
        source: "acme/skills".into(),
        skill: None,
    };
    let step = adapter.step(&Checked::Install(npx)).unwrap().unwrap();
    assert_eq!(step.npx_agent(), Some("claude-code"));
    assert!(step.touches().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_cli_change_backs_up_every_document_the_plugins_are_read_from() {
    let root = fixture();
    let adapter = ClaudeAdapter::at(root.clone());
    let documents = adapter.documents().unwrap().all();
    let mut read = documents.clone();
    read.sort();
    let plugin = row("workbench@workshop");
    for change in [
        Checked::Plugin {
            plugin: plugin.clone(),
            on: false,
        },
        Checked::Install(InstallSource::Plugin("acme@webapp".into())),
        Checked::Install(InstallSource::GitUrl(
            "https://github.com/acme/webapp.git".into(),
        )),
        Checked::Uninstall(plugin.clone()),
        Checked::Update(plugin.clone()),
    ] {
        let step = adapter.step(&change).unwrap().expect("a step");
        let mut touched = step.touches().to_vec();
        touched.sort();
        assert_eq!(touched, read, "{change:?}");
    }

    let listed = crate::drift::read(&adapter, None, crate::drift::Reach::Local).unwrap();
    for document in documents {
        let kept = fs::read(&document).unwrap();
        fs::write(&document, "{}").unwrap();
        assert_ne!(
            crate::drift::read(&adapter, None, crate::drift::Reach::Local).ok(),
            Some(listed.clone()),
            "the plugins are not read from {}",
            document.display()
        );
        fs::write(&document, kept).unwrap();
    }
    let _ = fs::remove_dir_all(root);
}
