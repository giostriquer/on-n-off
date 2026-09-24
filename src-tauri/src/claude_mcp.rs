//! The Claude MCP servers that live outside the user's own list in `~/.claude.json`: the ones an
//! enabled plugin brings, and the ones Claude keeps for particular projects (its local scope).
//!
//! Both are read-only rows. A plugin's servers come and go with the plugin, so the plugin's own
//! switch is the one that turns them off everywhere. A local-scope server applies only inside
//! the projects that list it; outside them it is one row per definition naming those projects,
//! and a project's own view shows that project's servers as project rows instead.
//!
//! What switches a server off inside a project is that project's own `disabledMcpServers` in
//! `~/.claude.json`: Claude Code 2.1.281 reads no other list, and names a plugin's server there
//! by its scoped `plugin:<plugin>:<server>` name. So the all-projects view shows a plugin's server
//! on whenever its plugin is enabled, and a project's view applies that project's list
//! ([`project_servers`]).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde_json::{Map, Value};

use crate::dto::McpServerDto;
use crate::mcp::{claude_disabled_list, claude_servers, ORIGIN_LOCAL, ORIGIN_PLUGIN};
use crate::plugin_files::{plugin_file, read_json, PluginSource};
use crate::project::normalize_project_key;

/// Every server the enabled `plugins` bring, named the way Claude names them
/// (`plugin:<plugin>:<server>`). Placeholders such as `${CLAUDE_PLUGIN_ROOT}` are shown as
/// written: expanding them would show a path the file does not contain.
///
/// Two plugins of one name from two marketplaces give the same scoped name, as they do in Claude
/// Code itself; each is still its own row.
pub fn plugin_servers(plugins: &[PluginSource]) -> Vec<McpServerDto> {
    let mut out = Vec::new();
    for plugin in plugins {
        for mut server in claude_servers(&plugin_server_map(&plugin.root), &[]) {
            server.id = format!("plugin:{}:{}", plugin.name, server.name);
            server.togglable = false;
            server.origin = ORIGIN_PLUGIN.to_string();
            server.plugin_id = Some(plugin.id.clone());
            out.push(server);
        }
    }
    out
}

/// One plugin's servers, merged the way Claude Code 2.1.281 merges them: its root `.mcp.json`
/// first, then each entry of the manifest's `mcpServers` in order (an object inline, a path to a
/// file, or a list of either), a later server of the same name replacing an earlier one.
fn plugin_server_map(root: &Path) -> Map<String, Value> {
    let mut merged = Map::new();
    if let Some(file) = read_json(&root.join(".mcp.json")) {
        merged.extend(server_map(file));
    }
    let declared = read_json(&root.join(".claude-plugin").join("plugin.json"))
        .and_then(|manifest| manifest.get("mcpServers").cloned());
    let entries = match declared {
        Some(Value::Array(items)) => items,
        Some(item) => vec![item],
        None => Vec::new(),
    };
    for entry in entries {
        merged.extend(declared_servers(root, entry));
    }
    merged
}

/// One manifest entry's servers. A path outside the plugin is refused (`plugin_file`), and an
/// MCP bundle (`.mcpb`, or the older `.dxt`) is an archive Claude Code installs, which on-n-off
/// does not open.
fn declared_servers(root: &Path, entry: Value) -> Map<String, Value> {
    match entry {
        Value::Object(_) => server_map(entry),
        Value::String(path) if is_bundle(&path) => Map::new(),
        Value::String(path) => plugin_file(root, &path)
            .and_then(|file| read_json(&file.path))
            .map(server_map)
            .unwrap_or_default(),
        _ => Map::new(),
    }
}

fn is_bundle(path: &str) -> bool {
    let path = path.trim().to_ascii_lowercase();
    path.ends_with(".mcpb") || path.ends_with(".dxt")
}

/// A servers file holds `{"mcpServers": {…}}` or is the bare map of servers; both are in use.
fn server_map(value: Value) -> Map<String, Value> {
    let Value::Object(mut map) = value else {
        return Map::new();
    };
    match map.remove("mcpServers") {
        Some(Value::Object(servers)) => servers,
        _ => map,
    }
}

/// The local-scope servers of every project in `~/.claude.json`, one row per definition (name,
/// transport and source alike), on when any of its projects has it on; `projects` names them.
/// Ids number a second definition of one name (`local:<name>#2`) in the file's order, so they
/// can change when a project is added; nothing keys on them.
pub fn local_servers(claude_json: &Value) -> Vec<McpServerDto> {
    let Some(projects) = claude_json.get("projects").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut definitions: BTreeMap<(String, String, String), (Vec<String>, bool)> = BTreeMap::new();
    for (project, entry) in projects {
        let Some(servers) = entry.get("mcpServers").and_then(Value::as_object) else {
            continue;
        };
        for server in claude_servers(servers, &claude_disabled_list(entry)) {
            let definition = definitions
                .entry((server.name, server.system, server.source))
                .or_default();
            definition.0.push(project.clone());
            definition.1 |= server.enabled;
        }
    }

    let mut per_name: HashMap<String, usize> = HashMap::new();
    definitions
        .into_iter()
        .map(|((name, system, source), (mut projects, enabled))| {
            projects.sort_unstable();
            let count = per_name.entry(name.clone()).or_default();
            *count += 1;
            let id = if *count == 1 {
                format!("local:{name}")
            } else {
                format!("local:{name}#{count}")
            };
            McpServerDto {
                id,
                name,
                system,
                source,
                enabled,
                togglable: false,
                origin: ORIGIN_LOCAL.to_string(),
                plugin_id: None,
                projects,
            }
        })
        .collect()
}

/// A project's view, from its entry in `~/.claude.json`: the all-projects rows for servers kept
/// for particular projects go, a plugin server the entry's `disabledMcpServers` names by its
/// scoped name reads off, and the project's own servers come back (off when that list names
/// them) for the caller to show as project rows.
pub fn project_servers(
    servers: &mut Vec<McpServerDto>,
    claude_json: &Value,
    project: &Path,
) -> Vec<McpServerDto> {
    servers.retain(|server| server.origin != ORIGIN_LOCAL);
    let key = normalize_project_key(&project.to_string_lossy());
    let Some(entry) = claude_json
        .get("projects")
        .and_then(Value::as_object)
        .and_then(|projects| {
            projects
                .iter()
                .find(|(path, _)| normalize_project_key(path) == key)
        })
        .map(|(_, entry)| entry)
    else {
        return Vec::new();
    };
    let disabled = claude_disabled_list(entry);
    for server in servers.iter_mut() {
        if server.origin == ORIGIN_PLUGIN && disabled.contains(&server.id) {
            server.enabled = false;
        }
    }
    entry
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|own| claude_servers(own, &disabled))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
