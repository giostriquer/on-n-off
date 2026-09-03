use super::*;

#[test]
fn claude_seed_lists_stable_plugin_ids_and_unique_skill_ids() {
    let adapter = FakeAdapter::claude();
    let tab = adapter.list_tab().expect("list");

    let ids: Vec<&str> = tab
        .plugins
        .iter()
        .map(|plugin| plugin.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "superpowers@claude-plugins-official",
            "toolkit@workshop",
            "workbench@workshop"
        ]
    );
    assert!(!tab.plugins[0].enabled);
    assert!(tab.plugins[2].enabled);

    let workbench = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    let brainstorming = &workbench.skills[0];
    assert_eq!(brainstorming.id, "workbench@workshop:brainstorming");
    assert!(!brainstorming.togglable);
    assert_eq!(tab.user_skills[0].id, "statusline");
    assert!(tab.user_skills[0].togglable);
}

#[test]
fn toggling_a_locked_plugin_skill_fails_without_changing_state() {
    let adapter = FakeAdapter::claude();
    let err = adapter
        .set_skill_enabled("workbench@workshop:brainstorming", false)
        .expect_err("locked skill");
    assert!(err.message.contains("not togglable"));
    let tab = adapter.list_tab().expect("list");
    let workbench = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert!(workbench.skills[0].enabled);
}

#[test]
fn toggling_a_user_skill_returns_the_full_tab() {
    let adapter = FakeAdapter::claude();
    let tab = adapter
        .set_skill_enabled("statusline", false)
        .expect("toggle");
    assert!(!tab.user_skills[0].enabled);
    let workbench = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert!(workbench.enabled);
    assert!(workbench.skills[0].enabled);
}

#[test]
fn toggling_an_mcp_returns_the_full_tab() {
    let adapter = FakeAdapter::claude();
    let tab = adapter.set_mcp_enabled("github", false).expect("toggle");
    let github = tab
        .mcp_servers
        .iter()
        .find(|server| server.id == "github")
        .unwrap();
    assert!(!github.enabled);
    assert!(github.togglable);
    let err = adapter
        .set_mcp_enabled("missing", true)
        .expect_err("missing");
    assert!(err.message.contains("mcp server not found"));
}

#[test]
fn install_success_adds_plugin_and_failure_does_not() {
    let adapter = FakeAdapter::claude();
    let tab = adapter.install_plugin("acme/tools").expect("install");
    assert!(tab.plugins.iter().any(|plugin| plugin.id == "tools@local"));
    let tab = adapter
        .install_plugin("npx skills add vercel-labs/agent-skills --skill web-design")
        .expect("npx");
    assert!(tab.user_skills.iter().any(|row| row.name == "web-design"));
    let err = adapter.install_plugin("").expect_err("empty");
    assert!(err.message.contains("HTTPS"));
    let tab = adapter.list_tab().expect("list");
    assert_eq!(
        tab.plugins
            .iter()
            .filter(|plugin| plugin.id == "tools@local")
            .count(),
        1
    );
}

#[test]
fn uninstall_removes_plugin() {
    let adapter = FakeAdapter::claude();
    let tab = adapter
        .uninstall_plugin("workbench@workshop")
        .expect("uninstall");
    assert!(tab
        .plugins
        .iter()
        .all(|plugin| plugin.id != "workbench@workshop"));
}

#[test]
fn update_clears_out_of_sync() {
    let adapter = FakeAdapter::claude();
    let tab = adapter.update_plugin("workbench@workshop").expect("update");
    let plugin = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert!(!plugin.out_of_sync);
    let err = adapter
        .update_plugin("missing@workshop")
        .expect_err("missing");
    assert!(err.message.contains("plugin not found"));
}
