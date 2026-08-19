//! `~/.on-n-off/installed-items.json`: provenance of every skill/agent on-n-off copied out of a
//! marketplace. Item folders themselves stay byte-identical to upstream; this file is what lets
//! us tell "upstream moved" from "you edited it".

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dto::{AdapterError, AgentId, ItemKind, ItemScope};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSource {
    pub owner: String,
    pub repo: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub plugin_name: String,
    /// Plugin folder inside the repository (`""` for the repository root), `/`-separated.
    #[serde(default)]
    pub plugin_root: String,
    /// Skill folder or agent file inside the repository, `/`-separated.
    pub upstream_path: String,
    /// Marketplace entries this item was detected to need (`plugin/kind/path`), high
    /// confidence only; recorded so a later removal can warn about dependants.
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub commit_sha: String,
    pub plugin_version: Option<String>,
    pub installed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledItem {
    pub id: String,
    pub provider: AgentId,
    pub kind: ItemKind,
    pub name: String,
    /// Absolute path of the folder (skill) or file (agent) as written on this machine.
    pub target_path: String,
    pub scope: ItemScope,
    pub source: ItemSource,
    pub installed: Installed,
    /// Relative path → sha256 hex of every file as installed.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// Upstream sha the user chose to keep their copy over ("Keep mine").
    #[serde(default)]
    pub dismissed_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledItemsFile {
    pub version: u32,
    #[serde(default)]
    pub items: Vec<InstalledItem>,
}

impl Default for InstalledItemsFile {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            items: Vec::new(),
        }
    }
}

impl InstalledItemsFile {
    pub fn upsert(&mut self, item: InstalledItem) {
        match self
            .items
            .iter_mut()
            .find(|existing| existing.id == item.id)
        {
            Some(existing) => *existing = item,
            None => self.items.push(item),
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<InstalledItem> {
        let index = self.items.iter().position(|item| item.id == id)?;
        Some(self.items.remove(index))
    }

    pub fn find_by_target(&self, provider: AgentId, target_path: &Path) -> Option<&InstalledItem> {
        let wanted = normalize(target_path);
        self.items.iter().find(|item| {
            item.provider == provider && normalize(Path::new(&item.target_path)) == wanted
        })
    }

    pub fn for_provider(&self, provider: AgentId, scope: &ItemScope) -> Vec<&InstalledItem> {
        self.items
            .iter()
            .filter(|item| item.provider == provider && same_scope(&item.scope, scope))
            .collect()
    }
}

fn same_scope(a: &ItemScope, b: &ItemScope) -> bool {
    match (a, b) {
        (ItemScope::Global, ItemScope::Global) => true,
        (ItemScope::Project { project_path: x }, ItemScope::Project { project_path: y }) => {
            normalize(Path::new(x)) == normalize(Path::new(y))
        }
        _ => false,
    }
}

/// Case-insensitive on Windows, `/`-separated everywhere: two spellings of one folder match.
fn normalize(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_end_matches('/').to_string();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

pub fn kind_key(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Skill => "skill",
        ItemKind::Agent => "agent",
    }
}

pub fn item_id(provider: AgentId, kind: ItemKind, target_path: &Path) -> String {
    format!(
        "{}:{}:{}",
        provider.key(),
        kind_key(kind),
        normalize(target_path)
    )
}

/// How `ItemSource::depends_on` names a marketplace entry: `plugin/kind/upstream_path`.
pub fn dependency_key(plugin_name: &str, kind: ItemKind, upstream_path: &str) -> String {
    format!("{plugin_name}/{}/{upstream_path}", kind_key(kind))
}

pub fn load(path: &Path) -> Result<InstalledItemsFile, AdapterError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(InstalledItemsFile::default());
        }
        Err(error) => {
            return Err(AdapterError::write(
                error.to_string(),
                Some(path.display().to_string()),
            ));
        }
    };
    serde_json::from_str(&text)
        .map_err(|error| AdapterError::parse(error.to_string(), Some(path.display().to_string())))
}

pub fn save(path: &Path, file: &InstalledItemsFile) -> Result<(), AdapterError> {
    let text = serde_json::to_string_pretty(file)
        .map_err(|error| AdapterError::message(error.to_string()))?;
    crate::usage::cache_io::atomic_write(path, &text)
        .map_err(|error| AdapterError::write(error.to_string(), Some(path.display().to_string())))
}
