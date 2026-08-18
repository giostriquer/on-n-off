//! Local items: skills and subagents copied out of a GitHub marketplace by on-n-off itself,
//! tracked in `~/.on-n-off/installed-items.json` so upstream changes stay visible even after
//! the user edits their copy.
//!
//! The provider CLIs are never involved; `AgentAdapter::item_roots` only tells us where each
//! provider keeps user skills (and, for Claude, subagents).

pub mod fetch;
pub mod manifest;
pub mod registry;
pub mod write;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use fetch::{Fetcher, HttpFetcher, Tarball};
use registry::{InstalledItem, InstalledItemsFile};

use crate::backup::BackupStore;
use crate::dto::{
    AdapterError, AgentId, InstallItemsRequest, InstallItemsResultDto, ItemKind, ItemOutcomeDto,
    ItemOutcomeStatus, ItemRoots, ItemScope, ItemStatusDto, ItemUpstream, MarketplaceInspectDto,
    UpdateItemMode,
};

const MAX_MEMO_TARBALLS: usize = 8;

/// A provider + scope the command layer already resolved to on-disk roots.
pub struct ResolvedTarget {
    pub provider: AgentId,
    pub scope: ItemScope,
    pub roots: ItemRoots,
}

type RepoRef = (String, String, String);

#[derive(Default)]
struct Memo {
    /// `(owner, repo, ref)` → the sha that ref pointed at when we last asked.
    shas: HashMap<RepoRef, String>,
    /// `(owner, repo, sha)` → unpacked snapshot.
    tarballs: HashMap<RepoRef, Arc<Tarball>>,
    order: VecDeque<RepoRef>,
}

impl Memo {
    fn remember(&mut self, owner: &str, repo: &str, git_ref: &str, tarball: Arc<Tarball>) {
        let sha = tarball.commit_sha.clone();
        self.shas
            .insert((owner.into(), repo.into(), git_ref.into()), sha.clone());
        let key = (owner.to_string(), repo.to_string(), sha);
        if self.tarballs.insert(key.clone(), tarball).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > MAX_MEMO_TARBALLS {
            if let Some(old) = self.order.pop_front() {
                self.tarballs.remove(&old);
            }
        }
    }
}

pub struct ItemService {
    registry_path: PathBuf,
    backups: BackupStore,
    fetcher: Box<dyn Fetcher>,
    memo: Mutex<Memo>,
    registry_lock: Mutex<()>,
}

impl ItemService {
    pub fn production() -> Result<Self, AdapterError> {
        Ok(Self {
            registry_path: crate::paths::installed_items_path()?,
            backups: BackupStore::new()?,
            fetcher: Box::new(HttpFetcher),
            memo: Mutex::new(Memo::default()),
            registry_lock: Mutex::new(()),
        })
    }

    #[cfg(test)]
    pub fn at(home: PathBuf, fetcher: Box<dyn Fetcher>) -> Self {
        Self {
            registry_path: crate::paths::installed_items_path_for(&home),
            backups: BackupStore::at(home.join(".on-n-off").join("backups")),
            fetcher,
            memo: Mutex::new(Memo::default()),
            registry_lock: Mutex::new(()),
        }
    }

    // -- network memo -----------------------------------------------------------------------

    fn memo(&self) -> MutexGuard<'_, Memo> {
        self.memo
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The snapshot `ref` points at: memoised, otherwise one tarball download.
    fn snapshot(
        &self,
        owner: &str,
        repo: &str,
        git_ref: &str,
    ) -> Result<Arc<Tarball>, AdapterError> {
        {
            let memo = self.memo();
            if let Some(sha) = memo.shas.get(&key(owner, repo, git_ref)) {
                if let Some(tarball) = memo.tarballs.get(&key(owner, repo, sha)) {
                    return Ok(Arc::clone(tarball));
                }
            }
        }
        let tarball = Arc::new(fetch::fetch_tarball(
            self.fetcher.as_ref(),
            owner,
            repo,
            git_ref,
        )?);
        self.memo()
            .remember(owner, repo, git_ref, Arc::clone(&tarball));
        Ok(tarball)
    }

    /// A specific sha when we still have it, else whatever `ref` points at now.
    fn snapshot_at(
        &self,
        owner: &str,
        repo: &str,
        git_ref: &str,
        sha: &str,
    ) -> Result<Arc<Tarball>, AdapterError> {
        if let Some(tarball) = self.memo().tarballs.get(&key(owner, repo, sha)) {
            return Ok(Arc::clone(tarball));
        }
        self.snapshot(owner, repo, git_ref)
    }

    fn upstream_sha(
        &self,
        owner: &str,
        repo: &str,
        git_ref: &str,
        force: bool,
    ) -> Result<String, AdapterError> {
        if !force {
            if let Some(sha) = self.memo().shas.get(&key(owner, repo, git_ref)) {
                return Ok(sha.clone());
            }
        }
        let sha = fetch::upstream_sha(self.fetcher.as_ref(), owner, repo, git_ref)?;
        self.memo()
            .shas
            .insert(key(owner, repo, git_ref), sha.clone());
        Ok(sha)
    }

    // -- registry -----------------------------------------------------------------------------

    fn lock_registry(&self) -> (MutexGuard<'_, ()>, Result<InstalledItemsFile, AdapterError>) {
        let guard = self
            .registry_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let file = registry::load(&self.registry_path);
        (guard, file)
    }

    fn save_registry(&self, file: &InstalledItemsFile) -> Result<(), AdapterError> {
        registry::save(&self.registry_path, file)
    }

    // -- public operations ------------------------------------------------------------------

    pub fn inspect_marketplace(
        &self,
        owner: &str,
        repo: &str,
        git_ref: Option<&str>,
    ) -> Result<MarketplaceInspectDto, AdapterError> {
        let git_ref = git_ref.filter(|r| !r.trim().is_empty()).unwrap_or("HEAD");
        let tarball = self.snapshot(owner, repo, git_ref)?;
        let mut second =
            |o: &str, r: &str, rf: Option<&str>| self.snapshot(o, r, rf.unwrap_or("HEAD"));
        Ok(manifest::inspect(&tarball, repo, &mut second))
    }

    pub fn install_items(
        &self,
        request: InstallItemsRequest,
        targets: Vec<ResolvedTarget>,
    ) -> Result<InstallItemsResultDto, AdapterError> {
        if request.items.is_empty() || targets.is_empty() {
            return Ok(InstallItemsResultDto {
                commit_sha: request.commit_sha,
                sha_moved: false,
                outcomes: Vec::new(),
            });
        }
        let src = &request.source;
        let primary = self.snapshot_at(&src.owner, &src.repo, &src.git_ref, &request.commit_sha)?;
        let sha_moved = primary.commit_sha != request.commit_sha;
        let plugin_roots = plugin_roots(&primary);

        let (_guard, registry) = self.lock_registry();
        let mut registry = registry?;
        let mut outcomes = Vec::new();
        let mut seen: HashMap<(AgentId, ItemKind, String), String> = HashMap::new();
        let mut dirty = false;

        for target in &targets {
            if let ItemScope::Project { project_path } = &target.scope {
                if !Path::new(project_path).is_dir() {
                    for item in &request.items {
                        outcomes.push(outcome(
                            target.provider,
                            item.kind,
                            item_name(&item.path, item.kind),
                            String::new(),
                            ItemOutcomeStatus::Failed,
                            Some(format!("project folder not found: {project_path}")),
                        ));
                    }
                    continue;
                }
            }
            for item in &request.items {
                let name = item_name(&item.path, item.kind);
                let Some(root) = root_for(&target.roots, item.kind) else {
                    outcomes.push(outcome(
                        target.provider,
                        item.kind,
                        name,
                        String::new(),
                        ItemOutcomeStatus::Skipped,
                        Some(format!(
                            "{} does not support subagents",
                            target.provider.display_name()
                        )),
                    ));
                    continue;
                };
                let dest = root.join(dest_name(&name, item.kind));
                let dest_text = dest.display().to_string();
                let dedupe_key = (target.provider, item.kind, name.to_lowercase());
                if let Some(first) = seen.get(&dedupe_key) {
                    outcomes.push(outcome(
                        target.provider,
                        item.kind,
                        name,
                        dest_text,
                        ItemOutcomeStatus::Conflict,
                        Some(format!("also selected from plugin {first}")),
                    ));
                    continue;
                }
                let (source, tarball) = match &item.source {
                    Some(external) => {
                        match self.snapshot(&external.owner, &external.repo, &external.git_ref) {
                            Ok(tarball) => (external.clone(), tarball),
                            Err(error) => {
                                outcomes.push(outcome(
                                    target.provider,
                                    item.kind,
                                    name,
                                    dest_text,
                                    ItemOutcomeStatus::Failed,
                                    Some(error.message),
                                ));
                                continue;
                            }
                        }
                    }
                    None => (src.clone(), Arc::clone(&primary)),
                };
                let files = match write::item_files(&tarball, &item.path, item.kind) {
                    Ok(files) => files,
                    Err(error) => {
                        outcomes.push(outcome(
                            target.provider,
                            item.kind,
                            name,
                            dest_text,
                            ItemOutcomeStatus::Failed,
                            Some(error.message),
                        ));
                        continue;
                    }
                };
                let exists = dest.exists();
                let managed = registry.find_by_target(target.provider, &dest).is_some();
                if exists && !managed && !request.overwrite_unmanaged {
                    outcomes.push(outcome(
                        target.provider,
                        item.kind,
                        name,
                        dest_text,
                        ItemOutcomeStatus::Conflict,
                        Some("already exists and was not installed by on-n-off".to_string()),
                    ));
                    continue;
                }
                let placed = (|| -> Result<(), AdapterError> {
                    if exists {
                        self.backups.backup_item(target.provider, &dest)?;
                    }
                    write::place(&dest, item.kind, &files)
                })();
                if let Err(error) = placed {
                    outcomes.push(outcome(
                        target.provider,
                        item.kind,
                        name,
                        dest_text,
                        ItemOutcomeStatus::Failed,
                        Some(error.message),
                    ));
                    continue;
                }
                let plugin_root = match &item.source {
                    Some(_) => String::new(),
                    None => plugin_roots
                        .get(&item.plugin_name)
                        .cloned()
                        .unwrap_or_default(),
                };
                let plugin_version =
                    manifest::plugin_manifest(&tarball, &plugin_root).and_then(|m| m.version);
                let upstream_path = write::normalize_upstream_path(&item.path)
                    .unwrap_or_else(|_| item.path.clone());
                registry.upsert(InstalledItem {
                    id: registry::item_id(target.provider, item.kind, &dest),
                    provider: target.provider,
                    kind: item.kind,
                    name: name.clone(),
                    target_path: dest_text.clone(),
                    scope: target.scope.clone(),
                    source: registry::ItemSource {
                        owner: source.owner.clone(),
                        repo: source.repo.clone(),
                        git_ref: source.git_ref.clone(),
                        plugin_name: item.plugin_name.clone(),
                        plugin_root,
                        upstream_path,
                    },
                    installed: registry::Installed {
                        commit_sha: tarball.commit_sha.clone(),
                        plugin_version,
                        installed_at: now(),
                    },
                    files: write::hash_files(&files),
                    dismissed_sha: None,
                });
                dirty = true;
                seen.insert(dedupe_key, item.plugin_name.clone());
                outcomes.push(outcome(
                    target.provider,
                    item.kind,
                    name,
                    dest_text,
                    if exists {
                        ItemOutcomeStatus::Replaced
                    } else {
                        ItemOutcomeStatus::Installed
                    },
                    None,
                ));
            }
        }
        if dirty {
            self.save_registry(&registry)?;
        }
        Ok(InstallItemsResultDto {
            commit_sha: primary.commit_sha.clone(),
            sha_moved,
            outcomes,
        })
    }

    pub fn item_update_status(
        &self,
        provider: AgentId,
        project_path: Option<&str>,
        force: bool,
    ) -> Result<Vec<ItemStatusDto>, AdapterError> {
        let scope = match project_path.map(str::trim).filter(|p| !p.is_empty()) {
            Some(path) => ItemScope::Project {
                project_path: path.to_string(),
            },
            None => ItemScope::Global,
        };
        let (_guard, registry) = self.lock_registry();
        let mut registry = registry?;
        let ids: Vec<String> = registry
            .for_provider(provider, &scope)
            .into_iter()
            .map(|item| item.id.clone())
            .collect();
        let mut sha_by_repo: HashMap<RepoRef, Option<String>> = HashMap::new();
        let mut statuses = Vec::new();
        let mut dirty = false;
        for id in ids {
            let Some(item) = registry.items.iter_mut().find(|item| item.id == id) else {
                continue;
            };
            let repo_key = key(&item.source.owner, &item.source.repo, &item.source.git_ref);
            let sha = sha_by_repo
                .entry(repo_key)
                .or_insert_with(|| {
                    self.upstream_sha(
                        &item.source.owner,
                        &item.source.repo,
                        &item.source.git_ref,
                        force,
                    )
                    .ok()
                })
                .clone();
            let (status, changed) = self.status_of(item, sha.as_deref());
            dirty |= changed;
            statuses.push(status);
        }
        if dirty {
            self.save_registry(&registry)?;
        }
        Ok(statuses)
    }

    pub fn update_item(
        &self,
        id: &str,
        mode: UpdateItemMode,
    ) -> Result<ItemStatusDto, AdapterError> {
        let (_guard, registry) = self.lock_registry();
        let mut registry = registry?;
        let item = registry
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| AdapterError::message(format!("item not found: {id}")))?;
        let sha = self.upstream_sha(
            &item.source.owner,
            &item.source.repo,
            &item.source.git_ref,
            false,
        )?;
        match mode {
            UpdateItemMode::Dismiss => {
                item.dismissed_sha = Some(sha.clone());
            }
            UpdateItemMode::Overwrite => {
                let tarball = self.snapshot_at(
                    &item.source.owner,
                    &item.source.repo,
                    &item.source.git_ref,
                    &sha,
                )?;
                let files = write::item_files(&tarball, &item.source.upstream_path, item.kind)?;
                let dest = PathBuf::from(&item.target_path);
                if dest.exists() {
                    self.backups.backup_item(item.provider, &dest)?;
                }
                write::place(&dest, item.kind, &files)?;
                item.installed = registry::Installed {
                    commit_sha: tarball.commit_sha.clone(),
                    plugin_version: manifest::plugin_manifest(&tarball, &item.source.plugin_root)
                        .and_then(|m| m.version),
                    installed_at: now(),
                };
                item.files = write::hash_files(&files);
                item.dismissed_sha = None;
            }
        }
        let (status, _) = self.status_of(item, Some(&sha));
        self.save_registry(&registry)?;
        Ok(status)
    }

    pub fn remove_item(&self, id: &str) -> Result<(), AdapterError> {
        let (_guard, registry) = self.lock_registry();
        let mut registry = registry?;
        let item = registry
            .remove(id)
            .ok_or_else(|| AdapterError::message(format!("item not found: {id}")))?;
        let dest = PathBuf::from(&item.target_path);
        if dest.exists() {
            self.backups.backup_item(item.provider, &dest)?;
            write::remove_any(&dest).map_err(|error| write::io_error(error, &dest))?;
        }
        self.save_registry(&registry)
    }

    /// Builds the status of one item; `upstream_sha` is `None` when the check failed. Returns
    /// whether the registry row was changed (recorded sha advanced past an identical upstream).
    fn status_of(
        &self,
        item: &mut InstalledItem,
        upstream_sha: Option<&str>,
    ) -> (ItemStatusDto, bool) {
        let target = PathBuf::from(&item.target_path);
        let on_disk = write::hash_tree_on_disk(&target, item.kind).ok().flatten();
        let missing = on_disk.is_none();
        let modified = on_disk.as_ref().is_some_and(|hashes| *hashes != item.files);
        let display_name =
            local_display_name(&target, item.kind).unwrap_or_else(|| item.name.clone());
        let mut changed = false;
        let upstream = match upstream_sha {
            None => ItemUpstream::Unknown,
            Some(sha) if sha == item.installed.commit_sha => ItemUpstream::Current,
            Some(sha) if item.dismissed_sha.as_deref() == Some(sha) => ItemUpstream::Current,
            Some(sha) => {
                match self.snapshot_at(
                    &item.source.owner,
                    &item.source.repo,
                    &item.source.git_ref,
                    sha,
                ) {
                    Err(_) => ItemUpstream::Unknown,
                    Ok(tarball) => {
                        match write::item_files(&tarball, &item.source.upstream_path, item.kind) {
                            // Removed upstream: nothing to update to.
                            Err(_) => ItemUpstream::Current,
                            Ok(files) if write::hash_files(&files) == item.files => {
                                item.installed.commit_sha = tarball.commit_sha.clone();
                                changed = true;
                                ItemUpstream::Current
                            }
                            Ok(_) => ItemUpstream::UpdateAvailable {
                                commit_sha: tarball.commit_sha.clone(),
                                plugin_version: manifest::plugin_manifest(
                                    &tarball,
                                    &item.source.plugin_root,
                                )
                                .and_then(|m| m.version),
                            },
                        }
                    }
                }
            }
        };
        let status = ItemStatusDto {
            id: item.id.clone(),
            provider: item.provider,
            kind: item.kind,
            name: item.name.clone(),
            display_name,
            target_path: item.target_path.clone(),
            installed_version: item.installed.plugin_version.clone(),
            installed_sha: item.installed.commit_sha.clone(),
            modified,
            missing,
            upstream,
        };
        (status, changed)
    }
}

fn key(owner: &str, repo: &str, third: &str) -> RepoRef {
    (owner.to_string(), repo.to_string(), third.to_string())
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn outcome(
    provider: AgentId,
    kind: ItemKind,
    name: String,
    target_path: String,
    status: ItemOutcomeStatus,
    reason: Option<String>,
) -> ItemOutcomeDto {
    ItemOutcomeDto {
        provider,
        kind,
        name,
        target_path,
        status,
        reason,
    }
}

fn root_for(roots: &ItemRoots, kind: ItemKind) -> Option<&Path> {
    match kind {
        ItemKind::Skill => Some(roots.skills.as_path()),
        ItemKind::Agent => roots.agents.as_deref(),
    }
}

/// `skills/engineering/tdd` → `tdd`; `agents/reviewer.md` → `reviewer`.
fn item_name(upstream_path: &str, kind: ItemKind) -> String {
    let normalized = write::normalize_upstream_path(upstream_path).unwrap_or_default();
    let last = normalized.rsplit('/').next().unwrap_or("").to_string();
    match kind {
        ItemKind::Skill => last,
        ItemKind::Agent => last.strip_suffix(".md").unwrap_or(&last).to_string(),
    }
}

fn dest_name(name: &str, kind: ItemKind) -> String {
    match kind {
        ItemKind::Skill => name.to_string(),
        ItemKind::Agent => format!("{name}.md"),
    }
}

/// Plugin name → plugin folder inside the marketplace repository (`""` = root).
fn plugin_roots(tarball: &Tarball) -> HashMap<String, String> {
    let manifest = crate::plugin_meta::MARKETPLACE_MANIFESTS
        .iter()
        .find_map(|relative| tarball.text(relative))
        .and_then(|text| crate::plugin_meta::parse_marketplace_text(&text));
    let mut roots = HashMap::new();
    if let Some(manifest) = manifest {
        for plugin in &manifest.plugins {
            if let Some(local) = plugin.local_source_path() {
                if let Ok(root) = write::normalize_upstream_path(local) {
                    roots.insert(plugin.name.clone(), root);
                }
            }
        }
    }
    roots
}

fn local_display_name(target: &Path, kind: ItemKind) -> Option<String> {
    let file = match kind {
        ItemKind::Skill => target.join("SKILL.md"),
        ItemKind::Agent => target.to_path_buf(),
    };
    let contents = std::fs::read_to_string(&file).ok()?;
    let fallback = match kind {
        ItemKind::Skill => target.file_name()?.to_string_lossy().into_owned(),
        ItemKind::Agent => target.file_stem()?.to_string_lossy().into_owned(),
    };
    Some(crate::scanner::parse_frontmatter(&contents, &fallback).0)
}
