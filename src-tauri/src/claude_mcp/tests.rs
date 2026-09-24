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

fn ids(servers: &[McpServerDto]) -> Vec<&str> {
    servers.iter().map(|server| server.id.as_str()).collect()
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

    let servers = plugin_servers(&[plugin(&bare, "bare"), plugin(&wrapped, "wrapped")], &[]);

    assert_eq!(
        ids(&servers),
        ["plugin:bare:tracker", "plugin:wrapped:search"]
    );
    assert_eq!(servers[0].system, "http");
    assert_eq!(servers[1].source, "node s.js");
    let _ = fs::remove_dir_all(root);
}

/// The manifest's `mcpServers` may be inline, a path, or a list of either, and adds to
/// `.mcp.json`; a name defined in both is the manifest's.
#[test]
fn reads_every_form_of_the_manifest_key_and_lets_it_win_a_shared_name() {
    let root = scratch_dir("claude-mcp-plugin-manifest");
    write(
        &root.join(".claude-plugin").join("plugin.json"),
        serde_json::json!({
            "name": "tools",
            "mcpServers": [
                { "inline": { "command": "${CLAUDE_PLUGIN_ROOT}/bin/inline" } },
                "./servers/extra.json",
                "../outside.json"
            ]
        }),
    );
    write(
        &root.join("servers").join("extra.json"),
        serde_json::json!({ "mcpServers": { "extra": { "command": "extra" } } }),
    );
    write(
        &root.parent().unwrap().join("outside.json"),
        serde_json::json!({ "escaped": { "command": "nope" } }),
    );
    write(
        &root.join(".mcp.json"),
        serde_json::json!({
            "inline": { "command": "shadowed" },
            "rooted": { "command": "rooted" }
        }),
    );

    let servers = plugin_servers(&[plugin(&root, "tools")], &[]);

    assert_eq!(
        ids(&servers),
        [
            "plugin:tools:inline",
            "plugin:tools:extra",
            "plugin:tools:rooted"
        ]
    );
    assert_eq!(servers[0].source, "${CLAUDE_PLUGIN_ROOT}/bin/inline");
    assert!(servers.iter().all(|server| !server.togglable));
    assert!(servers.iter().all(|server| server.origin == ORIGIN_PLUGIN));
    assert!(servers.iter().all(|server| server.via == "tools"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_plugin_server_the_user_disabled_by_its_scoped_name_is_off() {
    let root = scratch_dir("claude-mcp-plugin-disabled");
    write(
        &root.join(".mcp.json"),
        serde_json::json!({
            "tracker": { "type": "http", "url": "https://tracker.example/mcp" },
            "search": { "command": "search", "disabled": true },
            "notes": { "command": "notes" }
        }),
    );

    let servers = plugin_servers(&[plugin(&root, "kit")], &["plugin:kit:tracker".to_string()]);

    let states: Vec<_> = servers
        .iter()
        .map(|server| (server.name.as_str(), server.enabled))
        .collect();
    assert_eq!(
        states,
        [("tracker", false), ("search", false), ("notes", true)]
    );
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

    let servers = plugin_servers(
        &[
            plugin(&root.join("empty"), "empty"),
            plugin(&broken, "broken"),
        ],
        &[],
    );

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
                server.via.as_str(),
                server.enabled,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("local:db", "db", "tools", true),
            ("local:db#2", "db --ro", "api", true),
            ("local:docs", "https://docs.example/mcp", "2 projects", true),
        ]
    );
    assert!(servers
        .iter()
        .all(|server| !server.togglable && server.origin == ORIGIN_LOCAL));
}

#[test]
fn no_projects_means_no_local_servers() {
    assert!(local_servers(&serde_json::json!({ "mcpServers": {} })).is_empty());
    assert!(local_servers(&Value::Null).is_empty());
}
