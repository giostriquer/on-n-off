use super::*;
use crate::cli_stub::CliStub;

fn write_plugin(root: &Path, name: &str, skill: Option<&str>) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("plugin.json"), format!(r#"{{"name":"{name}"}}"#)).unwrap();
    if let Some(skill_name) = skill {
        let skill_dir = dir.join("skills").join(skill_name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {skill_name}\ndescription: Test\n---\n"),
        )
        .unwrap();
    }
}

#[test]
fn lists_cli_and_config_plugins_with_skills_and_mcp() {
    let gemini = crate::paths::scratch_dir("on-n-off-agy-list");
    write_plugin(
        &gemini.join("antigravity-cli").join("plugins"),
        "alpha",
        Some("pack"),
    );
    write_plugin(&gemini.join("config").join("plugins"), "beta", None);
    fs::create_dir_all(gemini.join("antigravity-cli").join("skills")).unwrap();
    fs::write(
        gemini
            .join("antigravity-cli")
            .join("skills")
            .join("solo.md"),
        "---\nname: solo\ndescription: Flat\n---\n",
    )
    .unwrap();
    fs::create_dir_all(gemini.join("config")).unwrap();
    fs::write(
        gemini.join("config").join("mcp_config.json"),
        r#"{"mcpServers":{"docs":{"serverUrl":"https://docs.example/mcp/"}}}"#,
    )
    .unwrap();
    fs::write(
        gemini.join("antigravity-cli").join("config.json"),
        r#"{"plugins":{"alpha":{"enabled":false}}}"#,
    )
    .unwrap();

    let tab = AntigravityAdapter::at(gemini.clone())
        .list_tab()
        .expect("list");
    let alpha = tab.plugins.iter().find(|p| p.id == "alpha@cli").unwrap();
    assert!(!alpha.enabled);
    assert_eq!(alpha.skills.len(), 1);
    let beta = tab.plugins.iter().find(|p| p.id == "beta@config").unwrap();
    assert!(beta.enabled);
    assert!(tab.user_skills.iter().any(|s| s.name == "solo"));
    assert!(!tab.user_skills[0].togglable);
    assert_eq!(tab.mcp_servers.len(), 1);
    assert_eq!(tab.mcp_servers[0].id, "docs");
    let _ = fs::remove_dir_all(gemini);
}

#[test]
fn enable_uses_agy_for_cli_plugins_and_blocks_config() {
    let gemini = crate::paths::scratch_dir("on-n-off-agy-toggle");
    write_plugin(
        &gemini.join("antigravity-cli").join("plugins"),
        "alpha",
        None,
    );
    write_plugin(&gemini.join("config").join("plugins"), "beta", None);
    let argv = gemini.join("argv.txt");
    let cli = CliStub::new("agy").log_args("argv.txt", false).cli(&gemini);

    let adapter = AntigravityAdapter::at_with_cli(gemini.clone(), cli);
    adapter
        .set_plugin_enabled("alpha@cli", false)
        .expect("toggle");
    let logged = fs::read_to_string(&argv).unwrap();
    assert!(logged.contains("plugin"), "{logged}");
    assert!(logged.contains("disable"), "{logged}");
    assert!(logged.contains("alpha"), "{logged}");

    let err = adapter
        .set_plugin_enabled("beta@config", false)
        .expect_err("config not togglable");
    assert!(err.message.contains("not togglable"));
    let _ = fs::remove_dir_all(gemini);
}

#[test]
fn merge_enablement_reads_disabled_list_and_imports() {
    let mut map = HashMap::new();
    merge_enablement(
        &mut map,
        r#"{
                "disabledPlugins": ["a"],
                "imports": [{"name":"b","disabled":true},{"name":"c"}]
            }"#,
    );
    assert_eq!(map.get("a"), Some(&false));
    assert_eq!(map.get("b"), Some(&false));
    assert_eq!(map.get("c"), Some(&true));
}

#[test]
fn workspace_plugin_scan_prefixes_project() {
    let root = crate::paths::scratch_dir("on-n-off-agy-ws");
    write_plugin(&root.join(".agents").join("plugins"), "ws", Some("local"));
    let plugins = scan_workspace_plugins(&root);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].id, "project:ws@workspace");
    assert_eq!(plugins[0].skills[0].origin, crate::project::ORIGIN_PROJECT);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn mcp_toggle_patches_disabled() {
    let gemini = crate::paths::scratch_dir("on-n-off-agy-mcp-toggle");
    fs::create_dir_all(gemini.join("config")).unwrap();
    fs::write(
        gemini.join("config").join("mcp_config.json"),
        r#"{"mcpServers":{"github":{"command":"npx","args":["-y","gh"]}}}"#,
    )
    .unwrap();
    let adapter = AntigravityAdapter::at(gemini.clone());
    let tab = adapter.set_mcp_enabled("github", false).expect("toggle");
    assert!(!tab.mcp_servers[0].enabled);
    let text = fs::read_to_string(gemini.join("config").join("mcp_config.json")).unwrap();
    assert!(text.contains("\"disabled\": true") || text.contains("\"disabled\":true"));
    let _ = fs::remove_dir_all(gemini);
}

#[test]
fn uninstall_refuses_config_plugins() {
    let gemini = crate::paths::scratch_dir("on-n-off-agy-uninstall");
    write_plugin(&gemini.join("config").join("plugins"), "beta", None);
    let err = AntigravityAdapter::at(gemini.clone())
        .uninstall_plugin("beta@config")
        .expect_err("config");
    assert!(err.message.contains("cannot be uninstalled"));
    let _ = fs::remove_dir_all(gemini);
}
