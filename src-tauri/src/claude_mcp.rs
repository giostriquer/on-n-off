//! The Claude MCP servers that live outside the user's own list in `~/.claude.json`: the ones an
//! enabled plugin brings, and the ones Claude keeps for particular projects (its local scope).
//!
//! Both are read-only rows. A plugin's servers come and go with the plugin, so the plugin's own
//! switch is the one that turns them off. A local-scope server applies only inside the projects
//! that list it; outside them it is one row per definition saying which projects have it, and a
//! project's own view shows that project's servers as project rows instead (`project.rs`).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use crate::dto::McpServerDto;
use crate::hooks::PluginSource;
use crate::mcp::{claude_disabled_list, claude_servers, ORIGIN_LOCAL, ORIGIN_PLUGIN};

/// Every server the enabled `plugins` bring, named the way Claude names them
/// (`plugin:<plugin>:<server>`). A plugin declares them in its manifest (`mcpServers`: an object
/// inline, a path to a file, or a list of either) and in `.mcp.json` at its root; both are read,
/// and the manifest's entry wins a name defined twice. Placeholders such as `${CLAUDE_PLUGIN_ROOT}`
/// are shown as written: expanding them would show a path the file does not contain.
/// `disabled` is the user's `disabledMcpServers`, which names a plugin server by its scoped name.
pub fn plugin_servers(plugins: &[PluginSource], disabled: &[String]) -> Vec<McpServerDto> {
    let mut out = Vec::new();
    for plugin in plugins {
        let mut seen: Vec<String> = Vec::new();
        for map in plugin_server_maps(&plugin.root) {
            for mut server in claude_servers(&map, &[]) {
                if seen.contains(&server.name) {
                    continue;
                }
                seen.push(server.name.clone());
                let scoped = format!("plugin:{}:{}", plugin.name, server.name);
                server.enabled &= !disabled.contains(&scoped);
                server.id = scoped;
                server.togglable = false;
                server.origin = ORIGIN_PLUGIN.to_string();
                server.via = plugin.name.clone();
                out.push(server);
            }
        }
    }
    out
}

/// The server maps a plugin declares, manifest first.
fn plugin_server_maps(root: &Path) -> Vec<Map<String, Value>> {
    let mut maps = Vec::new();
    let declared = read_json(&root.join(".claude-plugin").join("plugin.json"))
        .and_then(|manifest| manifest.get("mcpServers").cloned());
    match declared {
        Some(Value::Array(items)) => {
            for item in items {
                push_declared(root, item, &mut maps);
            }
        }
        Some(item) => push_declared(root, item, &mut maps),
        None => {}
    }
    if let Some(file) = read_json(&root.join(".mcp.json")) {
        maps.push(server_map(file));
    }
    maps
}

fn push_declared(root: &Path, item: Value, maps: &mut Vec<Map<String, Value>>) {
    match item {
        Value::String(path) => {
            if let Some(file) = inside(root, &path).and_then(|path| read_json(&path)) {
                maps.push(server_map(file));
            }
        }
        Value::Object(_) => maps.push(server_map(item)),
        _ => {}
    }
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

/// `relative` under `root`, or `None` for a path Claude refuses: absolute, or climbing out.
fn inside(root: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative);
    let escapes = relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)));
    (!escapes).then(|| root.join(relative))
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// The local-scope servers of every project in `~/.claude.json`, one row per definition (name,
/// transport and source alike), on when any of its projects has it on. `via` names the project,
/// or says how many projects have it.
pub fn local_servers(claude_json: &Value) -> Vec<McpServerDto> {
    let Some(projects) = claude_json.get("projects").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut definitions: BTreeMap<(String, String, String), (Vec<&str>, bool)> = BTreeMap::new();
    for (project, entry) in projects {
        let Some(servers) = entry.get("mcpServers").and_then(Value::as_object) else {
            continue;
        };
        for server in claude_servers(servers, &claude_disabled_list(entry)) {
            let definition = definitions
                .entry((server.name, server.system, server.source))
                .or_default();
            definition.0.push(project.as_str());
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
            let via = match projects.as_slice() {
                [only] => crate::project::project_label(only),
                many => format!("{} projects", many.len()),
            };
            McpServerDto {
                id,
                name,
                system,
                source,
                enabled,
                togglable: false,
                origin: ORIGIN_LOCAL.to_string(),
                via,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
