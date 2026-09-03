use super::*;
use crate::cli_stub::CliStub;

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
    let tab = adapter.list_tab().expect("list");
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

    let local = crate::plugin_meta::with_fetch_text(
        |_| panic!("local inventory must not fetch remote metadata"),
        || adapter.list_local_tab().expect("local list"),
    );
    let local_plugin = local
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert_eq!(local_plugin.upstream, "1.1.0");

    let enriched = crate::plugin_meta::with_fetch_text(
        |_| {
            Some(
                r#"{"plugins":[{"name":"workbench","version":"2.0.0","source":"./plugins/workbench"}]}"#
                    .into(),
            )
        },
        || adapter.list_tab().expect("enriched list"),
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
    let err = adapter
        .set_skill_enabled("workbench@workshop:brainstorming", false)
        .expect_err("locked");
    assert!(err.message.contains("not togglable"));
    let settings = fs::read_to_string(root.join("settings.json")).unwrap();
    assert!(!settings.contains("brainstorming"));

    let tab = adapter
        .set_skill_enabled("statusline", true)
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
    let tab = adapter.set_mcp_enabled("github", true).expect("on");
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
    let tab = adapter
        .set_plugin_enabled("workbench@workshop", false)
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
    let err = adapter
        .set_plugin_enabled("workbench@workshop", false)
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
    adapter
        .install_plugin("workbench@workshop")
        .expect("plugin");
    let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
    assert!(
        args.contains("plugin install -s user -y workbench@workshop"),
        "{args}"
    );
    adapter.install_plugin("acme/tools").expect("git");
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
    let err = adapter.install_plugin("acme/tools").expect_err("cli");
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
    adapter.update_plugin("workbench@workshop").expect("update");
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
    adapter
        .uninstall_plugin("workbench@workshop")
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
