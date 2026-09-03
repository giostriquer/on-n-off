use super::*;
use crate::dto::ErrorKind;
use serde_json::json;

impl ConfigIo {
    fn force_invalid_commit(&self, agent: AgentId, path: &Path) -> Result<(), AdapterError> {
        let original = read_or_empty(path)?;
        self.commit(agent, path, &original, "{not json", |text| {
            serde_json::from_str::<JsonValue>(text).map(|_| ())
        })
    }
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
    let io = ConfigIo::at(root.join("backups"));
    io.patch_json_skill_override(AgentId::Claude, &path, "statusline", false)
        .unwrap();
    let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
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
fn toml_upsert_skill_config_keeps_plugins_and_quotes_windows_paths() {
    let root = crate::paths::scratch_dir("on-n-off-toml");
    let path = root.join("config.toml");
    fs::write(
        &path,
        "[plugins.\"workbench@workshop\"]\nenabled = true\n\n[projects.'E:\\\\dev\\\\on-n-off']\ntrust_level = \"trusted\"\n",
    )
    .unwrap();
    let io = ConfigIo::at(root.join("backups"));
    let skill = r"C:\Users\me\fake-home\.agents\skills\loom-feed\SKILL.md";
    io.patch_toml_skill_enabled(AgentId::Codex, &path, skill, false)
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("[plugins.\"workbench@workshop\"]"), "{text}");
    assert!(text.contains("trust_level"), "{text}");
    assert!(text.contains("[[skills.config]]"), "{text}");
    assert!(text.contains("enabled = false"), "{text}");
    assert!(text.contains("loom-feed"), "{text}");
    io.patch_toml_skill_enabled(AgentId::Codex, &path, skill, true)
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("[[skills.config]]").count(), 1);
    assert!(text.contains("enabled = true"));
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
    let io = ConfigIo::at(root.join("backups"));
    io.patch_json_mcp_enabled(AgentId::Claude, &path, "github", false)
        .unwrap();
    let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["numStartups"], 3);
    assert_eq!(
        value["mcpServers"]["docs"]["url"],
        "https://docs.example/mcp"
    );
    assert_eq!(value["mcpServers"]["github"]["disabled"], true);
    assert_eq!(value["mcpServers"]["github"]["command"], "npx");
    assert_eq!(value["disabledMcpServers"], json!(["github"]));
    io.patch_json_mcp_enabled(AgentId::Claude, &path, "github", true)
        .unwrap();
    let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["github"]["disabled"], false);
    assert_eq!(value["disabledMcpServers"], json!([]));
    let err = io
        .patch_json_mcp_enabled(AgentId::Claude, &path, "missing", false)
        .expect_err("missing");
    assert!(err.message.contains("mcp server not found"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn antigravity_mcp_toggle_sets_disabled_only() {
    let root = crate::paths::scratch_dir("on-n-off-agy-mcp");
    let path = root.join("mcp_config.json");
    fs::write(
        &path,
        r#"{ "mcpServers": { "github": { "command": "npx", "args": ["-y", "gh"] } } }"#,
    )
    .unwrap();
    let io = ConfigIo::at(root.join("backups"));
    io.patch_antigravity_mcp_enabled(AgentId::Antigravity, &path, "github", false)
        .unwrap();
    let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["github"]["disabled"], true);
    assert!(value.get("disabledMcpServers").is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn toml_mcp_enable_only_flips_enabled() {
    let root = crate::paths::scratch_dir("on-n-off-toml-mcp");
    let path = root.join("config.toml");
    fs::write(
        &path,
        "[plugins.\"workbench@workshop\"]\nenabled = true\n\n[mcp_servers.github]\ncommand = \"npx\"\nargs = [\"-y\", \"@modelcontextprotocol/server-github\"]\nextra = 1\n\n[mcp_servers.docs]\nurl = \"https://docs.example/mcp\"\nenabled = false\n",
    )
    .unwrap();
    let io = ConfigIo::at(root.join("backups"));
    io.patch_toml_mcp_enabled(AgentId::Codex, &path, "github", false)
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("[plugins.\"workbench@workshop\"]"), "{text}");
    assert!(text.contains("extra = 1"), "{text}");
    assert!(text.contains("[mcp_servers.github]"), "{text}");
    assert!(text.contains("[mcp_servers.docs]"), "{text}");
    assert!(text.contains("enabled = false"), "{text}");
    io.patch_toml_mcp_enabled(AgentId::Codex, &path, "docs", true)
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("url = \"https://docs.example/mcp\""),
        "{text}"
    );
    assert_eq!(text.matches("enabled = true").count(), 2, "{text}");
    let err = io
        .patch_toml_mcp_enabled(AgentId::Codex, &path, "missing", false)
        .expect_err("missing");
    assert!(err.message.contains("mcp server not found"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn toml_plugin_enable_only_flips_enabled() {
    let root = crate::paths::scratch_dir("on-n-off-toml-plugin");
    let path = root.join("config.toml");
    fs::write(
        &path,
        "[plugins.\"workbench@workshop\"]\nenabled = true\nextra = 1\n\n[plugins.\"toolkit@workshop\"]\nenabled = false\n",
    )
    .unwrap();
    let io = ConfigIo::at(root.join("backups"));
    io.patch_toml_plugin_enabled(AgentId::Codex, &path, "workbench@workshop", false)
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("[plugins.\"workbench@workshop\"]"));
    assert!(text.contains("[plugins.\"toolkit@workshop\"]"));
    assert!(text.contains("extra = 1"));
    assert!(text.contains("enabled = false"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unreadable_write_restores_backup() {
    let root = crate::paths::scratch_dir("on-n-off-restore");
    let path = root.join("settings.json");
    fs::write(&path, "{\"keep\":true}").unwrap();
    let io = ConfigIo::at(root.join("backups"));
    let err = io
        .force_invalid_commit(AgentId::Claude, &path)
        .expect_err("invalid commit");
    assert_eq!(err.kind, ErrorKind::Write);
    let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value, json!({"keep": true}));
    let _ = fs::remove_dir_all(root);
}
