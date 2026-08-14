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
struct PluginManifest {
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct MarketplaceFile {
    #[serde(default)]
    plugins: Vec<MarketplacePlugin>,
}

#[derive(Debug, Deserialize)]
struct MarketplacePlugin {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    source: Option<MarketplaceSource>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MarketplaceSource {
    Path(String),
    Object {
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

#[cfg(test)]
pub fn installed_version(install_path: &Path, inventory_version: Option<&str>) -> String {
    installed_hint(install_path, inventory_version).version
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

#[cfg(test)]
pub fn catalog_versions(marketplace_root: &Path) -> HashMap<String, String> {
    catalog_hints(marketplace_root)
        .into_iter()
        .filter(|(_, hint)| !hint.version.is_empty())
        .map(|(name, hint)| (name, hint.version))
        .collect()
}

pub fn fill_remote_version(hint: &mut VersionHint) {
    if usable(Some(hint.version.as_str())).is_some() {
        return;
    }
    if hint.remote_url.is_empty() || hint.remote_rev.is_empty() {
        return;
    }
    if let Some(version) = remote_plugin_version(&hint.remote_url, &hint.remote_path, &hint.remote_rev) {
        hint.version = version;
    }
}

pub fn resolve_versions(installed: &VersionHint, catalog: Option<&VersionHint>) -> (String, String, bool) {
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
    let local_version = plugin.local_source_path().and_then(|relative| {
        manifest_version(&root.join(relative.trim_start_matches("./")))
    });
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
    const RELATIVE: &[&str] = &[
        ".claude-plugin/marketplace.json",
        ".codex-plugin/marketplace.json",
        ".agents/plugins/marketplace.json",
        ".cursor-plugin/marketplace.json",
    ];
    for relative in RELATIVE {
        let Some(text) = fs::read_to_string(root.join(relative)).ok() else {
            continue;
        };
        if let Ok(file) = serde_json::from_str::<MarketplaceFile>(&text) {
            return Some(file);
        }
    }
    None
}

fn manifest_version(install_path: &Path) -> Option<String> {
    for relative in [".codex-plugin/plugin.json", ".claude-plugin/plugin.json", "plugin.json"] {
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
    let manifest = serde_json::from_str::<PluginManifest>(text).ok()?;
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
    if let Some(cached) = cache().lock().ok().and_then(|cache| cache.get(&key).cloned()) {
        return cached;
    }
    let version = github_manifest_urls(url, path, rev).into_iter().find_map(|manifest_url| {
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
    let mut prefix = path.trim().trim_start_matches("./").trim_end_matches('/').to_string();
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
    const RELATIVE: &[&str] = &[
        ".claude-plugin/marketplace.json",
        ".codex-plugin/marketplace.json",
        ".agents/plugins/marketplace.json",
        ".cursor-plugin/marketplace.json",
    ];
    for relative in RELATIVE {
        let url = format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{relative}");
        let Some(text) = fetch_text(&url) else {
            continue;
        };
        if let Ok(file) = serde_json::from_str::<MarketplaceFile>(&text) {
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
    if let Ok(text) =
        fs::read_to_string(root.join(".git").join("refs").join("remotes").join("origin").join("HEAD"))
    {
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
    fn local_source_path(&self) -> Option<&str> {
        match &self.source {
            Some(MarketplaceSource::Path(path)) => Some(path.as_str()),
            Some(MarketplaceSource::Object { path, url, .. }) if url.as_deref().unwrap_or("").is_empty() => {
                path.as_deref()
            }
            _ => None,
        }
    }

    fn git_ref(&self) -> Option<String> {
        match &self.source {
            Some(MarketplaceSource::Object { git_ref, .. }) => git_ref.clone(),
            _ => None,
        }
    }

    fn remote_pin(&self) -> (String, String, String) {
        let Some(MarketplaceSource::Object { url, path, sha, git_ref, .. }) = &self.source else {
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
            path.as_deref().unwrap_or("").trim().trim_start_matches("./").to_string(),
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
thread_local! {
    static TEST_REMOTE: std::cell::RefCell<Option<fn(&str, &str, &str) -> Option<String>>> =
        const { std::cell::RefCell::new(None) };
    static TEST_FETCH: std::cell::RefCell<Option<fn(&str) -> Option<String>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_remote_version(url: &str, path: &str, rev: &str) -> Option<Option<String>> {
    TEST_REMOTE.with(|slot| slot.borrow().map(|fetch| fetch(url, path, rev)))
}

#[cfg(test)]
fn with_remote_fetch<F: FnOnce()>(fetch: fn(&str, &str, &str) -> Option<String>, run: F) {
    TEST_REMOTE.with(|slot| *slot.borrow_mut() = Some(fetch));
    run();
    TEST_REMOTE.with(|slot| *slot.borrow_mut() = None);
}

#[cfg(test)]
fn test_fetch_text(url: &str) -> Option<Option<String>> {
    TEST_FETCH.with(|slot| slot.borrow().map(|fetch| fetch(url)))
}

#[cfg(test)]
fn with_fetch_text<F: FnOnce()>(fetch: fn(&str) -> Option<String>, run: F) {
    TEST_FETCH.with(|slot| *slot.borrow_mut() = Some(fetch));
    run();
    TEST_FETCH.with(|slot| *slot.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn inventory_version_wins() {
        let root = crate::paths::scratch_dir("on-n-off-plugin-meta-inv").join("unknown");
        fs::create_dir_all(&root).unwrap();
        assert_eq!(installed_version(&root, Some("1.2.0")), "1.2.0");
        assert_eq!(installed_version(&root, Some(" unknown ")), "");
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn reads_codex_manifest_then_folder_name() {
        let root = crate::paths::scratch_dir("on-n-off-plugin-meta-dir").join("0.22.1");
        fs::create_dir_all(root.join(".codex-plugin")).unwrap();
        fs::write(
            root.join(".codex-plugin/plugin.json"),
            r#"{"name":"workbench","version":"0.22.1"}"#,
        )
        .unwrap();
        assert_eq!(installed_version(&root, None), "0.22.1");
        fs::remove_file(root.join(".codex-plugin/plugin.json")).unwrap();
        assert_eq!(installed_version(&root, None), "0.22.1");
        let unknown = root.parent().unwrap().join("unknown");
        fs::create_dir_all(&unknown).unwrap();
        assert_eq!(installed_version(&unknown, Some("unknown")), "");
        assert_eq!(installed_version(&root.join("missing-cache"), None), "");
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn catalog_uses_release_version_not_commit_sha() {
        let root = crate::paths::scratch_dir("on-n-off-plugin-meta-cat");
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        fs::create_dir_all(root.join("plugins/workbench/.claude-plugin")).unwrap();
        fs::write(
            root.join("plugins/workbench/.claude-plugin/plugin.json"),
            r#"{"name":"workbench","version":"0.22.1"}"#,
        )
        .unwrap();
        fs::write(
            root.join(".claude-plugin/marketplace.json"),
            r#"{
              "plugins": [
                {"name":"workbench","version":"0.23.0","source":"./plugins/workbench"},
                {"name":"toolkit","source":{"source":"local","path":"./plugins/workbench"}},
                {"name":"api","source":{"source":"git-subdir","path":"plugins/api","ref":"v1.5.5"}},
                {"name":"mainline","source":{"source":"git-subdir","path":"plugins/mainline","ref":"main"}},
                {"name":"superpowers","source":{"source":"url","url":"https://github.com/obra/superpowers.git","sha":"b36e0829c6d0140e93cfef2ca599b1b07d4a7797"}}
              ]
            }"#,
        )
        .unwrap();
        let hints = catalog_hints(&root);
        assert_eq!(hints.get("workbench").map(|hint| hint.version.as_str()), Some("0.23.0"));
        assert_eq!(hints.get("toolkit").map(|hint| hint.version.as_str()), Some("0.22.1"));
        assert_eq!(hints.get("api").map(|hint| hint.version.as_str()), Some("v1.5.5"));
        assert!(!hints.contains_key("mainline"));
        assert_eq!(hints.get("superpowers").map(|hint| hint.version.as_str()), Some(""));
        with_remote_fetch(
            |url, path, rev| {
                assert!(url.contains("obra/superpowers"));
                assert!(path.is_empty());
                assert_eq!(rev, "b36e0829c6d0140e93cfef2ca599b1b07d4a7797");
                Some("6.3.0".into())
            },
            || {
                let mut hint = catalog_hints(&root).remove("superpowers").unwrap();
                fill_remote_version(&mut hint);
                assert_eq!(hint.version, "6.3.0");
                let installed = VersionHint {
                    version: "6.3.0".into(),
                    ..VersionHint::default()
                };
                let (version, upstream, drift) = resolve_versions(&installed, Some(&hint));
                assert_eq!(version, "6.3.0");
                assert_eq!(upstream, "6.3.0");
                assert!(!drift);
            },
        );
        let versions = catalog_versions(&root);
        assert_eq!(versions.get("api").map(String::as_str), Some("v1.5.5"));
        assert!(!versions.contains_key("superpowers"));
        assert_eq!(
            github_manifest_urls(
                "https://github.com/mongodb/agent-skills.git",
                "plugins/mongodb",
                "b4ea8150a020b9babaddc6c271c6dc177c06a83f",
            )[0],
            "https://raw.githubusercontent.com/mongodb/agent-skills/b4ea8150a020b9babaddc6c271c6dc177c06a83f/plugins/mongodb/.claude-plugin/plugin.json"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_marketplace_json_beats_stale_local_clone() {
        let root = crate::paths::scratch_dir("on-n-off-plugin-meta-remote-mkt");
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        fs::write(
            root.join(".claude-plugin/marketplace.json"),
            r#"{
              "plugins": [
                {"name":"workbench","version":"0.22.1","source":"./plugins/workbench"},
                {"name":"toolkit","version":"0.5.0","source":"./plugins/toolkit"}
              ]
            }"#,
        )
        .unwrap();
        let mut hints = catalog_hints(&root);
        assert_eq!(hints.get("workbench").map(|hint| hint.version.as_str()), Some("0.22.1"));
        assert_eq!(hints.get("toolkit").map(|hint| hint.version.as_str()), Some("0.5.0"));
        with_fetch_text(
            |url| {
                assert!(url.contains("example/workshop"));
                assert!(url.contains("marketplace.json"));
                Some(
                    r#"{
                      "plugins": [
                        {"name":"workbench","version":"0.23.0","source":"./plugins/workbench"},
                        {"name":"toolkit","version":"0.6.0","source":"./plugins/toolkit"}
                      ]
                    }"#
                    .into(),
                )
            },
            || {
                apply_remote_marketplace_versions(&mut hints, "example/workshop", &root);
                assert_eq!(hints.get("workbench").map(|hint| hint.version.as_str()), Some("0.23.0"));
                assert_eq!(hints.get("toolkit").map(|hint| hint.version.as_str()), Some("0.6.0"));
                let installed = VersionHint {
                    version: "0.22.1".into(),
                    ..VersionHint::default()
                };
                let (_, upstream, drift) = resolve_versions(&installed, hints.get("workbench"));
                assert_eq!(upstream, "0.23.0");
                assert!(drift);
            },
        );
        assert_eq!(
            github_repo("git@github.com:example/workshop.git"),
            Some(("example".into(), "workshop".into()))
        );
        assert_eq!(
            github_repo("example/workshop"),
            Some(("example".into(), "workshop".into()))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_compares_release_versions_only() {
        let installed = VersionHint {
            version: "6.3.0".into(),
            ..VersionHint::default()
        };
        let sha_only = VersionHint {
            version: String::new(),
            remote_url: "https://github.com/obra/superpowers.git".into(),
            remote_rev: "b36e0829c6d0140e93cfef2ca599b1b07d4a7797".into(),
            ..VersionHint::default()
        };
        let (version, upstream, drift) = resolve_versions(&installed, Some(&sha_only));
        assert_eq!(version, "6.3.0");
        assert_eq!(upstream, "");
        assert!(!drift);

        let behind = VersionHint {
            version: "0.23.0".into(),
            ..VersionHint::default()
        };
        let installed_semver = VersionHint {
            version: "0.22.1".into(),
            ..VersionHint::default()
        };
        let (_, upstream, drift) = resolve_versions(&installed_semver, Some(&behind));
        assert_eq!(upstream, "0.23.0");
        assert!(drift);
        assert!(!out_of_sync("v0.22.1", "0.22.1"));
        assert!(!out_of_sync("1.0.0", ""));
        assert!(!out_of_sync("6.3.0", "b36e082"));
    }

    #[test]
    fn strip_verbatim_drops_windows_prefix() {
        assert_eq!(
            strip_verbatim(r"\\?\C:\Users\me\.codex\.tmp\bundled"),
            PathBuf::from(r"C:\Users\me\.codex\.tmp\bundled")
        );
        assert_eq!(strip_verbatim(" /tmp/market "), PathBuf::from("/tmp/market"));
    }
}
