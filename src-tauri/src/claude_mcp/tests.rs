use std::fs;

use super::*;
use crate::paths::scratch_dir;

fn plugin(root: &Path, name: &str) -> PluginSource {
    PluginSource {
        id: format!("{name}@acme"),
        name: name.to_string(),
        root: root.to_path_buf(),
    }
}

fn write(path: &Path, body: Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body.to_string()).unwrap();
}

fn manifest(root: &Path, servers: Value) {
    write(
        &root.join(".claude-plugin").join("plugin.json"),
        serde_json::json!({ "name": "tools", "mcpServers": servers }),
    );
}

fn ids(servers: &[McpServerDto]) -> Vec<&str> {
    servers.iter().map(|server| server.id.as_str()).collect()
}

fn sources(servers: &[McpServerDto]) -> Vec<(&str, &str)> {
    servers
        .iter()
        .map(|server| (server.name.as_str(), server.source.as_str()))
        .collect()
}

#[test]
fn reads_a_bare_or_wrapped_mcp_json_at_the_plugin_root() {
    let root = scratch_dir("claude-mcp-plugin-files");
    let bare = root.join("bare");
    let wrapped = root.join("wrapped");
    write(
        &bare.join(".mcp.json"),
        serde_json::json!({ "tracker": { "type": "http", "url": "https://tracker.example/mcp" } }),
    );
    write(
        &wrapped.join(".mcp.json"),
        serde_json::json!({ "mcpServers": { "search": { "command": "node", "args": ["s.js"] } } }),
    );

    let servers = plugin_servers(&[plugin(&bare, "bare"), plugin(&wrapped, "wrapped")]);

    assert_eq!(
        ids(&servers),
        ["plugin:bare:tracker", "plugin:wrapped:search"]
    );
    assert_eq!(servers[0].system, "http");
    assert_eq!(servers[1].source, "node s.js");
    let plugin_ids: Vec<_> = servers
        .iter()
        .map(|server| server.plugin_id.as_deref())
        .collect();
    assert_eq!(plugin_ids, [Some("bare@acme"), Some("wrapped@acme")]);
    assert!(servers.iter().all(|server| server.projects.is_empty()));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reads_a_manifest_that_declares_one_inline_map() {
    let root = scratch_dir("claude-mcp-manifest-inline");
    manifest(
        &root,
        serde_json::json!({ "inline": { "command": "${CLAUDE_PLUGIN_ROOT}/bin/inline" } }),
    );

    let servers = plugin_servers(&[plugin(&root, "tools")]);

    assert_eq!(ids(&servers), ["plugin:tools:inline"]);
    assert_eq!(servers[0].source, "${CLAUDE_PLUGIN_ROOT}/bin/inline");
    assert!(!servers[0].togglable);
    assert_eq!(servers[0].origin, ORIGIN_PLUGIN);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reads_a_manifest_that_names_one_file() {
    let root = scratch_dir("claude-mcp-manifest-path");
    manifest(&root, serde_json::json!("./servers/extra.json"));
    write(
        &root.join("servers").join("extra.json"),
        serde_json::json!({ "mcpServers": { "extra": { "command": "extra" } } }),
    );

    let servers = plugin_servers(&[plugin(&root, "tools")]);

    assert_eq!(ids(&servers), ["plugin:tools:extra"]);
    let _ = fs::remove_dir_all(root);
}

/// Claude Code 2.1.281 merges a plugin's servers into one map: `.mcp.json` first, then each
/// manifest entry in order, a later entry replacing an earlier one of the same name.
#[test]
fn a_later_source_replaces_a_server_of_the_same_name() {
    let root = scratch_dir("claude-mcp-merge-order");
    write(
        &root.join(".mcp.json"),
        serde_json::json!({
            "search": { "command": "from-mcp-json" },
            "rooted": { "command": "rooted" }
        }),
    );
    write(
        &root.join("servers").join("first.json"),
        serde_json::json!({ "search": { "command": "from-first-file" }, "extra": { "command": "first-extra" } }),
    );
    manifest(
        &root,
        serde_json::json!([
            "./servers/first.json",
            { "extra": { "command": "from-inline" } },
            "../outside.json"
        ]),
    );
    write(
        &root.parent().unwrap().join("outside.json"),
        serde_json::json!({ "escaped": { "command": "nope" } }),
    );

    let servers = plugin_servers(&[plugin(&root, "tools")]);

    let mut seen = sources(&servers);
    seen.sort_unstable();
    assert_eq!(
        seen,
        [
            ("extra", "from-inline"),
            ("rooted", "rooted"),
            ("search", "from-first-file"),
        ]
    );
    let _ = fs::remove_dir_all(root);
}

/// An MCP bundle (`.mcpb`, and the older `.dxt`) is an archive Claude Code installs; on-n-off
/// does not open it, whatever it holds.
#[test]
fn bundle_entries_are_skipped() {
    let root = scratch_dir("claude-mcp-bundles");
    manifest(
        &root,
        serde_json::json!(["./server.mcpb", "./legacy.DXT", "./servers/real.json"]),
    );
    for name in ["server.mcpb", "legacy.DXT"] {
        write(
            &root.join(name),
            serde_json::json!({ "bundled": { "command": "bundled" } }),
        );
    }
    write(
        &root.join("servers").join("real.json"),
        serde_json::json!({ "real": { "command": "real" } }),
    );

    let servers = plugin_servers(&[plugin(&root, "tools")]);

    assert_eq!(ids(&servers), ["plugin:tools:real"]);
    let _ = fs::remove_dir_all(root);
}

/// Names only collide within one plugin: two plugins that each bring a `search` are two rows.
#[test]
fn two_plugins_may_bring_servers_of_the_same_name() {
    let root = scratch_dir("claude-mcp-two-plugins");
    for name in ["alpha", "beta"] {
        write(
            &root.join(name).join(".mcp.json"),
            serde_json::json!({ "search": { "command": name } }),
        );
    }

    let servers = plugin_servers(&[
        plugin(&root.join("alpha"), "alpha"),
        plugin(&root.join("beta"), "beta"),
    ]);

    assert_eq!(ids(&servers), ["plugin:alpha:search", "plugin:beta:search"]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_plugin_without_servers_or_with_a_broken_file_brings_none() {
    let root = scratch_dir("claude-mcp-plugin-none");
    let broken = root.join("broken");
    write(
        &broken.join(".claude-plugin").join("plugin.json"),
        serde_json::json!({ "mcpServers": 7 }),
    );
    fs::write(broken.join(".mcp.json"), "{ torn").unwrap();

    let servers = plugin_servers(&[
        plugin(&root.join("empty"), "empty"),
        plugin(&broken, "broken"),
    ]);

    assert!(servers.is_empty(), "{servers:?}");
    let _ = fs::remove_dir_all(root);
}

/// One row per definition: the same server in several projects is one row, a different
/// definition under the same name is its own row, and a row is on when any project has it on.
#[test]
fn groups_local_servers_by_definition() {
    let config = serde_json::json!({
        "projects": {
            "/Users/me/acme/webapp": {
                "mcpServers": { "docs": { "type": "http", "url": "https://docs.example/mcp" } },
                "disabledMcpServers": ["docs"]
            },
            "/Users/me/acme/api": {
                "mcpServers": {
                    "docs": { "type": "http", "url": "https://docs.example/mcp" },
                    "db": { "command": "db", "args": ["--ro"] }
                }
            },
            "C:\\Users\\me\\acme\\tools": {
                "mcpServers": { "db": { "command": "db" } }
            }
        }
    });

    let servers = local_servers(&config);

    let rows: Vec<_> = servers
        .iter()
        .map(|server| {
            (
                server.id.as_str(),
                server.source.as_str(),
                server.projects.clone(),
                server.enabled,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                "local:db",
                "db",
                vec!["C:\\Users\\me\\acme\\tools".to_string()],
                true
            ),
            (
                "local:db#2",
                "db --ro",
                vec!["/Users/me/acme/api".to_string()],
                true
            ),
            (
                "local:docs",
                "https://docs.example/mcp",
                vec![
                    "/Users/me/acme/api".to_string(),
                    "/Users/me/acme/webapp".to_string()
                ],
                true
            ),
        ]
    );
    assert!(servers.iter().all(|server| !server.togglable
        && server.origin == ORIGIN_LOCAL
        && server.plugin_id.is_none()));
}

/// Claude Code reads `disabledMcpServers` from the project's own entry (2.1.281): a server the
/// only project keeping it switched off is off here too.
#[test]
fn a_local_server_its_project_disabled_is_off() {
    let config = serde_json::json!({
        "projects": {
            "/Users/me/acme/webapp": {
                "mcpServers": { "scratchpad": { "command": "node", "args": ["pad.js"] } },
                "disabledMcpServers": ["scratchpad"]
            }
        }
    });

    let servers = local_servers(&config);

    assert_eq!(ids(&servers), ["local:scratchpad"]);
    assert!(!servers[0].enabled);
}

#[test]
fn no_projects_means_no_local_servers() {
    assert!(local_servers(&serde_json::json!({ "mcpServers": {} })).is_empty());
    assert!(local_servers(&Value::Null).is_empty());
}

fn server(id: &str, origin: &str) -> McpServerDto {
    McpServerDto {
        id: id.to_string(),
        name: id.rsplit(':').next().unwrap().to_string(),
        system: "stdio".to_string(),
        source: "run".to_string(),
        enabled: true,
        togglable: origin.is_empty(),
        origin: origin.to_string(),
        plugin_id: None,
        projects: Vec::new(),
    }
}

/// Inside a project, what that project's `~/.claude.json` entry says: its local servers, off when
/// its `disabledMcpServers` names them; the plugin servers it switched off by their scoped name;
/// and none of the rows that stand for servers kept for particular projects.
#[test]
fn a_project_view_applies_that_projects_own_disabled_list() {
    let config = serde_json::json!({
        "disabledMcpServers": ["plugin:kit:search"],
        "projects": {
            "E:/dev/app": {
                "mcpServers": {
                    "docs": { "type": "http", "url": "https://docs.example/mcp" },
                    "db": { "command": "db" }
                },
                "disabledMcpServers": ["docs", "plugin:kit:tracker"]
            },
            "/Users/me/acme/other": {
                "disabledMcpServers": ["db", "plugin:kit:search"]
            }
        }
    });
    let mut servers = vec![
        server("github", ""),
        server("plugin:kit:tracker", ORIGIN_PLUGIN),
        server("plugin:kit:search", ORIGIN_PLUGIN),
        server("local:docs", ORIGIN_LOCAL),
    ];

    let own = project_servers(&mut servers, &config, Path::new("E:\\dev\\app"));

    let view: Vec<_> = servers
        .iter()
        .map(|server| (server.id.as_str(), server.enabled))
        .collect();
    assert_eq!(
        view,
        [
            ("github", true),
            ("plugin:kit:tracker", false),
            ("plugin:kit:search", true),
        ]
    );
    let own: Vec<_> = own
        .iter()
        .map(|server| (server.id.as_str(), server.enabled))
        .collect();
    assert_eq!(own, [("docs", false), ("db", true)]);
}

#[test]
fn a_project_with_no_entry_keeps_every_plugin_server_on() {
    let mut servers = vec![server("plugin:kit:tracker", ORIGIN_PLUGIN)];

    let own = project_servers(
        &mut servers,
        &Value::Null,
        Path::new("/Users/me/acme/webapp"),
    );

    assert!(own.is_empty());
    assert!(servers[0].enabled);
}
