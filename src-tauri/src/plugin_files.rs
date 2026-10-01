use std::path::{Path, PathBuf};

use serde_json::Value;

pub struct PluginSource {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PluginFile {
    pub key: String,
    pub path: PathBuf,
}

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

pub fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests;
