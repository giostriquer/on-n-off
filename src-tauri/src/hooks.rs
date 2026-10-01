use std::collections::HashSet;
use std::path::Path;

use serde_json::{Map, Value};

use crate::dto::HookDto;
use crate::plugin_files::{plugin_file, read_json, PluginSource};

const CLAUDE_SETTINGS: &str = "settings.json";
const CODEX_HOOKS: &str = "hooks.json";
const CODEX_CONFIG: &str = "config.toml";
const STATE: &str = "state";
const INLINE: &str = "plugin.json#hooks[0]";

struct Origin {
    plugin_id: Option<String>,
    key: String,
    label: String,
    description: String,
}

impl Origin {
    fn file(name: &str) -> Self {
        Self {
            plugin_id: None,
            key: name.to_string(),
            label: name.to_string(),
            description: String::new(),
        }
    }
}

pub fn claude_hooks(root: &Path, plugins: &[PluginSource]) -> Vec<HookDto> {
    let mut hooks = claude_settings_hooks(&read_text(&root.join(CLAUDE_SETTINGS)));
    for plugin in plugins {
        hooks.extend(claude_plugin_hooks(&plugin.id, &plugin.name, &plugin.root));
    }
    hooks
}

pub fn codex_hooks(root: &Path, plugins: &[PluginSource]) -> Vec<HookDto> {
    let config = parse_toml(&read_text(&root.join(CODEX_CONFIG)));
    let mut hooks = codex_hooks_file(&read_text(&root.join(CODEX_HOOKS)));
    hooks.extend(codex_config_hooks(&config));
    for plugin in plugins {
        hooks.extend(codex_plugin_hooks(&plugin.id, &plugin.name, &plugin.root));
    }
    apply_codex_state(&mut hooks, &config);
    hooks
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn parse_toml(text: &str) -> Value {
    toml::from_str(text).unwrap_or(Value::Null)
}

fn claude_settings_hooks(text: &str) -> Vec<HookDto> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(events) = value.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    rows(&Origin::file(CLAUDE_SETTINGS), events)
}

fn claude_plugin_hooks(plugin_id: &str, name: &str, root: &Path) -> Vec<HookDto> {
    plugin_hooks(
        plugin_id,
        name,
        root,
        ".claude-plugin",
        Some("hooks/hooks.json"),
    )
}

fn codex_hooks_file(text: &str) -> Vec<HookDto> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(events) = event_map(&value) else {
        return Vec::new();
    };
    let mut origin = Origin::file(CODEX_HOOKS);
    origin.description = description_of(&value);
    rows(&origin, events)
}

fn codex_config_hooks(value: &Value) -> Vec<HookDto> {
    let mut out = Vec::new();
    if let Some(table) = value.get("hooks").and_then(Value::as_object) {
        let events: Map<String, Value> = table
            .iter()
            .filter(|(key, _)| key.as_str() != STATE)
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        out.extend(rows(&Origin::file(CODEX_CONFIG), &events));
    }
    out.extend(notify_row(value));
    out
}

fn codex_plugin_hooks(plugin_id: &str, name: &str, root: &Path) -> Vec<HookDto> {
    plugin_hooks(plugin_id, name, root, ".codex-plugin", None)
}

fn apply_codex_state(hooks: &mut [HookDto], value: &Value) {
    let Some(state) = value
        .get("hooks")
        .and_then(|hooks| hooks.get(STATE))
        .and_then(Value::as_object)
    else {
        return;
    };
    for hook in hooks.iter_mut() {
        let Some(entry) = state.get(&hook.id) else {
            continue;
        };
        hook.enabled = entry
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
    }
}

fn rows(origin: &Origin, events: &Map<String, Value>) -> Vec<HookDto> {
    let mut out = Vec::new();
    for (event, groups) in events {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for (group, entry) in groups.iter().enumerate() {
            let matcher = entry
                .get("matcher")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            let handlers = match entry.get("hooks").and_then(Value::as_array) {
                Some(list) => list.as_slice(),
                None if entry.get("type").is_some() => std::slice::from_ref(entry),
                None => continue,
            };
            for (index, handler) in handlers.iter().enumerate() {
                out.push(row(origin, event, matcher, group, index, handler));
            }
        }
    }
    out
}

fn row(
    origin: &Origin,
    event: &str,
    matcher: &str,
    group: usize,
    index: usize,
    handler: &Value,
) -> HookDto {
    let kind = field(handler, "type").unwrap_or("command");
    HookDto {
        id: format!(
            "{}:{}:{}:{group}:{index}",
            origin.plugin_id.as_deref().unwrap_or(""),
            origin.key,
            snake_case(event)
        ),
        event: event.to_string(),
        matcher: matcher.to_string(),
        handler: kind.to_string(),
        command: command_of(kind, handler),
        source: origin.label.clone(),
        plugin_id: origin.plugin_id.clone(),
        description: origin.description.clone(),
        enabled: true,
    }
}

fn command_of(kind: &str, handler: &Value) -> String {
    if kind == "mcp_tool" {
        return match (field(handler, "server"), field(handler, "tool")) {
            (Some(server), Some(tool)) => format!("{server} · {tool}"),
            (Some(one), None) | (None, Some(one)) => one.to_string(),
            (None, None) => String::new(),
        };
    }
    field(handler, "command")
        .or_else(|| field(handler, "url"))
        .unwrap_or_default()
        .to_string()
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|found| !found.is_empty())
}

fn snake_case(event: &str) -> String {
    let mut out = String::with_capacity(event.len() + 4);
    for (position, letter) in event.chars().enumerate() {
        if letter.is_uppercase() && position > 0 && !out.ends_with('_') {
            out.push('_');
        }
        out.extend(letter.to_lowercase());
    }
    out
}

fn event_map(value: &Value) -> Option<&Map<String, Value>> {
    let map = value.as_object()?;
    match map.get("hooks").and_then(Value::as_object) {
        Some(inner) => Some(inner),
        None => Some(map),
    }
}

fn description_of(value: &Value) -> String {
    one_line(field(value, "description").unwrap_or_default())
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn notify_row(value: &Value) -> Option<HookDto> {
    let command = match value.get("notify")? {
        Value::String(one) => one.trim().to_string(),
        Value::Array(argv) => argv
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };
    if command.is_empty() {
        return None;
    }
    Some(HookDto {
        id: ":notify:notification:0:0".to_string(),
        event: "Notification".to_string(),
        matcher: String::new(),
        handler: "command".to_string(),
        command,
        source: CODEX_CONFIG.to_string(),
        plugin_id: None,
        description: "Legacy notify key.".to_string(),
        enabled: true,
    })
}

enum Declared<'a> {
    Files(Vec<String>),
    Inline(&'a Value),
}

fn plugin_hooks(
    plugin_id: &str,
    name: &str,
    root: &Path,
    manifest_dir: &str,
    default_file: Option<&str>,
) -> Vec<HookDto> {
    let manifest = read_json(&root.join(manifest_dir).join("plugin.json"));
    let plugin_description = manifest.as_ref().map(description_of).unwrap_or_default();
    match declared(manifest.as_ref()) {
        Some(Declared::Inline(value)) => {
            let origin = Origin {
                plugin_id: Some(plugin_id.to_string()),
                key: INLINE.to_string(),
                label: name.to_string(),
                description: plugin_description,
            };
            event_map(value)
                .map(|events| rows(&origin, events))
                .unwrap_or_default()
        }
        Some(Declared::Files(files)) => {
            let mut seen = HashSet::new();
            files
                .iter()
                .filter_map(|rel| plugin_file(root, rel))
                .filter(|file| seen.insert(file.key.clone()))
                .flat_map(|file| {
                    file_hooks(plugin_id, name, &file.key, &file.path, &plugin_description)
                })
                .collect()
        }
        None => default_file
            .and_then(|rel| plugin_file(root, rel))
            .map(|file| file_hooks(plugin_id, name, &file.key, &file.path, &plugin_description))
            .unwrap_or_default(),
    }
}

fn declared(manifest: Option<&Value>) -> Option<Declared<'_>> {
    let value = manifest?.get("hooks")?;
    match value {
        Value::String(path) => Some(Declared::Files(vec![path.clone()])),
        Value::Array(paths) => Some(Declared::Files(
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        )),
        Value::Object(_) => Some(Declared::Inline(value)),
        _ => None,
    }
}

fn file_hooks(
    plugin_id: &str,
    name: &str,
    key: &str,
    path: &Path,
    plugin_description: &str,
) -> Vec<HookDto> {
    let Some(value) = read_json(path) else {
        return Vec::new();
    };
    let Some(events) = event_map(&value) else {
        return Vec::new();
    };
    let own = description_of(&value);
    let origin = Origin {
        plugin_id: Some(plugin_id.to_string()),
        key: key.to_string(),
        label: name.to_string(),
        description: if own.is_empty() {
            plugin_description.to_string()
        } else {
            own
        },
    };
    rows(&origin, events)
}

#[cfg(test)]
mod tests;
