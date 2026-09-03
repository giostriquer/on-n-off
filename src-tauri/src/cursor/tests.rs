use super::*;

fn write_skill(dir: &Path, name: &str, description: &str) {
    fs::create_dir_all(dir.join(name)).unwrap();
    fs::write(
        dir.join(name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )
    .unwrap();
}

fn write_plugin(dir: &Path, folder: &str, name: &str, skill: Option<&str>, cursor_manifest: bool) {
    let plugin = dir.join(folder);
    fs::create_dir_all(&plugin).unwrap();
    let manifest = if cursor_manifest {
        let nested = plugin.join(".cursor-plugin");
        fs::create_dir_all(&nested).unwrap();
        nested.join("plugin.json")
    } else {
        plugin.join("plugin.json")
    };
    fs::write(
        &manifest,
        format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
    )
    .unwrap();
    if let Some(skill_name) = skill {
        write_skill(&plugin.join("skills"), skill_name, "From plugin");
    }
}

#[test]
fn empty_home_lists_nothing() {
    let root = crate::paths::scratch_dir("on-n-off-cursor-empty");
    fs::create_dir_all(&root).unwrap();
    let tab = CursorAdapter::at(root.clone()).list_tab().expect("list");
    assert!(tab.plugins.is_empty());
    assert!(tab.user_skills.is_empty());
    assert!(tab.mcp_servers.is_empty());
    let info = CursorAdapter::at(root.clone()).info();
    assert_eq!(info.id, AgentId::Cursor);
    assert_eq!(info.display_name, "Cursor");
    assert!(!info.plugin_toggle);
    assert!(!info.install_git);
    assert!(!info.install_folder);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn lists_local_and_cache_plugins_user_skills_and_mcp() {
    let root = crate::paths::scratch_dir("on-n-off-cursor-list");
    write_plugin(
        &root.join("plugins").join("local"),
        "reviewer",
        "reviewer",
        Some("pr-review"),
        true,
    );
    write_plugin(
        &root.join("plugins").join("cache").join("workshop"),
        "workbench",
        "workbench",
        None,
        false,
    );
    write_skill(&root.join("skills"), "notes", "User skill");
    write_skill(&root.join("skills-cursor"), "builtin", "Managed builtin");
    fs::write(
        root.join("mcp.json"),
        r#"{"mcpServers":{"docs":{"url":"https://docs.example/mcp"},"github":{"command":"npx","args":["-y","gh"],"disabled":true}}}"#,
    )
    .unwrap();

    let tab = CursorAdapter::at(root.clone()).list_tab().expect("list");
    let local = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "reviewer@local")
        .expect("local plugin");
    assert_eq!(local.source, "local");
    assert!(local.enabled);
    assert!(!local.togglable);
    assert_eq!(local.skills.len(), 1);
    assert_eq!(local.skills[0].name, "pr-review");
    assert!(!local.skills[0].togglable);
    let cached = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .expect("cached plugin");
    assert_eq!(cached.source, "workshop");
    assert!(!cached.togglable);
    assert_eq!(tab.user_skills.len(), 1);
    assert_eq!(tab.user_skills[0].name, "notes");
    assert!(!tab.user_skills[0].togglable);
    assert!(!tab.user_skills.iter().any(|skill| skill.name == "builtin"));
    assert_eq!(tab.mcp_servers.len(), 2);
    // Cursor keeps on/off in its own state and never reads a `disabled` key, so every
    // configured server is listed as on and none can be switched from here.
    for server in &tab.mcp_servers {
        assert!(server.enabled, "{} should read as configured/on", server.id);
        assert!(!server.togglable, "{} must not be togglable", server.id);
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn versioned_cache_lists_the_newest_complete_version_once() {
    // Cursor installs marketplace plugins as cache/<marketplace>/<plugin>/<commit>/, keeping
    // older checkouts around and marking finished downloads with `.cache-complete`.
    let root = crate::paths::scratch_dir("on-n-off-cursor-versioned");
    let plugin = root
        .join("plugins")
        .join("cache")
        .join("workshop")
        .join("workbench");
    for (sha, version, skill, complete) in [
        ("1111111", "0.20.0", "old-skill", true),
        ("2222222", "0.22.1", "review", true),
        ("3333333", "0.23.0", "half-downloaded", false),
    ] {
        let checkout = plugin.join(sha);
        let manifest_dir = checkout.join(".cursor-plugin");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(
            manifest_dir.join("plugin.json"),
            format!(r#"{{"name":"workbench","version":"{version}"}}"#),
        )
        .unwrap();
        write_skill(&checkout.join("skills"), skill, "From plugin");
        if complete {
            fs::write(checkout.join(".cache-complete"), "").unwrap();
        }
    }
    let tab = CursorAdapter::at(root.clone()).list_tab().expect("list");
    assert_eq!(tab.plugins.len(), 1, "{:?}", tab.plugins);
    let workbench = &tab.plugins[0];
    assert_eq!(workbench.id, "workbench@workshop");
    assert_eq!(workbench.version, "0.22.1");
    assert_eq!(workbench.skills.len(), 1);
    assert_eq!(workbench.skills[0].name, "review");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn mcp_toggle_is_refused_and_leaves_the_file_alone() {
    let root = crate::paths::scratch_dir("on-n-off-cursor-mcp-toggle");
    let original = r#"{"mcpServers":{"github":{"command":"npx","args":["-y","gh"]}}}"#;
    fs::write(root.join("mcp.json"), original).unwrap();
    let adapter = CursorAdapter::at(root.clone());
    let err = adapter
        .set_mcp_enabled("github", false)
        .expect_err("Cursor MCP servers are read-only here");
    assert!(err.message.contains("Cursor"), "{}", err.message);
    assert!(err.message.contains("agent mcp"), "{}", err.message);
    assert_eq!(fs::read_to_string(root.join("mcp.json")).unwrap(), original);
    assert!(
        !root.join("_backups").exists(),
        "no backup for a refused write"
    );
    let _ = fs::remove_dir_all(root);
}
