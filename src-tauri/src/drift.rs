use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::adapter::AgentAdapter;
use crate::dto::{AdapterError, AgentTabDto};
use crate::plugin_meta::{
    parse_marketplace_text, parse_plugin_manifest, MarketplaceFile, MarketplacePlugin,
    MarketplaceSource, MARKETPLACE_MANIFESTS, PLUGIN_MANIFESTS,
};

pub const REMEMBERED_FOR: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy)]
pub enum Reach<'a> {
    Local,
    Remote(&'a Memo),
    Forced(&'a Memo),
}

pub struct Marketplace {
    name: String,
    offers: Offers,
}

enum Offers {
    Checkout { root: PathBuf, origin: String },
    Listing(HashMap<String, String>),
}

impl Marketplace {
    pub fn checkout(name: impl Into<String>, root: PathBuf, origin: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            offers: Offers::Checkout {
                root,
                origin: origin.into(),
            },
        }
    }

    pub fn listing(
        name: impl Into<String>,
        versions: impl IntoIterator<Item = (String, Option<String>)>,
    ) -> Self {
        let mut listed = HashMap::new();
        for (plugin, version) in versions {
            listed.entry(plugin).or_insert(version.unwrap_or_default());
        }
        Self {
            name: name.into(),
            offers: Offers::Listing(listed),
        }
    }
}

pub fn read(
    adapter: &dyn AgentAdapter,
    view: Option<&str>,
    reach: Reach<'_>,
) -> Result<AgentTabDto, AdapterError> {
    let mut tab = adapter.list_scope(view)?;
    let fetch = reach.fetch();
    let catalogs: HashMap<String, HashMap<String, Hint>> = adapter
        .marketplaces()
        .into_iter()
        .map(|marketplace| (marketplace.name, marketplace.offers.read(fetch.as_ref())))
        .collect();
    for plugin in &mut tab.plugins {
        let offered = catalogs
            .get(&plugin.source)
            .and_then(|hints| hints.get(&plugin.name))
            .map(|hint| hint.offered(fetch.as_ref()))
            .unwrap_or_default();
        (plugin.upstream, plugin.out_of_sync) = compare(&plugin.version, &offered);
    }
    Ok(tab)
}

pub fn installed(install_path: &Path, recorded: Option<&str>) -> String {
    usable(recorded)
        .or_else(|| manifest_version(install_path))
        .or_else(|| folder_version(install_path))
        .unwrap_or_default()
}

pub struct Memo {
    answers: Mutex<HashMap<String, Answer>>,
    clock: fn() -> Instant,
}

struct Answer {
    at: Instant,
    text: Option<String>,
}

impl Memo {
    pub fn new() -> Self {
        Self::on(Instant::now)
    }

    pub(crate) fn on(clock: fn() -> Instant) -> Self {
        Self {
            answers: Mutex::new(HashMap::new()),
            clock,
        }
    }

    fn answers(&self) -> MutexGuard<'_, HashMap<String, Answer>> {
        self.answers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn reader(&self, forced: bool) -> Fetch<'_> {
        Fetch {
            memo: self,
            started: (self.clock)(),
            window: (!forced).then_some(REMEMBERED_FOR),
        }
    }
}

impl<'a> Reach<'a> {
    fn fetch(self) -> Option<Fetch<'a>> {
        match self {
            Self::Local => None,
            Self::Remote(memo) => Some(memo.reader(false)),
            Self::Forced(memo) => Some(memo.reader(true)),
        }
    }
}

struct Fetch<'a> {
    memo: &'a Memo,
    started: Instant,
    window: Option<Duration>,
}

impl Fetch<'_> {
    fn text(&self, url: &str) -> Option<String> {
        let known = self
            .memo
            .answers()
            .get(url)
            .filter(|answer| {
                let age = (self.memo.clock)().saturating_duration_since(answer.at);
                answer.at >= self.started || self.window.is_some_and(|window| age < window)
            })
            .map(|answer| answer.text.clone());
        if let Some(text) = known {
            return text;
        }
        let Ok(text) = fetch_text(url) else {
            return None;
        };
        let now = (self.memo.clock)();
        let mut answers = self.memo.answers();
        answers.retain(|_, answer| now.saturating_duration_since(answer.at) < REMEMBERED_FOR);
        answers.insert(
            url.to_string(),
            Answer {
                at: now,
                text: text.clone(),
            },
        );
        text
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Hint {
    version: String,
    pin: Option<Pin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pin {
    url: String,
    path: String,
    rev: String,
}

impl Offers {
    fn read(self, fetch: Option<&Fetch<'_>>) -> HashMap<String, Hint> {
        match self {
            Self::Checkout { root, origin } => {
                let mut hints = catalog(&root);
                if let Some(fetch) = fetch {
                    merge_remote_marketplace(&mut hints, &origin, &root, fetch);
                }
                hints
            }
            Self::Listing(listed) => listed
                .into_iter()
                .map(|(plugin, version)| (plugin, Hint { version, pin: None }))
                .collect(),
        }
    }
}

impl Hint {
    fn offered(&self, fetch: Option<&Fetch<'_>>) -> String {
        if usable(Some(&self.version)).is_some() {
            return self.version.clone();
        }
        match (&self.pin, fetch) {
            (Some(pin), Some(fetch)) => remote_plugin_version(pin, fetch).unwrap_or_default(),
            _ => self.version.clone(),
        }
    }
}

fn catalog(marketplace_root: &Path) -> HashMap<String, Hint> {
    let mut hints = HashMap::new();
    let Some(file) = read_marketplace_file(marketplace_root) else {
        return hints;
    };
    for plugin in file.plugins {
        let hint = catalog_plugin_hint(marketplace_root, &plugin);
        if hint.version.is_empty() && hint.pin.is_none() {
            continue;
        }
        hints.insert(plugin.name, hint);
    }
    hints
}

fn merge_remote_marketplace(
    hints: &mut HashMap<String, Hint>,
    origin: &str,
    marketplace_root: &Path,
    fetch: &Fetch<'_>,
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
        if let Some(file) = fetch_remote_marketplace(&owner, &repo, &branch, fetch) {
            merge_marketplace_versions(hints, file);
            return;
        }
    }
}

fn compare(installed: &str, offered: &str) -> (String, bool) {
    match (release(installed), release(offered)) {
        (Some(installed), Some(upstream)) => {
            let behind = version_key(&installed) != version_key(&upstream);
            (upstream, behind)
        }
        _ => (String::new(), false),
    }
}

fn release(value: &str) -> Option<String> {
    usable(Some(value)).filter(|value| !is_sha(value))
}

fn catalog_plugin_hint(root: &Path, plugin: &MarketplacePlugin) -> Hint {
    let local_version = plugin
        .local_source_path()
        .and_then(|relative| manifest_version(&root.join(relative.trim_start_matches("./"))));
    let version = usable(plugin.version.as_deref())
        .or(local_version)
        .or_else(|| {
            git_ref(plugin)
                .filter(|value| is_version_ref(value))
                .and_then(|git_ref| usable(Some(&git_ref)))
        })
        .filter(|value| !is_sha(value))
        .unwrap_or_default();
    Hint {
        version,
        pin: pin(plugin),
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

fn remote_plugin_version(pin: &Pin, fetch: &Fetch<'_>) -> Option<String> {
    github_manifest_urls(&pin.url, &pin.path, &pin.rev)
        .into_iter()
        .find_map(|manifest_url| parse_manifest_text(fetch.text(&manifest_url).as_deref()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Transient;

fn fetch_text(url: &str) -> Result<Option<String>, Transient> {
    #[cfg(test)]
    if let Some(result) = test_fetch_text(url) {
        return result;
    }
    match crate::http::get_text(url, Duration::from_secs(2)) {
        Ok(text) => Ok(Some(text)),
        Err(crate::http::HttpError::Status(404)) => Ok(None),
        Err(_) => Err(Transient),
    }
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

fn merge_marketplace_versions(hints: &mut HashMap<String, Hint>, file: MarketplaceFile) {
    for plugin in file.plugins {
        let Some(version) = usable(plugin.version.as_deref()).filter(|value| !is_sha(value)) else {
            continue;
        };
        hints
            .entry(plugin.name)
            .and_modify(|hint| hint.version = version.clone())
            .or_insert(Hint { version, pin: None });
    }
}

fn fetch_remote_marketplace(
    owner: &str,
    repo: &str,
    branch: &str,
    fetch: &Fetch<'_>,
) -> Option<MarketplaceFile> {
    for relative in MARKETPLACE_MANIFESTS {
        let url = format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{relative}");
        let Some(text) = fetch.text(&url) else {
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

fn git_ref(plugin: &MarketplacePlugin) -> Option<String> {
    match &plugin.source {
        Some(MarketplaceSource::Object { git_ref, .. }) => git_ref.clone(),
        _ => None,
    }
}

fn pin(plugin: &MarketplacePlugin) -> Option<Pin> {
    let Some(MarketplaceSource::Object {
        url,
        path,
        sha,
        git_ref,
        ..
    }) = &plugin.source
    else {
        return None;
    };
    let url = url.as_deref().unwrap_or("").trim();
    let rev = usable_sha(sha.as_deref())
        .or_else(|| git_ref.clone().filter(|value| is_version_ref(value)))
        .unwrap_or_default();
    if url.is_empty() || rev.is_empty() {
        return None;
    }
    Some(Pin {
        url: url.to_string(),
        path: path
            .as_deref()
            .unwrap_or("")
            .trim()
            .trim_start_matches("./")
            .to_string(),
        rev,
    })
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
type TextFetch = fn(&str) -> Result<Option<String>, Transient>;

#[cfg(test)]
thread_local! {
    static TEST_FETCH: std::cell::RefCell<Option<TextFetch>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_fetch_text(url: &str) -> Option<Result<Option<String>, Transient>> {
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
