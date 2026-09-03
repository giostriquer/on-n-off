use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionHint {
    pub version: String,
    remote_url: String,
    remote_path: String,
    remote_rev: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct PluginManifest {
    #[serde(default)]
    pub(crate) version: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    /// Explicit skill folders (or `SKILL.md` files) relative to the plugin root.
    #[serde(default)]
    pub(crate) skills: Option<Vec<String>>,
    /// Plugin-level assets declared inline; only their presence matters here.
    #[serde(default)]
    pub(crate) commands: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) hooks: Option<serde_json::Value>,
    #[serde(default, rename = "mcpServers")]
    pub(crate) mcp_servers: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MarketplaceFile {
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) plugins: Vec<MarketplacePlugin>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MarketplacePlugin {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) version: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) source: Option<MarketplaceSource>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum MarketplaceSource {
    Path(String),
    Object {
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        repo: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        #[serde(rename = "ref")]
        git_ref: Option<String>,
        #[serde(default)]
        sha: Option<String>,
    },
}

/// Marketplace manifest locations, in the order providers look them up.
pub(crate) const MARKETPLACE_MANIFESTS: &[&str] = &[
    ".claude-plugin/marketplace.json",
    ".codex-plugin/marketplace.json",
    ".agents/plugins/marketplace.json",
    ".cursor-plugin/marketplace.json",
];

/// Plugin manifest locations, in lookup order.
pub(crate) const PLUGIN_MANIFESTS: &[&str] = &[
    ".cursor-plugin/plugin.json",
    ".codex-plugin/plugin.json",
    ".claude-plugin/plugin.json",
    "plugin.json",
];

pub(crate) fn parse_marketplace_text(text: &str) -> Option<MarketplaceFile> {
    serde_json::from_str::<MarketplaceFile>(text).ok()
}

pub(crate) fn parse_plugin_manifest(text: &str) -> Option<PluginManifest> {
    serde_json::from_str::<PluginManifest>(text).ok()
}

pub fn installed_hint(install_path: &Path, inventory_version: Option<&str>) -> VersionHint {
    VersionHint {
        version: if let Some(version) = usable(inventory_version) {
            version
        } else if let Some(version) = manifest_version(install_path) {
            version
        } else {
            folder_version(install_path).unwrap_or_default()
        },
        ..VersionHint::default()
    }
}

pub fn catalog_hints(marketplace_root: &Path) -> HashMap<String, VersionHint> {
    let mut hints = HashMap::new();
    let Some(file) = read_marketplace_file(marketplace_root) else {
        return hints;
    };
    for plugin in file.plugins {
        let hint = catalog_plugin_hint(marketplace_root, &plugin);
        if hint.version.is_empty() && hint.remote_url.is_empty() {
            continue;
        }
        hints.insert(plugin.name, hint);
    }
    hints
}

pub fn apply_remote_marketplace_versions(
    hints: &mut HashMap<String, VersionHint>,
    origin: &str,
    marketplace_root: &Path,
) {
    let origin = origin.trim();
    let origin = if origin.is_empty() {
        git_origin_url(marketplace_root).unwrap_or_default()
    } else {
        origin.to_string()
    };
    let Some((owner, repo)) = github_repo(&origin) else {
        return;
    };
    for branch in git_remote_branches(marketplace_root) {
        if let Some(file) = fetch_remote_marketplace(&owner, &repo, &branch) {
            merge_marketplace_versions(hints, file);
            return;
        }
    }
}

pub fn fill_remote_version(hint: &mut VersionHint) {
    if usable(Some(hint.version.as_str())).is_some() {
        return;
    }
    if hint.remote_url.is_empty() || hint.remote_rev.is_empty() {
        return;
    }
    if let Some(version) =
        remote_plugin_version(&hint.remote_url, &hint.remote_path, &hint.remote_rev)
    {
        hint.version = version;
    }
}

pub fn resolve_versions(
    installed: &VersionHint,
    catalog: Option<&VersionHint>,
) -> (String, String, bool) {
    let catalog = catalog.cloned().unwrap_or_default();
    let upstream = usable(Some(catalog.version.as_str()))
        .filter(|value| !is_sha(value))
        .unwrap_or_default();
    let installed_version = usable(Some(installed.version.as_str()))
        .filter(|value| !is_sha(value))
        .unwrap_or_else(|| installed.version.clone());
    let out_of_sync = out_of_sync(&installed_version, &upstream);
    (installed_version, upstream, out_of_sync)
}

pub fn out_of_sync(installed: &str, upstream: &str) -> bool {
    match (
        usable(Some(installed)).filter(|value| !is_sha(value)),
        usable(Some(upstream)).filter(|value| !is_sha(value)),
    ) {
        (Some(installed), Some(upstream)) => version_key(&installed) != version_key(&upstream),
        _ => false,
    }
}

pub fn strip_verbatim(path: &str) -> PathBuf {
    let path = path.trim();
    PathBuf::from(path.strip_prefix(r"\\?\").unwrap_or(path))
}

fn catalog_plugin_hint(root: &Path, plugin: &MarketplacePlugin) -> VersionHint {
    let local_version = plugin
        .local_source_path()
        .and_then(|relative| manifest_version(&root.join(relative.trim_start_matches("./"))));
    let version = usable(plugin.version.as_deref())
        .or(local_version)
        .or_else(|| {
            plugin
                .git_ref()
                .filter(|value| is_version_ref(value))
                .and_then(|git_ref| usable(Some(&git_ref)))
        })
        .filter(|value| !is_sha(value))
        .unwrap_or_default();
    let (remote_url, remote_path, remote_rev) = plugin.remote_pin();
    VersionHint {
        version,
        remote_url,
        remote_path,
        remote_rev,
    }
}

fn read_marketplace_file(root: &Path) -> Option<MarketplaceFile> {
    for relative in MARKETPLACE_MANIFESTS {
        let Some(text) = fs::read_to_string(root.join(relative)).ok() else {
            continue;
        };
        if let Some(file) = parse_marketplace_text(&text) {
            return Some(file);
        }
    }
    None
}

fn manifest_version(install_path: &Path) -> Option<String> {
    for relative in PLUGIN_MANIFESTS {
        if let Ok(text) = fs::read_to_string(install_path.join(relative)) {
            if let Some(version) = parse_manifest_text(Some(&text)) {
                return Some(version);
            }
        }
    }
    None
}

fn parse_manifest_text(text: Option<&str>) -> Option<String> {
    let text = text?;
    let manifest = parse_plugin_manifest(text)?;
    usable(manifest.version.as_deref()).filter(|value| !is_sha(value))
}

fn folder_version(install_path: &Path) -> Option<String> {
    if !install_path.is_dir() {
        return None;
    }
    usable(install_path.file_name()?.to_str()).filter(|value| !is_sha(value))
}

fn usable(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("unknown") {
        return None;
    }
    Some(value.to_string())
}

fn version_key(value: &str) -> String {
    value
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_ascii_lowercase()
}

fn is_sha(value: &str) -> bool {
    let value = value.trim();
    value.len() >= 7 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn is_version_ref(value: &str) -> bool {
    let key = version_key(value);
    if matches!(
        key.as_str(),
        "main" | "master" | "head" | "latest" | "trunk" | "develop"
    ) {
        return false;
    }
    !is_sha(&key)
}

fn remote_plugin_version(url: &str, path: &str, rev: &str) -> Option<String> {
    #[cfg(test)]
    if let Some(version) = test_remote_version(url, path, rev) {
        return version;
    }
    let key = format!("{url}|{path}|{rev}");
    if let Some(cached) = cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned())
    {
        return cached;
    }
    let version = github_manifest_urls(url, path, rev)
        .into_iter()
        .find_map(|manifest_url| {
            let text = fetch_text(&manifest_url)?;
            parse_manifest_text(Some(&text))
        });
    if let Ok(mut cache) = cache().lock() {
        cache.insert(key, version.clone());
    }
    version
}

fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fetch_text(url: &str) -> Option<String> {
    #[cfg(test)]
    if let Some(result) = test_fetch_text(url) {
        return result;
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .user_agent("on-n-off/0.1")
        .build();
    agent.get(url).call().ok()?.into_string().ok()
}

fn github_manifest_urls(url: &str, path: &str, rev: &str) -> Vec<String> {
    let Some((owner, repo)) = github_repo(url) else {
        return Vec::new();
    };
    let mut prefix = path
        .trim()
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_string();
    if !prefix.is_empty() {
        prefix.push('/');
    }
    [
        ".claude-plugin/plugin.json",
        ".codex-plugin/plugin.json",
        "plugin.json",
    ]
    .into_iter()
    .map(|file| format!("https://raw.githubusercontent.com/{owner}/{repo}/{rev}/{prefix}{file}"))
    .collect()
}

fn github_repo(url: &str) -> Option<(String, String)> {
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))
        .unwrap_or(url);
    let mut parts = rest.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    if owner.contains(':') || owner.contains('\\') {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

fn merge_marketplace_versions(hints: &mut HashMap<String, VersionHint>, file: MarketplaceFile) {
    for plugin in file.plugins {
        let Some(version) = usable(plugin.version.as_deref()).filter(|value| !is_sha(value)) else {
            continue;
        };
        hints
            .entry(plugin.name)
            .and_modify(|hint| hint.version = version.clone())
            .or_insert(VersionHint {
                version,
                ..VersionHint::default()
            });
    }
}

fn fetch_remote_marketplace(owner: &str, repo: &str, branch: &str) -> Option<MarketplaceFile> {
    for relative in MARKETPLACE_MANIFESTS {
        let url = format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{relative}");
        let Some(text) = fetch_text(&url) else {
            continue;
        };
        if let Some(file) = parse_marketplace_text(&text) {
            if !file.plugins.is_empty() {
                return Some(file);
            }
        }
    }
    None
}

fn git_origin_url(root: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join(".git").join("config")).ok()?;
    let mut in_origin = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line.eq_ignore_ascii_case("[remote \"origin\"]");
            continue;
        }
        if in_origin {
            if let Some(url) = line
                .strip_prefix("url = ")
                .or_else(|| line.strip_prefix("url="))
            {
                let url = url.trim();
                if !url.is_empty() {
                    return Some(url.to_string());
                }
            }
        }
    }
    None
}

fn git_remote_branches(root: &Path) -> Vec<String> {
    let mut branches = Vec::new();
    if let Ok(text) = fs::read_to_string(
        root.join(".git")
            .join("refs")
            .join("remotes")
            .join("origin")
            .join("HEAD"),
    ) {
        if let Some(branch) = text.trim().strip_prefix("ref: refs/remotes/origin/") {
            let branch = branch.trim();
            if !branch.is_empty() {
                branches.push(branch.to_string());
            }
        }
    }
    for fallback in ["main", "master"] {
        if !branches.iter().any(|branch| branch == fallback) {
            branches.push(fallback.to_string());
        }
    }
    branches
}

impl MarketplacePlugin {
    pub(crate) fn local_source_path(&self) -> Option<&str> {
        match &self.source {
            Some(MarketplaceSource::Path(path)) => Some(path.as_str()),
            Some(MarketplaceSource::Object {
                path, url, source, ..
            }) if url.as_deref().unwrap_or("").is_empty()
                && source
                    .as_deref()
                    .is_none_or(|kind| kind.is_empty() || kind == "local") =>
            {
                path.as_deref()
            }
            _ => None,
        }
    }

    /// `{ "source": "github", "repo": "owner/name", "ref"?: ... }` -> `(owner, name, ref)`.
    pub(crate) fn github_source(&self) -> Option<(String, String, Option<String>)> {
        let Some(MarketplaceSource::Object {
            source,
            repo,
            git_ref,
            ..
        }) = &self.source
        else {
            return None;
        };
        if source.as_deref() != Some("github") {
            return None;
        }
        let repo = repo.as_deref()?.trim().trim_end_matches(".git");
        let (owner, name) = repo.split_once('/')?;
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        Some((owner.to_string(), name.to_string(), git_ref.clone()))
    }

    fn git_ref(&self) -> Option<String> {
        match &self.source {
            Some(MarketplaceSource::Object { git_ref, .. }) => git_ref.clone(),
            _ => None,
        }
    }

    fn remote_pin(&self) -> (String, String, String) {
        let Some(MarketplaceSource::Object {
            url,
            path,
            sha,
            git_ref,
            ..
        }) = &self.source
        else {
            return Default::default();
        };
        let url = url.as_deref().unwrap_or("").trim();
        if url.is_empty() {
            return Default::default();
        }
        let rev = usable_sha(sha.as_deref())
            .or_else(|| git_ref.clone().filter(|value| is_version_ref(value)))
            .unwrap_or_default();
        (
            url.to_string(),
            path.as_deref()
                .unwrap_or("")
                .trim()
                .trim_start_matches("./")
                .to_string(),
            rev,
        )
    }
}

fn usable_sha(value: Option<&str>) -> Option<String> {
    let value = usable(value)?;
    if is_sha(&value) {
        Some(value.to_ascii_lowercase())
    } else {
        None
    }
}

#[cfg(test)]
type RemoteVersionFetch = fn(&str, &str, &str) -> Option<String>;

#[cfg(test)]
type TextFetch = fn(&str) -> Option<String>;

#[cfg(test)]
thread_local! {
    static TEST_REMOTE: std::cell::RefCell<Option<RemoteVersionFetch>> =
        const { std::cell::RefCell::new(None) };
    static TEST_FETCH: std::cell::RefCell<Option<TextFetch>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_remote_version(url: &str, path: &str, rev: &str) -> Option<Option<String>> {
    TEST_REMOTE.with(|slot| slot.borrow().map(|fetch| fetch(url, path, rev)))
}

#[cfg(test)]
fn with_remote_fetch<F: FnOnce()>(fetch: RemoteVersionFetch, run: F) {
    TEST_REMOTE.with(|slot| *slot.borrow_mut() = Some(fetch));
    run();
    TEST_REMOTE.with(|slot| *slot.borrow_mut() = None);
}

#[cfg(test)]
fn test_fetch_text(url: &str) -> Option<Option<String>> {
    TEST_FETCH.with(|slot| slot.borrow().map(|fetch| fetch(url)))
}

#[cfg(test)]
pub(crate) fn with_fetch_text<T>(fetch: TextFetch, run: impl FnOnce() -> T) -> T {
    TEST_FETCH.with(|slot| *slot.borrow_mut() = Some(fetch));
    let result = run();
    TEST_FETCH.with(|slot| *slot.borrow_mut() = None);
    result
}

#[cfg(test)]
mod tests;
