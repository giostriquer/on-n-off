//! What a provider plugin keeps on disk: the plugin itself, as an adapter found it in the
//! provider's inventory, and the files its manifest names.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// One enabled plugin: its id (`name@marketplace`), the name its rows show, and the directory
/// its manifest sits in. A disabled plugin contributes nothing, so an adapter leaves it out.
pub struct PluginSource {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

/// A file a manifest names, inside the plugin: the `/`-joined path from the plugin root, and
/// where it is on this machine.
#[derive(Debug, PartialEq, Eq)]
pub struct PluginFile {
    pub key: String,
    pub path: PathBuf,
}

/// `rel`, as a manifest writes it (`./hooks/codex.json`), resolved under `root`; `None` for a
/// path that could leave the plugin. Decided by the text rather than by the platform's path
/// parsing, the same on every OS: a leading `/` or `\`, any `..` segment, and any segment with a
/// `:` (a drive, `C:outside.json` being drive-relative on Windows) are refused. That is
/// on-n-off's own rule: a plugin's files live inside the plugin, and a manifest is not a reason
/// to read anything above it. `.` and empty segments are skipped, so `./a.json` and `a//b.json`
/// name what they appear to.
pub fn plugin_file(root: &Path, rel: &str) -> Option<PluginFile> {
    let rel = rel.trim();
    if rel.starts_with(['/', '\\']) {
        return None;
    }
    let mut parts = Vec::new();
    for part in rel.split(['/', '\\']).map(str::trim) {
        match part {
            "" | "." => {}
            ".." => return None,
            part if part.contains(':') => return None,
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return None;
    }
    let mut path = root.to_path_buf();
    for part in &parts {
        path.push(part);
    }
    Some(PluginFile {
        key: parts.join("/"),
        path,
    })
}

/// A JSON file, or `None` when it is missing, unreadable or not JSON.
pub fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests;
