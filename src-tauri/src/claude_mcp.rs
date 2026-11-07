use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use crate::dto::{AdapterError, McpServerDto};
use crate::mcp::{claude_disabled_list, claude_servers, ORIGIN_LOCAL, ORIGIN_PLUGIN};
use crate::plugin_files::{plugin_file, read_json, PluginSource};
use crate::project::normalize_project_key;

pub const SWITCHED_IN_PROJECTS: &str = "Claude Code turns MCP servers off one project at a time, so on-n-off switches them only inside a project. Choose a project under Scope, or use /mcp in Claude Code.";

pub fn servers(claude_json: Option<&Path>, plugins: &[PluginSource]) -> Vec<McpServerDto> {
    let document = read(claude_json);
    let mut servers = document
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(claude_servers)
        .unwrap_or_default();
    servers.extend(plugin_servers(plugins));
    servers.extend(local_servers(&document));
    servers
}

pub fn in_project(
    claude_json: &Path,
    project: &Path,
    rows: &mut Vec<McpServerDto>,
    brought: &mut Vec<McpServerDto>,
) {
    let document = read(Some(claude_json));
    rows.retain(|server| server.origin != ORIGIN_LOCAL);
    let entry = entry(&document, project);
    match entry {
        Some(entry) => brought.extend(
            entry
                .get("mcpServers")
                .and_then(Value::as_object)
                .map(claude_servers)
                .unwrap_or_default(),
        ),
        None => rows.iter_mut().for_each(|server| server.togglable = false),
    }
    let disabled = entry.map(claude_disabled_list).unwrap_or_default();
    for server in rows.iter_mut().chain(brought.iter_mut()) {
        server.enabled = !disabled.iter().any(|name| name == listed_as(server));
    }
}

fn listed_as(server: &McpServerDto) -> &str {
    if server.origin == ORIGIN_PLUGIN {
        &server.id
    } else {
        &server.name
    }
}

pub fn switch(
    project: Option<&Path>,
    server: &McpServerDto,
    on: bool,
) -> Result<impl FnOnce(&mut Value) -> Result<(), AdapterError>, AdapterError> {
    let project = project.ok_or_else(|| AdapterError::message(SWITCHED_IN_PROJECTS))?;
    let key = key_of(project);
    let shown = project.display().to_string();
    let name = listed_as(server).to_string();
    Ok(move |document: &mut Value| list(document, &key, &shown, &name, on))
}

fn list(
    document: &mut Value,
    key: &str,
    shown: &str,
    name: &str,
    on: bool,
) -> Result<(), AdapterError> {
    let entry = entry_mut(document, key).ok_or_else(|| {
        AdapterError::message(format!(
            "Claude Code has not recorded this project: {shown}"
        ))
    })?;
    let list = entry
        .entry("disabledMcpServers")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| AdapterError::message("disabledMcpServers must be an array"))?;
    list.retain(|item| item.as_str() != Some(name));
    if !on {
        list.push(Value::String(name.to_string()));
    }
    Ok(())
}

fn read(claude_json: Option<&Path>) -> Value {
    claude_json
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn entry<'d>(document: &'d Value, project: &Path) -> Option<&'d Value> {
    keyed(
        document.get("projects")?.as_object()?.iter(),
        &key_of(project),
    )
}

fn entry_mut<'d>(document: &'d mut Value, key: &str) -> Option<&'d mut Map<String, Value>> {
    keyed(
        document.get_mut("projects")?.as_object_mut()?.iter_mut(),
        key,
    )?
    .as_object_mut()
}

fn keyed<'d, V>(entries: impl Iterator<Item = (&'d String, V)>, key: &str) -> Option<V> {
    entries
        .filter(|(path, _)| normalize_project_key(path) == key)
        .map(|(_, entry)| entry)
        .next()
}

fn key_of(project: &Path) -> String {
    normalize_project_key(&claude_key(project).to_string_lossy())
}

fn claude_key(project: &Path) -> PathBuf {
    let dir = lexical(project);
    let Some(root) = dir
        .ancestors()
        .find(|candidate| holds_git(candidate))
        .map(Path::to_path_buf)
    else {
        return dir;
    };
    main_checkout(&root).unwrap_or(root)
}

fn holds_git(dir: &Path) -> bool {
    fs::metadata(dir.join(".git")).is_ok_and(|found| found.is_dir() || found.is_file())
}

fn main_checkout(root: &Path) -> Option<PathBuf> {
    let link = fs::read_to_string(root.join(".git")).ok()?;
    let gitdir = lexical(&root.join(link.trim().strip_prefix("gitdir:")?.trim()));
    let common = lexical(&gitdir.join(fs::read_to_string(gitdir.join("commondir")).ok()?.trim()));
    if gitdir.parent()? != common.join("worktrees") {
        return None;
    }
    let back = gitdir.join(fs::read_to_string(gitdir.join("gitdir")).ok()?.trim());
    if fs::canonicalize(back).ok()? != fs::canonicalize(root).ok()?.join(".git") {
        return None;
    }
    if common.file_name()? == ".git" {
        return common.parent().map(Path::to_path_buf);
    }
    (!common.join(".git").exists()).then_some(common)
}

fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn plugin_servers(plugins: &[PluginSource]) -> Vec<McpServerDto> {
    let mut out = Vec::new();
    for plugin in plugins {
        for mut server in claude_servers(&plugin_server_map(&plugin.root)) {
            server.id = format!("plugin:{}:{}", plugin.name, server.name);
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

fn local_servers(claude_json: &Value) -> Vec<McpServerDto> {
    let Some(projects) = claude_json.get("projects").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut definitions: BTreeMap<(String, String, String), (Vec<String>, bool)> = BTreeMap::new();
    for (project, entry) in projects {
        let Some(servers) = entry.get("mcpServers").and_then(Value::as_object) else {
            continue;
        };
        let disabled = claude_disabled_list(entry);
        for server in claude_servers(servers) {
            let on = !disabled.contains(&server.name);
            let definition = definitions
                .entry((server.name, server.system, server.source))
                .or_default();
            definition.0.push(project.clone());
            definition.1 |= on;
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

#[cfg(test)]
mod tests;
