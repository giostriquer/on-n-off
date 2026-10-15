use super::*;
use crate::cli_stub::CliStub;

fn write_skill(dir: &Path, name: &str, description: &str) {
    fs::create_dir_all(dir.join(name)).unwrap();
    fs::write(
        dir.join(name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )
    .unwrap();
}

struct Fixture {
    home: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        Self {
            home: crate::paths::scratch_dir(&format!("on-n-off-muse-{tag}")),
        }
    }

    fn config(&self) -> PathBuf {
        crate::paths::muse_config_for(&self.home)
    }

    fn data(&self) -> PathBuf {
        crate::paths::muse_data_for(&self.home)
    }

    fn write_settings(&self, body: &str) {
        fs::create_dir_all(self.config()).unwrap();
        fs::write(self.config().join("settings.json"), body).unwrap();
    }

    fn install_plugin(
        &self,
        id: &str,
        version: &str,
        enabled: bool,
        skill: Option<&str>,
    ) -> PathBuf {
        let package = self
            .data()
            .join("plugins")
            .join("cache")
            .join("local")
            .join(id)
            .join("abc123")
            .join("package");
        fs::create_dir_all(package.join(".muse-plugin")).unwrap();
        fs::write(
            package.join(".muse-plugin").join("plugin.json"),
            format!(
                r#"{{"schemaVersion":1,"name":"{id}","version":"{version}","capabilities":{{"skills":[{{"id":"acme-review","path":"skills/acme-review/SKILL.md"}}]}}}}"#
            ),
        )
        .unwrap();
        if let Some(name) = skill {
            write_skill(&package.join("skills"), name, "From plugin");
        }
        let plugins_dir = self.data().join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();
        let body = format!(
            r#"{{"version":1,"plugins":{{"{id}":{{"id":"{id}","display_name":"Acme {id}","version":"{version}","description":"d","manifest_family":"muse","enabled":{enabled},"trust":"user-local","source":{{"provenance":"local-path","path":"/srv/acme/{id}"}},"installed_at":"2026-01-01T00:00:00Z","cache_path":{}}}}}}}"#,
            serde_json::to_string(&package.to_string_lossy()).unwrap()
        );
        fs::write(plugins_dir.join("installed.json"), body).unwrap();
        package
    }

    fn adapter(&self) -> MuseAdapter {
        MuseAdapter::at(self.home.clone())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

#[test]
fn empty_home_lists_nothing() {
    let fixture = Fixture::new("empty");
    let tab = fixture.adapter().list_tab().expect("list");
    assert!(tab.plugins.is_empty());
    assert!(tab.user_skills.is_empty());
    assert!(tab.mcp_servers.is_empty());
    assert!(tab.hooks.is_empty());
    let info = fixture.adapter().info();
    assert_eq!(info.id, AgentId::Muse);
    assert_eq!(info.display_name, "Muse");
}

#[test]
fn lists_plugins_skills_and_activation() {
    let fixture = Fixture::new("list");
    fixture.install_plugin("acme-tools", "1.2.3", false, Some("acme-review"));
    write_skill(
        &fixture.config().join("skills"),
        "acme-notes",
        "Config skill",
    );
    write_skill(
        &fixture.home.join(".agents").join("skills"),
        "acme-shared",
        "Shared skill",
    );
    fixture.write_settings(
        r#"{"schema_version":1,"skills":{"activation":{
          "user":{"$CONFIG_DIR/skills/acme-notes/SKILL.md":"off","$HOME/.agents/skills/acme-shared/SKILL.md":"user-invocable-only"},
          "plugin":{"plugin://acme-tools/skills/acme-review/SKILL.md":"off"}}},
          "mcpServers":{"docs":{"url":"https://docs.example/mcp"},"git":{"command":"npx","args":["-y","acme-git"]}}}"#,
    );

    let tab = fixture.adapter().list_tab().expect("list");
    assert_eq!(tab.plugins.len(), 1);
    let plugin = &tab.plugins[0];
    assert_eq!(plugin.id, "acme-tools@local");
    assert_eq!(plugin.name, "acme-tools");
    assert_eq!(plugin.source, "local");
    assert_eq!(plugin.version, "1.2.3");
    assert!(!plugin.enabled);
    assert!(plugin.togglable);
    assert_eq!(plugin.skills.len(), 1);
    let plugin_skill = &plugin.skills[0];
    assert_eq!(plugin_skill.id, "plugin:acme-tools:acme-review");
    assert_eq!(plugin_skill.plugin_id.as_deref(), Some("acme-tools@local"));
    assert!(!plugin_skill.enabled);
    assert!(!plugin_skill.togglable);

    assert_eq!(tab.user_skills.len(), 2);
    let notes = tab
        .user_skills
        .iter()
        .find(|skill| skill.name == "acme-notes")
        .expect("config skill");
    assert_eq!(notes.id, "acme-notes");
    assert!(!notes.enabled);
    assert!(notes.togglable);
    let shared = tab
        .user_skills
        .iter()
        .find(|skill| skill.name == "acme-shared")
        .expect("shared skill");
    assert!(shared.enabled, "user-invocable-only counts as enabled");

    assert_eq!(tab.mcp_servers.len(), 2);
    let docs = tab
        .mcp_servers
        .iter()
        .find(|server| server.id == "docs")
        .expect("docs server");
    assert_eq!(docs.system, "http");
    assert!(docs.enabled);
    assert!(!docs.togglable);
}

#[test]
fn plugin_source_names_its_marketplace_and_upstream_version() {
    let fixture = Fixture::new("marketplace");
    let package = fixture.install_plugin("acme-tools", "1.0.0", true, None);
    let plugins_dir = fixture.data().join("plugins");
    let snapshot = plugins_dir
        .join("marketplaces")
        .join("acme")
        .join("snapshot.json");
    fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
    fs::write(
        &snapshot,
        r#"{"schemaVersion":1,"source":"local-dir","plugins":[{"name":"acme-tools","version":"2.0.0","install":{"transport":"local","source":"x"},"availability":{"status":"available"}}],"diagnostics":[]}"#,
    )
    .unwrap();
    let _ = &package;
    let _ = &snapshot;
    fs::write(
        plugins_dir.join("marketplaces.json"),
        r#"{"version":1,"marketplaces":{"acme":{"name":"acme","source":{"kind":"local-dir","path":"/srv/acme"},"snapshot_path":"plugins/marketplaces/acme/snapshot.json","last_updated_at":"2026-01-01T00:00:00Z"}}}"#,
    )
    .unwrap();

    let tab = fixture.adapter().list_tab().expect("list");
    let plugin = &tab.plugins[0];
    assert_eq!(plugin.id, "acme-tools@acme");
    assert_eq!(plugin.source, "acme");
    assert_eq!(plugin.upstream, "2.0.0");
    assert!(plugin.out_of_sync);
}

#[test]
fn mcp_toggle_is_refused() {
    let fixture = Fixture::new("mcp");
    fixture.write_settings(r#"{"mcpServers":{"docs":{"url":"https://docs.example/mcp"}}}"#);
    let error = fixture
        .adapter()
        .set_mcp_enabled("docs", false)
        .expect_err("refused");
    assert_eq!(error.message, MCP_READ_ONLY);
}

fn stubbed(fixture: &Fixture, append: bool) -> MuseAdapter {
    let cli = CliStub::new("muse")
        .log_args("argv.txt", append)
        .cli(&fixture.home);
    MuseAdapter::at_with_cli(fixture.home.clone(), cli)
}

fn argv(fixture: &Fixture) -> String {
    fs::read_to_string(fixture.home.join("argv.txt")).unwrap_or_default()
}

#[test]
fn plugin_toggle_calls_muse_with_bare_name() {
    let fixture = Fixture::new("toggle");
    fixture.install_plugin("acme-tools", "1.0.0", true, None);
    let adapter = stubbed(&fixture, false);
    adapter
        .set_plugin_enabled("acme-tools@local", false)
        .expect("toggle");
    assert_eq!(argv(&fixture).trim(), "plugins disable acme-tools --json");

    let error = adapter
        .set_plugin_enabled("missing@local", true)
        .expect_err("unknown plugin");
    assert!(error.message.contains("plugin not found"), "{error:?}");
}

#[test]
fn skill_toggles_use_muse_scopes_and_ids() {
    let fixture = Fixture::new("skill-toggle");
    fixture.install_plugin("acme-tools", "1.0.0", true, Some("acme-review"));
    write_skill(&fixture.config().join("skills"), "acme-notes", "Config");

    let adapter = stubbed(&fixture, true);
    adapter
        .set_skill_enabled("acme-notes", false)
        .expect("user toggle");
    adapter
        .set_skill_enabled("plugin:acme-tools:acme-review", true)
        .expect("plugin toggle");
    assert_eq!(
        argv(&fixture).lines().collect::<Vec<_>>(),
        [
            "skills disable acme-notes --scope user --json",
            "skills enable plugin:acme-tools:acme-review --scope plugin --json"
        ]
    );
}

#[test]
fn plugin_mutation_preserves_both_state_files() {
    let fixture = Fixture::new("backup-state");
    fixture.install_plugin("acme-tools", "1.0.0", true, None);
    fixture.write_settings(r#"{"schema_version":1,"skills":{"activation":{}}}"#);
    let originals = [
        ("settings.json", fixture.config().join("settings.json")),
        (
            "installed.json",
            fixture.data().join("plugins").join("installed.json"),
        ),
    ];
    let before: Vec<_> = originals
        .iter()
        .map(|(name, path)| (*name, fs::read(path).unwrap()))
        .collect();
    stubbed(&fixture, false)
        .set_plugin_enabled("acme-tools@local", false)
        .unwrap();
    let backups: Vec<_> = fs::read_dir(fixture.home.join("_backups").join("muse"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    for (name, bytes) in before {
        let matches: Vec<_> = backups
            .iter()
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&format!("{name}."))
            })
            .collect();
        assert_eq!(matches.len(), 1, "one backup of {name}");
        assert_eq!(fs::read(matches[0]).unwrap(), bytes);
    }
    assert_eq!(argv(&fixture).trim(), "plugins disable acme-tools --json");
}

#[test]
fn backup_failure_prevents_cli_mutation() {
    let fixture = Fixture::new("backup-failure");
    fixture.install_plugin("acme-tools", "1.0.0", true, None);
    fixture.write_settings(r#"{"schema_version":1}"#);
    fs::write(fixture.home.join("_backups"), "blocked").unwrap();
    let result = stubbed(&fixture, false).set_plugin_enabled("acme-tools@local", false);
    assert!(result.is_err());
    assert!(argv(&fixture).is_empty(), "back up before invoking Muse");
}

#[test]
fn omitted_plugin_enabled_defaults_to_enabled() {
    let fixture = Fixture::new("default-enabled");
    fixture.install_plugin("acme-tools", "1.0.0", true, Some("acme-review"));
    let installed = fixture.data().join("plugins").join("installed.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&installed).unwrap()).unwrap();
    state["plugins"]["acme-tools"]
        .as_object_mut()
        .unwrap()
        .remove("enabled");
    fs::write(&installed, serde_json::to_vec(&state).unwrap()).unwrap();
    let tab = fixture.adapter().list_tab().unwrap();
    assert!(tab.plugins[0].enabled);
    assert!(tab.plugins[0].skills[0].enabled);
    assert!(tab.plugins[0].skills[0].togglable);
}

#[test]
fn uninstall_and_local_update_use_bare_name() {
    let fixture = Fixture::new("uninstall");
    fixture.install_plugin("acme-tools", "1.0.0", true, None);
    let adapter = stubbed(&fixture, true);
    adapter.update_plugin("acme-tools@local").expect("update");
    adapter
        .uninstall_plugin("acme-tools@local")
        .expect("uninstall");
    assert_eq!(
        argv(&fixture).lines().collect::<Vec<_>>(),
        [
            "plugins update acme-tools --json",
            "plugins remove acme-tools --json"
        ]
    );
}

#[test]
fn install_accepts_folder_and_marketplace_id_but_not_git() {
    let fixture = Fixture::new("install");
    let adapter = stubbed(&fixture, true);
    let folder = fixture.home.join("acme-plugin");
    fs::create_dir_all(&folder).unwrap();
    adapter
        .install_plugin(&folder.to_string_lossy())
        .expect("folder install");
    adapter
        .install_plugin("acme-tools@acme")
        .expect("marketplace install");
    assert_eq!(
        argv(&fixture).lines().collect::<Vec<_>>(),
        [
            format!("plugins install {} --scope user --json", folder.display()),
            "plugins install acme-tools@acme --json".to_string()
        ]
    );

    let error = adapter
        .install_plugin("https://github.com/acme/webapp.git")
        .expect_err("git refused");
    assert!(error.message.contains("marketplace add"), "{error:?}");
}

#[test]
fn info_disables_git_install_only() {
    let fixture = Fixture::new("info");
    let info = fixture.adapter().info();
    assert!(!info.install_git);
}

#[test]
fn malformed_installed_plugins_is_a_parse_error() {
    let fixture = Fixture::new("malformed");
    let plugins_dir = fixture.data().join("plugins");
    fs::create_dir_all(&plugins_dir).unwrap();
    fs::write(plugins_dir.join("installed.json"), "{nope").unwrap();
    let error = fixture.adapter().list_tab().expect_err("parse error");
    assert_eq!(error.kind, crate::dto::ErrorKind::Parse);
}

#[test]
fn disabled_plugin_skills_are_not_live_or_actionable() {
    let fixture = Fixture::new("disabled-skills");
    fixture.install_plugin("acme-tools", "1.0.0", false, Some("acme-review"));
    let adapter = stubbed(&fixture, false);
    let tab = adapter.list_tab().unwrap();
    let skill = &tab.plugins[0].skills[0];
    assert!(
        !skill.enabled,
        "a disabled plugin cannot supply a live skill"
    );
    assert!(
        !skill.togglable,
        "Muse cannot select a disabled plugin's skills"
    );
    assert!(adapter.set_skill_enabled(&skill.id, true).is_err());
    assert!(argv(&fixture).is_empty(), "refuse before calling the CLI");
}

#[test]
fn marketplace_matching_uses_source_before_selecting_same_named_plugin() {
    let fixture = Fixture::new("marketplace-collision");
    fixture.install_plugin("acme-tools", "1.0.0", true, None);
    let plugins = fixture.data().join("plugins");
    fs::write(
        plugins.join("first.json"),
        r#"{"plugins":[{"name":"acme-tools","version":"9.0.0"}]}"#,
    )
    .unwrap();
    fs::write(
        plugins.join("second.json"),
        r#"{"plugins":[{"name":"acme-tools","version":"2.0.0"}]}"#,
    )
    .unwrap();
    fs::write(
        plugins.join("marketplaces.json"),
        r#"{"marketplaces":{
      "aaa":{"source":{"path":"/srv/other"},"snapshot_path":"plugins/first.json"},
      "acme":{"source":{"path":"/srv/acme"},"snapshot_path":"plugins/second.json"}
    }}"#,
    )
    .unwrap();
    let adapter = stubbed(&fixture, true);
    let tab = adapter.list_tab().unwrap();
    let plugin = &tab.plugins[0];
    assert_eq!(plugin.id, "acme-tools@acme");
    assert_eq!(plugin.upstream, "2.0.0");
    assert!(plugin.out_of_sync);
    adapter.update_plugin(&plugin.id).unwrap();
    assert_eq!(
        argv(&fixture).lines().collect::<Vec<_>>(),
        [
            "plugins marketplace update acme --json",
            "plugins install acme-tools@acme --json"
        ]
    );
}

#[test]
fn reenabling_a_plugin_preserves_its_skill_activation() {
    let fixture = Fixture::new("reenabled-skills");
    fixture.write_settings(r#"{"skills":{"activation":{"plugin":{"plugin://acme-tools/skills/acme-review/SKILL.md":"off"}}}}"#);
    fixture.install_plugin("acme-tools", "1.0.0", false, Some("acme-review"));
    let adapter = fixture.adapter();
    let disabled = adapter.list_tab().unwrap();
    assert!(!disabled.plugins[0].skills[0].enabled);
    assert!(!disabled.plugins[0].skills[0].togglable);
    fixture.install_plugin("acme-tools", "1.0.0", true, Some("acme-review"));
    let enabled = adapter.list_tab().unwrap();
    assert!(
        !enabled.plugins[0].skills[0].enabled,
        "keep the explicit off state"
    );
    assert!(enabled.plugins[0].skills[0].togglable);
}
