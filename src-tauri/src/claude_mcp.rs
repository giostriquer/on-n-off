use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde_json::{Map, Value};

use crate::dto::McpServerDto;
use crate::mcp::{claude_disabled_list, claude_servers, ORIGIN_LOCAL, ORIGIN_PLUGIN};
use crate::plugin_files::{plugin_file, read_json, PluginSource};
use crate::project::normalize_project_key;

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

fn server_map(value: Value) -> Map<String, Value> {
    let Value::Object(mut map) = value else {
        return Map::new();
    };
    match map.remove("mcpServers") {
        Some(Value::Object(servers)) => servers,
        _ => map,
    }
}

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
