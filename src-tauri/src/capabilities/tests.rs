use std::path::{Path, PathBuf};

use super::*;
use crate::adapter::scratch_adapter as adapter_at;
use crate::dto::{AgentId, AgentTabDto};

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path.clone());
            }
            found.push(path);
        }
    }
    found.sort();
    found
}

fn health(info: &AgentInfo) -> serde_json::Value {
    let mut document = serde_json::to_value(info).unwrap();
    let fields = document.as_object_mut().unwrap();
    for key in ["id", "displayName", "capabilities", "itemRoots"] {
        assert!(fields.remove(key).is_some(), "{key}");
    }
    document
}

#[test]
fn the_agents_the_ui_builds_from_are_the_ones_each_adapter_describes() {
    let home = crate::paths::scratch_dir("on-n-off-capabilities-fixture");
    let written: serde_json::Map<String, serde_json::Value> = AgentId::ALL
        .into_iter()
        .map(|id| {
            let adapter = adapter_at(id, &home);
            let found = AgentInfo::describe(&*adapter, true, Some(&home));
            let missing = AgentInfo::describe(&*adapter, false, Some(&home));
            assert_eq!((found.id, missing.id), (id, id));
            assert_eq!(found.display_name, id.display_name());
            assert_eq!(found.capabilities, missing.capabilities, "{id:?}");
            assert_eq!(
                found.item_roots,
                ItemPlaces::of(&*adapter, Some(&home)),
                "{id:?}"
            );
            let mut row = serde_json::json!({
                "capabilities": found.capabilities,
                "cliFound": health(&found),
                "cliMissing": health(&missing),
            });
            if id != AgentId::Muse {
                row["itemRoots"] = serde_json::to_value(&found.item_roots).unwrap();
            }
            (id.key().to_string(), row)
        })
        .collect();
    let written = serde_json::Value::Object(written);
    let _ = std::fs::remove_dir_all(home);
    assert_eq!(
        written,
        serde_json::from_str::<serde_json::Value>(include_str!("fixtures/agents.json")).unwrap(),
        "src-tauri/src/capabilities/fixtures/agents.json must read:\n{}",
        serde_json::to_string_pretty(&written).unwrap()
    );
}

fn samples(kind: SourceKind, home: &Path) -> Vec<String> {
    match kind {
        SourceKind::GitMarketplace => vec![
            "https://github.com/acme/webapp.git".to_string(),
            "acme/webapp".to_string(),
        ],
        SourceKind::Folder => vec![home.display().to_string()],
        SourceKind::MarketplacePlugin => vec!["acme@webapp".to_string()],
        SourceKind::MarketplaceSkill => vec!["npx skills add acme/webapp".to_string()],
    }
}

fn refusals(capabilities: &Capabilities) -> Vec<String> {
    let changes = [
        &capabilities.plugin_switch,
        &capabilities.skill_switch,
        &capabilities.mcp_switch,
        &capabilities.install,
        &capabilities.uninstall,
        &capabilities.update,
    ];
    changes
        .into_iter()
        .filter_map(|change| match change {
            Change::Refused { reason, .. } => Some(reason.clone()),
            Change::Cli { .. } | Change::File { .. } => None,
        })
        .chain(capabilities.refused_sources.values().cloned())
        .collect()
}

fn assert_agrees(
    id: AgentId,
    what: &str,
    change: &Change,
    answer: Result<AgentTabDto, AdapterError>,
    refusals: &[String],
) {
    let message = answer.err().map(|error| error.message);
    match change {
        Change::Refused { reason, .. } => {
            assert_eq!(message.as_deref(), Some(reason.as_str()), "{id:?} {what}");
        }
        Change::Cli { .. } | Change::File { .. } => {
            let message = message.unwrap_or_default();
            assert!(
                !message.contains("not implemented") && !refusals.contains(&message),
                "{id:?} declares {what} but refuses it: {message}"
            );
        }
    }
}

#[test]
fn every_change_an_adapter_declares_is_one_it_makes_and_every_refusal_is_declared() {
    for id in AgentId::ALL {
        let home = crate::paths::scratch_dir("on-n-off-capabilities-agree");
        let adapter = adapter_at(id, &home);
        let declared = adapter.capabilities();
        let refusals = refusals(&declared);

        let attempts = [
            (
                "plugin switch",
                &declared.plugin_switch,
                adapter.set_plugin_enabled("acme@webapp", false),
            ),
            (
                "skill switch",
                &declared.skill_switch,
                adapter.set_skill_enabled("acme", false),
            ),
            (
                "mcp switch",
                &declared.mcp_switch,
                adapter.set_mcp_enabled("acme", false),
            ),
            (
                "uninstall",
                &declared.uninstall,
                adapter.uninstall_plugin("acme@webapp"),
            ),
            (
                "update",
                &declared.update,
                adapter.update_plugin("acme@webapp"),
            ),
        ];
        for (what, change, answer) in attempts {
            assert_agrees(id, what, change, answer, &refusals);
        }

        for (kind, source) in SourceKind::ALL
            .into_iter()
            .flat_map(|kind| samples(kind, &home).into_iter().map(move |one| (kind, one)))
        {
            let what = format!("install from {kind:?}");
            let refused = declared.refused_sources.get(&kind);
            match (declared.install.ready(true), refused) {
                (true, Some(reason)) => assert_agrees(
                    id,
                    &what,
                    &Change::refused("", reason),
                    adapter.install_plugin(&source),
                    &[],
                ),
                (true, None) if kind == SourceKind::MarketplaceSkill => {
                    assert!(declared.admit(&source).is_ok(), "{id:?} {what}");
                }
                _ => assert_agrees(
                    id,
                    &what,
                    &declared.install,
                    adapter.install_plugin(&source),
                    &refusals,
                ),
            }
        }
        let _ = std::fs::remove_dir_all(home);
    }
}

fn write(path: PathBuf, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn seed(id: AgentId, home: &Path) {
    let plugin = "acme@webapp";
    match id {
        AgentId::Claude => {
            write(
                home.join(".claude").join("settings.json"),
                &format!(r#"{{"enabledPlugins":{{"{plugin}":true}}}}"#),
            );
            write(
                home.join(".claude").join(".claude.json"),
                r#"{"mcpServers":{"acme":{"command":"acme"}}}"#,
            );
        }
        AgentId::Codex => write(
            home.join(".codex").join("config.toml"),
            &format!(
                "[plugins.\"{plugin}\"]\nenabled = true\n\n[mcp_servers.acme]\ncommand = \"acme\"\n"
            ),
        ),
        AgentId::Antigravity => {
            let gemini = home.join(".gemini");
            write(
                gemini.join("antigravity-cli").join("config.json"),
                r#"{"plugins":[{"name":"acme","enabled":true}]}"#,
            );
            write(
                gemini.join("config").join("mcp_config.json"),
                r#"{"mcpServers":{"acme":{"command":"acme"}}}"#,
            );
            write(
                gemini
                    .join("antigravity-cli")
                    .join("plugins")
                    .join("acme")
                    .join("plugin.json"),
                r#"{"name":"acme"}"#,
            );
        }
        AgentId::Cursor => write(
            home.join(".cursor").join("mcp.json"),
            r#"{"mcpServers":{"acme":{"command":"acme"}}}"#,
        ),
        AgentId::Muse => {
            write(
                crate::paths::muse_config_for(home).join("settings.json"),
                r#"{"mcpServers":{"acme":{"command":"acme"}}}"#,
            );
            let package = crate::paths::muse_data_for(home)
                .join("plugins")
                .join("cache")
                .join("local")
                .join("acme")
                .join("package");
            write(package.join("plugin.json"), r#"{"name":"acme"}"#);
            write(
                crate::paths::muse_data_for(home)
                    .join("plugins")
                    .join("installed.json"),
                &format!(
                    r#"{{"version":1,"plugins":{{"acme":{{"id":"acme","version":"1.0.0","enabled":true,"source":{{"path":"/Users/me/acme"}},"cache_path":{}}}}}}}"#,
                    serde_json::to_string(&package.to_string_lossy()).unwrap()
                ),
            );
        }
    }
}

fn contents(dir: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    files_under(dir)
        .into_iter()
        .map(|path| {
            let bytes = path.is_file().then(|| std::fs::read(&path).unwrap());
            (path, bytes)
        })
        .collect()
}

#[test]
fn a_refused_change_leaves_the_home_as_it_was() {
    for id in AgentId::ALL {
        let home = crate::paths::scratch_dir("on-n-off-capabilities-untouched");
        seed(id, &home);
        let adapter = adapter_at(id, &home);
        let declared = adapter.capabilities();
        let before = contents(&home);
        assert!(before.iter().any(|(_, bytes)| bytes.is_some()), "{id:?}");
        let refused = |change: &Change| matches!(change, Change::Refused { .. });
        if refused(&declared.plugin_switch) {
            let _ = adapter.set_plugin_enabled("acme@webapp", true);
        }
        if refused(&declared.skill_switch) {
            let _ = adapter.set_skill_enabled("acme", true);
        }
        if refused(&declared.mcp_switch) {
            let _ = adapter.set_mcp_enabled("acme", true);
        }
        if refused(&declared.uninstall) {
            let _ = adapter.uninstall_plugin("acme@webapp");
        }
        if refused(&declared.update) {
            let _ = adapter.update_plugin("acme@webapp");
        }
        let refused_kinds: Vec<SourceKind> = if refused(&declared.install) {
            SourceKind::ALL.to_vec()
        } else {
            declared.refused_sources.keys().copied().collect()
        };
        for kind in refused_kinds {
            for source in samples(kind, &home) {
                let _ = adapter.install_plugin(&source);
            }
        }
        let after = contents(&home);
        let changed: Vec<_> = after
            .iter()
            .filter(|entry| !before.contains(entry))
            .chain(before.iter().filter(|entry| !after.contains(entry)))
            .map(|(path, _)| path.strip_prefix(&home).unwrap_or(path).to_path_buf())
            .collect();
        assert!(changed.is_empty(), "{id:?} changed {changed:?}");
        let _ = std::fs::remove_dir_all(home);
    }
}

#[test]
fn a_cli_change_waits_for_the_cli_a_file_change_does_not_and_a_refused_one_never_runs() {
    let cli = Change::cli("acme cli");
    let file = Change::file("acme file");
    let refused = Change::refused("read-only", "acme refuses");
    assert_eq!((cli.ready(true), cli.ready(false)), (true, false));
    assert_eq!((file.ready(true), file.ready(false)), (true, true));
    assert_eq!((refused.ready(true), refused.ready(false)), (false, false));
    assert_eq!(refused.refusal().message, "acme refuses");
    assert!(cli.refusal().message.contains("not implemented"));
}

#[test]
fn an_install_source_is_parsed_once_and_refused_only_by_its_declared_kind() {
    let declared = Capabilities {
        refused_sources: BTreeMap::from([(SourceKind::GitMarketplace, "no git here".to_string())]),
        ..Capabilities::default()
    };
    assert_eq!(
        declared.admit("acme/webapp").unwrap_err().message,
        "no git here"
    );
    assert_eq!(
        declared
            .admit("https://github.com/acme/webapp.git")
            .unwrap_err()
            .message,
        "no git here"
    );
    assert_eq!(
        declared.admit("acme@webapp").unwrap(),
        InstallSource::Plugin("acme@webapp".into())
    );
    assert!(declared.admit("git@github.com:acme/webapp.git").is_err());
    assert!(declared.accepts(SourceKind::Folder));
    assert!(!declared.accepts(SourceKind::GitMarketplace));
}

#[test]
fn an_uninstall_reaches_only_the_sources_it_declares() {
    let plugin = |source: &str| PluginDto {
        id: format!("acme@{source}"),
        name: "acme".into(),
        source: source.into(),
        version: String::new(),
        upstream: String::new(),
        out_of_sync: false,
        enabled: true,
        togglable: true,
        skills: Vec::new(),
    };
    let everywhere = Capabilities::default();
    assert!(everywhere.uninstalls(&plugin("config")).is_ok());
    let only_cli = Capabilities {
        uninstall_from: Some(vec!["cli".into()]),
        ..Capabilities::default()
    };
    assert!(only_cli.uninstalls(&plugin("cli")).is_ok());
    assert_eq!(
        only_cli.uninstalls(&plugin("config")).unwrap_err().message,
        "plugin cannot be uninstalled here: acme@config"
    );
}
