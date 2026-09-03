use super::*;
use crate::cli_stub::CliStub;

#[test]
fn uppercase_git_marketplace_source_uses_the_clone_cache() {
    let root = PathBuf::from(r"C:\Users\me\.codex");
    let entry = CodexMarketplace {
        source_type: None,
        source: Some("git@github.com:example/workshop.GIT".into()),
    };

    assert_eq!(
        marketplace_path(&root, "workshop", &entry),
        Some(root.join(".tmp").join("marketplaces").join("workshop"))
    );
}

fn fixture() -> (PathBuf, PathBuf) {
    let home = crate::paths::scratch_dir("on-n-off-codex-home");
    let root = home.join(".codex");
    let agents_skills = home.join(".agents").join("skills");
    let plugin = root.join("plugins/cache/workshop/workbench/0.22.1");
    fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
    fs::write(
        plugin.join(".codex-plugin/plugin.json"),
        r#"{"name":"workbench","version":"0.22.1"}"#,
    )
    .unwrap();
    fs::create_dir_all(plugin.join("skills/brainstorming")).unwrap();
    fs::write(
        plugin.join("skills/brainstorming/SKILL.md"),
        "---\nname: brainstorming\ndescription: Turn ideas into designs\n---\n",
    )
    .unwrap();
    fs::create_dir_all(agents_skills.join("loom-feed")).unwrap();
    fs::write(
        agents_skills.join("loom-feed/SKILL.md"),
        "---\nname: loom-feed\ndescription: Feed Loom\n---\n",
    )
    .unwrap();
    let extra = home.join("elsewhere/conoswiki-feed/SKILL.md");
    fs::create_dir_all(extra.parent().unwrap()).unwrap();
    fs::write(
        &extra,
        "---\nname: conoswiki-feed\ndescription: Feed ConosWiki\n---\n",
    )
    .unwrap();
    let marketplace = root.join(".tmp/marketplaces/workshop");
    fs::create_dir_all(marketplace.join(".claude-plugin")).unwrap();
    fs::write(
        marketplace.join(".claude-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"workbench","version":"0.23.0","source":"./plugins/workbench"}]}"#,
    )
    .unwrap();
    fs::write(
        root.join("config.toml"),
        format!(
            "[marketplaces.workshop]\nsource_type = \"git\"\nsource = \"https://example.invalid/workshop.git\"\n\n[plugins.\"workbench@workshop\"]\nenabled = true\n\n[plugins.\"toolkit@workshop\"]\nenabled = false\n\n[[skills.config]]\npath = '{}'\nenabled = false\n\n[mcp_servers.github]\ncommand = \"npx\"\nargs = [\"-y\", \"@modelcontextprotocol/server-github\"]\n\n[mcp_servers.docs]\nurl = \"https://docs.example/mcp\"\nenabled = false\n",
            extra.display()
        ),
    )
    .unwrap();
    (root, agents_skills)
}

#[test]
fn lists_plugins_and_user_skills_from_agents_dir_and_config_paths() {
    let (root, agents_skills) = fixture();
    let adapter = CodexAdapter::at(root.clone(), agents_skills);
    let tab = adapter.list_tab().expect("list");
    assert_eq!(tab.plugins[0].id, "toolkit@workshop");
    assert!(!tab.plugins[0].enabled);
    assert_eq!(tab.plugins[1].id, "workbench@workshop");
    assert!(tab.plugins[1].enabled);
    assert_eq!(tab.plugins[0].version, "");
    assert_eq!(tab.plugins[1].version, "0.22.1");
    assert_eq!(tab.plugins[0].upstream, "");
    assert_eq!(tab.plugins[1].upstream, "0.23.0");
    assert!(tab.plugins[1].out_of_sync);
    assert_eq!(tab.plugins[1].skills.len(), 1);
    let names: Vec<_> = tab
        .user_skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect();
    assert_eq!(names, ["conoswiki-feed", "loom-feed"]);
    let wiki = tab
        .user_skills
        .iter()
        .find(|skill| skill.name == "conoswiki-feed")
        .unwrap();
    assert!(!wiki.enabled);
    let mcp_ids: Vec<_> = tab
        .mcp_servers
        .iter()
        .map(|server| server.id.as_str())
        .collect();
    assert_eq!(mcp_ids, ["docs", "github"]);
    assert!(!tab.mcp_servers[0].enabled);
    assert_eq!(tab.mcp_servers[0].system, "http");
    assert!(tab.mcp_servers[1].enabled);
    assert!(tab.mcp_servers[1].togglable);
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn local_tab_skips_remote_marketplace_enrichment() {
    let (root, agents_skills) = fixture();
    let config_path = root.join("config.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("https://example.invalid/workshop.git", "example/workshop");
    fs::write(&config_path, config).unwrap();
    let adapter = CodexAdapter::at(root.clone(), agents_skills);

    let local = crate::plugin_meta::with_fetch_text(
        |_| panic!("local inventory must not fetch remote metadata"),
        || adapter.list_local_tab().expect("local list"),
    );
    let local_plugin = local
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert_eq!(local_plugin.upstream, "0.23.0");

    let enriched = crate::plugin_meta::with_fetch_text(
        |_| {
            Some(
                r#"{"plugins":[{"name":"workbench","version":"0.24.0","source":"./plugins/workbench"}]}"#
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
    assert_eq!(enriched_plugin.upstream, "0.24.0");
    assert_eq!(enriched_plugin.enabled, local_plugin.enabled);
    assert_eq!(enriched_plugin.skills, local_plugin.skills);

    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn missing_inventory_is_empty_not_a_parse_error() {
    let home = crate::paths::scratch_dir("on-n-off-codex-empty");
    fs::create_dir_all(home.join(".codex")).unwrap();
    let tab = CodexAdapter::at(home.join(".codex"), home.join(".agents/skills"))
        .list_tab()
        .expect("list");
    assert!(tab.plugins.is_empty());
    assert!(tab.user_skills.is_empty());
    assert!(tab.mcp_servers.is_empty());
    let _ = fs::remove_dir_all(home);
}

#[test]
fn toggling_skill_upserts_config_and_keeps_plugins() {
    let (root, agents_skills) = fixture();
    let adapter = CodexAdapter::at(root.clone(), agents_skills);
    let tab = adapter.list_tab().expect("list");
    let loom = tab
        .user_skills
        .iter()
        .find(|skill| skill.name == "loom-feed")
        .unwrap();
    assert!(loom.enabled);
    let loom_id = loom.id.clone();
    let wiki_id = tab
        .user_skills
        .iter()
        .find(|skill| skill.name == "conoswiki-feed")
        .unwrap()
        .id
        .clone();

    let tab = adapter
        .set_skill_enabled(&loom_id, false)
        .expect("toggle loom");
    let loom = tab
        .user_skills
        .iter()
        .find(|skill| skill.name == "loom-feed")
        .unwrap();
    assert!(!loom.enabled);
    let text = fs::read_to_string(root.join("config.toml")).unwrap();
    assert!(text.contains("[plugins.\"workbench@workshop\"]"));
    assert!(text.contains("[plugins.\"toolkit@workshop\"]"));
    assert!(text.contains("[[skills.config]]"));
    assert!(text.contains("loom-feed"));

    let tab = adapter
        .set_skill_enabled(&wiki_id, true)
        .expect("toggle wiki");
    let wiki = tab
        .user_skills
        .iter()
        .find(|skill| skill.name == "conoswiki-feed")
        .unwrap();
    assert!(wiki.enabled);
    assert!(root
        .join("_backups/codex")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn toggling_plugin_patches_toml_enabled_only() {
    let (root, agents_skills) = fixture();
    let adapter = CodexAdapter::at(root.clone(), agents_skills);
    let tab = adapter
        .set_plugin_enabled("workbench@workshop", false)
        .expect("disable");
    let plugin = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert!(!plugin.enabled);
    let text = fs::read_to_string(root.join("config.toml")).unwrap();
    assert!(text.contains("[plugins.\"toolkit@workshop\"]"));
    assert!(text.contains("[[skills.config]]"));
    assert!(root
        .join("_backups/codex")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn toggling_mcp_patches_toml_enabled_only() {
    let (root, agents_skills) = fixture();
    let adapter = CodexAdapter::at(root.clone(), agents_skills);
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
    let text = fs::read_to_string(root.join("config.toml")).unwrap();
    assert!(text.contains("[plugins.\"workbench@workshop\"]"), "{text}");
    assert!(text.contains("[[skills.config]]"), "{text}");
    assert!(text.contains("[mcp_servers.docs]"), "{text}");
    let tab = adapter.set_mcp_enabled("docs", true).expect("on");
    assert!(
        tab.mcp_servers
            .iter()
            .find(|server| server.id == "docs")
            .unwrap()
            .enabled
    );
    let text = fs::read_to_string(root.join("config.toml")).unwrap();
    assert!(
        text.contains("url = \"https://docs.example/mcp\""),
        "{text}"
    );
    assert!(root
        .join("_backups/codex")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

fn codex_argv_stub(root: &Path, exit: i32, stderr: &str) -> AgentCli {
    let dir = root.join("_cli");
    if exit == 0 {
        CliStub::new("codex").log_args("args.txt", false).cli(&dir)
    } else {
        CliStub::new("codex").stderr(stderr).exit(exit).cli(&dir)
    }
}

#[test]
fn update_upgrades_marketplace_then_readds_plugin() {
    let (root, agents_skills) = fixture();
    let adapter = CodexAdapter::at_with_cli(
        root.clone(),
        agents_skills,
        CliStub::new("codex")
            .log_args("args.txt", true)
            .cli(&root.join("_cli")),
    );
    adapter.update_plugin("workbench@workshop").expect("update");
    let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
    assert!(
        args.contains("plugin marketplace upgrade --json workshop"),
        "{args}"
    );
    assert!(
        args.contains("plugin add --json workbench@workshop"),
        "{args}"
    );
    assert!(root
        .join("_backups/codex")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn install_and_uninstall_use_official_argv() {
    let (root, agents_skills) = fixture();
    let adapter =
        CodexAdapter::at_with_cli(root.clone(), agents_skills, codex_argv_stub(&root, 0, ""));
    adapter.install_plugin("workbench@workshop").expect("add");
    let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
    assert!(
        args.contains("plugin add --json workbench@workshop"),
        "{args}"
    );
    adapter.install_plugin("acme/tools@main").expect("market");
    let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
    assert!(
        args.contains("plugin marketplace add --json acme/tools --ref main"),
        "{args}"
    );
    adapter
        .uninstall_plugin("workbench@workshop")
        .expect("remove");
    let args = fs::read_to_string(root.join("_cli/args.txt")).unwrap();
    assert!(
        args.contains("plugin remove --json workbench@workshop"),
        "{args}"
    );
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn install_cli_failure_does_not_mutate_config() {
    let (root, agents_skills) = fixture();
    let before = fs::read_to_string(root.join("config.toml")).unwrap();
    let adapter = CodexAdapter::at_with_cli(
        root.clone(),
        agents_skills,
        codex_argv_stub(&root, 3, "nope"),
    );
    let err = adapter.install_plugin("acme/tools").expect_err("cli");
    assert!(err.message.contains("nope"));
    assert_eq!(
        fs::read_to_string(root.join("config.toml")).unwrap(),
        before
    );
    let _ = fs::remove_dir_all(root.parent().unwrap());
}
