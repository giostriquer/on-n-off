//! Every hook the providers would run, read out of the files that declare them.
//!
//! One module for all of them, the way `mcp.rs` is: the vocabularies differ — Claude fires
//! `PreToolUse`, Codex `pre_tool_use`, and neither is translated here — but the *shape* is the
//! same everywhere, `<Event>[] -> { matcher?, hooks: [handler] }`, so one walker serves Claude's
//! `settings.json`, Codex's `config.toml` and every plugin manifest, and an adapter only says
//! which plugins are enabled.
//!
//! Scope is the user's own configuration and the enabled plugins: `~/.claude/settings.json`,
//! `~/.codex/hooks.json`, `~/.codex/config.toml`. Project overlays, `settings.local.json` and
//! managed settings stay out, so a row can never claim a hook that only fires inside one
//! repository. Claude *merges* hooks across sources rather than letting one override another,
//! which is why every contributor here simply appends.
//!
//! Ids are Codex's `[hooks.state]` key verbatim — `<plugin-id>:<source>:<event>:<group>:<index>`
//! with the event snake_cased — because Codex keys enablement that way and a second id scheme
//! would only have to be mapped back to it. Claude's ids have the same shape, with an empty
//! plugin segment for user settings. A row's id therefore survives every edit that does not move
//! the entry: it counts within its event, so an event written in above it moves nothing.
//!
//! Unique, with one exception the files themselves create: two event keys that snake_case alike
//! — `Stop` and `stop` in one file — share a key in Codex's own state table too, so they share a
//! row id here, and switching one off switches both. Both rows are still listed, because
//! dropping one would hide a hook that does run.
//!
//! An adapter calls [`claude_hooks`] or [`codex_hooks`] once, with the enabled plugins it found,
//! and gets finished rows back; which files that means is this module's business, not the
//! adapter's. Both run inside the startup scan, so `config.toml` is read once and parsed once
//! for all three things Codex keeps in it.
//!
//! Nothing here fails: a file that will not parse contributes no rows. A screen that cannot be
//! wrong about one plugin is worth more than one that refuses to draw.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::dto::HookDto;

/// The one Claude settings file in scope (see the module doc).
const CLAUDE_SETTINGS: &str = "settings.json";
/// Codex's own hook file, beside `config.toml`.
const CODEX_HOOKS: &str = "hooks.json";
const CODEX_CONFIG: &str = "config.toml";
/// `[hooks.state]` is enablement, not a hook.
const STATE: &str = "state";
/// Codex's key for a manifest that holds its events inline instead of naming a file.
const INLINE: &str = "plugin.json#hooks[0]";

/// What a batch of rows has in common: the segments of their ids that do not vary, and the two
/// labels the screen shows.
struct Origin {
    plugin_id: Option<String>,
    /// The `<source>` segment of every id built here: a plugin-relative file path, or the
    /// settings file's name.
    key: String,
    /// Where the row comes from, for a human: the plugin's name, or that same file name.
    label: String,
    description: String,
}

impl Origin {
    /// A file the user owns: no plugin, and the file's name serves as both id segment and label.
    fn file(name: &str) -> Self {
        Self {
            plugin_id: None,
            key: name.to_string(),
            label: name.to_string(),
            description: String::new(),
        }
    }
}

/// One enabled plugin, as the adapter that walked the provider's inventory found it: the id
/// Codex keys its state by, the name a row shows, and the directory the manifest sits in. A
/// disabled plugin's hooks do not run, so the adapter simply leaves it out.
pub struct PluginSource {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

/// Every hook Claude would run from this home: the user's own `settings.json` first, then each
/// enabled plugin. Claude *merges* hooks across sources instead of letting one override another,
/// which is why every contributor appends. There is no state table to apply: Claude has no
/// per-entry switch.
pub fn claude_hooks(root: &Path, plugins: &[PluginSource]) -> Vec<HookDto> {
    let mut hooks = claude_settings_hooks(&read_text(&root.join(CLAUDE_SETTINGS)));
    for plugin in plugins {
        hooks.extend(claude_plugin_hooks(&plugin.id, &plugin.name, &plugin.root));
    }
    hooks
}

/// Every hook Codex would run from this home, with `[hooks.state]` already applied — enablement
/// is keyed by the row's own id, so it can only be looked up once every source has contributed.
/// `config.toml` is read once and parsed once here for all three of the things Codex keeps in
/// it, because this runs inside the startup scan.
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

/// A file that is not there, or cannot be read, reads as empty: it contributes no rows, exactly
/// like one that will not parse.
fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// `config.toml` as a value, or `Null` when it will not parse — every reader below asks it for a
/// key, and `Null` has none of them.
fn parse_toml(text: &str) -> Value {
    toml::from_str(text).unwrap_or(Value::Null)
}

/// The `hooks` key of `~/.claude/settings.json`.
fn claude_settings_hooks(text: &str) -> Vec<HookDto> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(events) = value.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    rows(&Origin::file(CLAUDE_SETTINGS), events)
}

/// One enabled Claude plugin's hooks: the manifest's `hooks` key when it has one, and
/// `hooks/hooks.json` otherwise, which is where Claude looks when the manifest says nothing.
fn claude_plugin_hooks(plugin_id: &str, name: &str, root: &Path) -> Vec<HookDto> {
    plugin_hooks(
        plugin_id,
        name,
        root,
        ".claude-plugin",
        Some("hooks/hooks.json"),
    )
}

/// `~/.codex/hooks.json`: a plugin's hook file without a plugin around it.
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

/// The `[hooks]` table of an already-parsed `~/.codex/config.toml`, plus the legacy top-level
/// `notify` key.
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

/// One enabled Codex plugin's hooks, from the manifest's `hooks` key alone. There is no default
/// file on purpose: a plugin that serves both providers keeps Claude's entries in
/// `hooks/hooks.json` and names its Codex file separately, so defaulting would list Claude's
/// events under Codex.
fn codex_plugin_hooks(plugin_id: &str, name: &str, root: &Path) -> Vec<HookDto> {
    plugin_hooks(plugin_id, name, root, ".codex-plugin", None)
}

/// Apply `[hooks.state]`, which is where Codex keeps enablement and trust for each entry. It
/// keys them by `<plugin>:<source>:<event>:<group>:<index>`, which is exactly the id built
/// above, so this is a lookup rather than a reconstruction.
///
/// **verified** for plugin sources on a real machine — both a plugin naming a file
/// (`<id>:hooks/codex.json:session_start:0:0`) and one holding its events inline
/// (`<id>:plugin.json#hooks[0]:stop:0:0`); the key Codex writes for `hooks.json` and for the
/// `[hooks]` table has not been observed, so those rows may
/// simply never match. That is the safe way round: an entry Codex has no state for runs, so a
/// miss leaves the row enabled rather than claiming it is off.
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
        // An entry that only records a trusted hash is trusted, not disabled.
        hook.enabled = entry
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
    }
}

/// Flatten `<Event>[] -> { matcher?, hooks: [handler] }` into one row per handler, in file
/// order: `serde_json`'s `preserve_order` keeps a map in the order it was written, so the rows
/// come out the way the file reads and `sort::sort_hooks` only has to be stable to keep it.
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
            // An entry is a matcher with its handlers under `hooks`; a bare handler with no list
            // around it is taken as well, because that is the natural spelling in TOML.
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
    // Both providers run a handler with no `type` as a command, which is what it usually is.
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

/// What the row shows as the thing that runs, left exactly as the file spells it.
fn command_of(kind: &str, handler: &Value) -> String {
    if kind == "mcp_tool" {
        // An MCP handler names a server and a tool instead of a command line.
        return match (field(handler, "server"), field(handler, "tool")) {
            (Some(server), Some(tool)) => format!("{server} · {tool}"),
            (Some(one), None) | (None, Some(one)) => one.to_string(),
            (None, None) => String::new(),
        };
    }
    // `url` is what an `http` handler has in place of a command line. A command written across
    // lines keeps its lines: the row truncates it and the tooltip shows the whole of it, so
    // collapsing here would destroy the only copy anything can show.
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

/// `PreToolUse` -> `pre_tool_use`, which is how Codex spells the same event in its
/// `[hooks.state]` keys. A name already in snake case comes back unchanged.
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

/// A hook file is `{ description?, hooks: { <Event>: [...] } }`, but the event map on its own is
/// accepted too: Codex's bundled plugins ship both spellings, one wrapped and one bare.
fn event_map(value: &Value) -> Option<&Map<String, Value>> {
    let map = value.as_object()?;
    match map.get("hooks").and_then(Value::as_object) {
        Some(inner) => Some(inner),
        None => Some(map),
    }
}

/// A plugin's own description can run to several lines; a row shows one.
fn description_of(value: &Value) -> String {
    one_line(field(value, "description").unwrap_or_default())
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Codex's original hook, from before there were events: `notify = [argv…]`, run when a turn
/// ends. It has no state entry, no matcher and no event of its own, so it is shown under the
/// event it stands in for rather than as a row with an empty one.
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

/// What a plugin manifest's `hooks` key names.
enum Declared<'a> {
    Files(Vec<String>),
    /// The events themselves, written into the manifest.
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
            // One file can be named twice under two spellings (`hooks/a.json` and
            // `./hooks/a.json`). Both resolve to one key, so reading it twice would list every
            // row twice under the first row's ids; the second naming names nothing new.
            let mut seen = HashSet::new();
            files
                .iter()
                .filter_map(|rel| resolve(root, rel))
                .filter(|(key, _)| seen.insert(key.clone()))
                .flat_map(|(key, path)| {
                    file_hooks(plugin_id, name, &key, &path, &plugin_description)
                })
                .collect()
        }
        // No `hooks` key at all: only Claude has a file it looks for anyway.
        None => default_file
            .and_then(|rel| resolve(root, rel))
            .map(|(key, path)| file_hooks(plugin_id, name, &key, &path, &plugin_description))
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

/// One hook file the manifest named, already resolved to its `[hooks.state]` key and its path
/// on this machine.
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
        // The hook file describes the hooks; the plugin's own description stands in for it.
        description: if own.is_empty() {
            plugin_description.to_string()
        } else {
            own
        },
    };
    rows(&origin, events)
}

/// A manifest path is written the way its author reads it (`./hooks/codex.json`) and has to end
/// up both as a path on this machine and as the `/`-joined key Codex writes into
/// `[hooks.state]`. `..` is dropped rather than followed: a plugin's hook file lives inside the
/// plugin, and a manifest is not a reason to read anything above it.
fn resolve(root: &Path, rel: &str) -> Option<(String, PathBuf)> {
    let parts: Vec<&str> = rel
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .collect();
    if parts.is_empty() {
        return None;
    }
    let mut path = root.to_path_buf();
    for part in &parts {
        path.push(part);
    }
    Some((parts.join("/"), path))
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests;
