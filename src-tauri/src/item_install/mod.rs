//! Local items: skills and subagents copied out of a GitHub marketplace by on-n-off itself,
//! tracked in `~/.on-n-off/installed-items.json` so upstream changes stay visible even after
//! the user edits their copy.
//!
//! The provider CLIs are never involved; `AgentAdapter::item_roots` only tells us where each
//! provider keeps user skills (and, for Claude, subagents).

pub mod deps;
pub mod fetch;
pub mod manifest;
pub mod registry;
pub mod write;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use deps::SiblingIndex;
use fetch::{Fetcher, HttpFetcher, Tarball};
use registry::{InstalledItem, InstalledItemsFile};

use crate::adapter::ItemRoots;
use crate::backup::BackupStore;
use crate::dto::{
    AdapterError, AgentId, DepConfidence, InstallItemsRequest, InstallItemsResultDto, ItemKind,
    ItemOutcomeDto, ItemOutcomeStatus, ItemPick, ItemScope, ItemSourceDto, ItemStatusDto,
    ItemTarget, ItemUpstream, MarketplaceInspectDto, UpdateItemMode,
};

const MAX_MEMO_TARBALLS: usize = 8;

/// Resolves a requested target to on-disk roots; the command layer supplies the adapters.
pub type ResolveRoots<'a> = &'a dyn Fn(&ItemTarget) -> Result<ItemRoots, AdapterError>;

type RepoRef = (String, String, String);

#[derive(Default)]
struct Memo {
    /// `(owner, repo, ref)` -> the sha that ref pointed at when we last asked.
    shas: HashMap<RepoRef, String>,
    /// `(owner, repo, sha)` -> unpacked snapshot.
    tarballs: HashMap<RepoRef, Arc<Tarball>>,
    order: VecDeque<RepoRef>,
}

impl Memo {
    fn remember(&mut self, owner: &str, repo: &str, git_ref: &str, tarball: Arc<Tarball>) {
        let sha = tarball.commit_sha.clone();
        self.shas.insert(key(owner, repo, git_ref), sha.clone());
        let entry = key(owner, repo, &sha);
        if self.tarballs.insert(entry.clone(), tarball).is_none() {
            self.order.push_back(entry);
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

    /// The snapshot `ref` points at according to the memo, without any network.
    fn memoized(&self, owner: &str, repo: &str, git_ref: &str) -> Option<Arc<Tarball>> {
        let memo = self.memo();
        let sha = memo.shas.get(&key(owner, repo, git_ref))?;
        memo.tarballs.get(&key(owner, repo, sha)).map(Arc::clone)
    }

    /// The snapshot `ref` points at according to the memo, otherwise one tarball download.
    fn snapshot(
        &self,
        owner: &str,
        repo: &str,
        git_ref: &str,
    ) -> Result<Arc<Tarball>, AdapterError> {
        if let Some(tarball) = self.memoized(owner, repo, git_ref) {
            return Ok(tarball);
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

    fn load_registry(&self) -> Result<InstalledItemsFile, AdapterError> {
        registry::load(&self.registry_path)
    }

    /// Locks, loads, runs `edit`, and saves when it reports a change. Never holds the lock
    /// around network work: callers fetch what they need first.
    fn with_registry<T>(
        &self,
        edit: impl FnOnce(&mut InstalledItemsFile) -> Result<(T, bool), AdapterError>,
    ) -> Result<T, AdapterError> {
        let _guard = self
            .registry_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut file = self.load_registry()?;
        let (value, dirty) = edit(&mut file)?;
        if dirty {
            registry::save(&self.registry_path, &file)?;
        }
        Ok(value)
    }

    // -- public operations ------------------------------------------------------------------

    pub fn inspect_marketplace(
        &self,
        owner: &str,
        repo: &str,
        git_ref: Option<&str>,
    ) -> Result<MarketplaceInspectDto, AdapterError> {
        let git_ref = git_ref.filter(|r| !r.trim().is_empty()).unwrap_or("HEAD");
        // One cheap sha request keeps a re-opened sheet from installing a stale memoised
        // snapshot; if that request fails the tarball fetch (or memo) still decides.
        let tarball = match self.upstream_sha(owner, repo, git_ref, true) {
            Ok(sha) => self.snapshot_at(owner, repo, git_ref, &sha)?,
            Err(_) => self.snapshot(owner, repo, git_ref)?,
        };
        let mut second =
            |o: &str, r: &str, rf: Option<&str>| self.snapshot(o, r, rf.unwrap_or("HEAD"));
        Ok(manifest::inspect(&tarball, repo, &mut second))
    }

    pub fn install_items(
        &self,
        request: InstallItemsRequest,
        resolve: ResolveRoots<'_>,
    ) -> Result<InstallItemsResultDto, AdapterError> {
        if request.items.is_empty() || request.targets.is_empty() {
            return Ok(InstallItemsResultDto {
                commit_sha: request.commit_sha,
                sha_moved: false,
                outcomes: Vec::new(),
            });
        }
        let src = &request.source;
        let primary = self.snapshot_at(&src.owner, &src.repo, &src.git_ref, &request.commit_sha)?;
        let sha_moved = primary.commit_sha != request.commit_sha;
        let plugin_roots = manifest::plugin_roots(&primary);
        // Fetch every plugin-specific repository before taking the registry lock.
        let mut snapshots: HashMap<ItemSourceDto, Result<Arc<Tarball>, AdapterError>> =
            HashMap::new();
        for pick in &request.items {
            if let Some(external) = &pick.source {
                snapshots.entry(external.clone()).or_insert_with(|| {
                    self.snapshot(&external.owner, &external.repo, &external.git_ref)
                });
            }
        }
        // Dependencies are recorded from what is already in memory: the marketplace listing
        // and the plugin repositories fetched above (or memoised by the inspect step). Install
        // never downloads a repository just to describe an unpicked plugin.
        let mut memo_only = |o: &str, r: &str, rf: Option<&str>| {
            self.memoized(o, r, rf.unwrap_or("HEAD"))
                .ok_or_else(|| AdapterError::message("plugin repository not fetched"))
        };
        let listing = manifest::inspect_entries(&primary, &src.repo, &mut memo_only);
        let siblings = SiblingIndex::from_inspect(&listing.dto);
        let batch = Batch {
            request: &request,
            primary: &primary,
            plugin_roots: &plugin_roots,
            snapshots: &snapshots,
            siblings: &siblings,
        };

        let outcomes = self.with_registry(|registry| {
            let mut outcomes = Vec::new();
            let mut seen: HashMap<(AgentId, ItemKind, String), String> = HashMap::new();
            let mut dirty = false;
            for target in &request.targets {
                let roots = match target_roots(target, resolve) {
                    Ok(roots) => roots,
                    Err(error) => {
                        for pick in &request.items {
                            outcomes.push(failure(target, pick, error.message.clone()));
                        }
                        continue;
                    }
                };
                for pick in &request.items {
                    let placed =
                        self.install_one(&batch, registry, target, &roots, pick, &mut seen);
                    dirty |= matches!(
                        placed.status,
                        ItemOutcomeStatus::Installed | ItemOutcomeStatus::Replaced
                    );
                    outcomes.push(placed);
                }
            }
            Ok((outcomes, dirty))
        })?;
        Ok(InstallItemsResultDto {
            commit_sha: primary.commit_sha.clone(),
            sha_moved,
            outcomes,
        })
    }

    /// One (target, pick) placement; every early exit is an outcome, never an error.
    fn install_one(
        &self,
        batch: &Batch<'_>,
        registry: &mut InstalledItemsFile,
        target: &ItemTarget,
        roots: &ItemRoots,
        pick: &ItemPick,
        seen: &mut HashMap<(AgentId, ItemKind, String), String>,
    ) -> ItemOutcomeDto {
        let name = item_name(&pick.path, pick.kind);
        let done =
            |status: ItemOutcomeStatus, dest: &Path, reason: Option<String>| ItemOutcomeDto {
                provider: target.provider,
                kind: pick.kind,
                name: name.clone(),
                plugin_name: pick.plugin_name.clone(),
                path: pick.path.clone(),
                target_path: dest.display().to_string(),
                status,
                reason,
            };
        let Some(root) = root_for(roots, pick.kind) else {
            return done(
                ItemOutcomeStatus::Skipped,
                Path::new(""),
                Some(format!(
                    "{} does not support subagents",
                    target.provider.display_name()
                )),
            );
        };
        let dest = root.join(dest_name(&name, pick.kind));
        let dedupe_key = (target.provider, pick.kind, name.to_lowercase());
        if let Some(first) = seen.get(&dedupe_key) {
            return done(
                ItemOutcomeStatus::Skipped,
                &dest,
                Some(format!("also selected from plugin {first}")),
            );
        }
        let (source, tarball) = match &pick.source {
            Some(external) => match batch.snapshots.get(external) {
                Some(Ok(tarball)) => (external.clone(), Arc::clone(tarball)),
                Some(Err(error)) => {
                    return done(
                        ItemOutcomeStatus::Failed,
                        &dest,
                        Some(error.message.clone()),
                    )
                }
                None => {
                    return done(
                        ItemOutcomeStatus::Failed,
                        &dest,
                        Some("plugin repository was not fetched".into()),
                    )
                }
            },
            None => (batch.request.source.clone(), Arc::clone(batch.primary)),
        };
        let files = match write::item_files(&tarball, &pick.path, pick.kind) {
            Ok(files) => files,
            Err(error) => return done(ItemOutcomeStatus::Failed, &dest, Some(error.message)),
        };
        let exists = dest.exists();
        let managed = registry.find_by_target(target.provider, &dest).is_some();
        if exists && !managed && !batch.request.overwrite_unmanaged {
            return done(
                ItemOutcomeStatus::Conflict,
                &dest,
                Some("already exists and was not installed by on-n-off".into()),
            );
        }
        let placed = (|| -> Result<(), AdapterError> {
            if exists {
                self.backups.backup_item(target.provider, &dest)?;
            }
            write::place(&dest, pick.kind, &files)
        })();
        if let Err(error) = placed {
            return done(ItemOutcomeStatus::Failed, &dest, Some(error.message));
        }
        let plugin_root = plugin_root_of(batch, pick);
        let upstream_path = write::normalize_upstream_path(&pick.path).unwrap_or_default();
        let depends_on = batch
            .siblings
            .find(&pick.plugin_name, pick.kind, &upstream_path)
            .map(|me| {
                deps::detect(&files, me, batch.siblings)
                    .depends_on
                    .into_iter()
                    .filter(|dep| dep.confidence == DepConfidence::High)
                    .map(|dep| registry::dependency_key(&dep.plugin_name, dep.kind, &dep.path))
                    .collect()
            })
            .unwrap_or_default();
        registry.upsert(InstalledItem {
            id: registry::item_id(target.provider, pick.kind, &dest),
            provider: target.provider,
            kind: pick.kind,
            name: name.clone(),
            target_path: dest.display().to_string(),
            scope: target.scope.clone(),
            source: registry::ItemSource {
                owner: source.owner,
                repo: source.repo,
                git_ref: source.git_ref,
                plugin_name: pick.plugin_name.clone(),
                plugin_root: plugin_root.clone(),
                upstream_path,
                depends_on,
            },
            installed: registry::Installed {
                commit_sha: tarball.commit_sha.clone(),
                plugin_version: manifest::plugin_manifest(&tarball, &plugin_root)
                    .and_then(|m| m.version),
                installed_at: now(),
            },
            files: write::hash_files(&files),
            dismissed_sha: None,
        });
        seen.insert(dedupe_key, pick.plugin_name.clone());
        done(
            if exists {
                ItemOutcomeStatus::Replaced
            } else {
                ItemOutcomeStatus::Installed
            },
            &dest,
            None,
        )
    }

    /// Read-only: never writes the registry, and takes no lock around the network work.
    pub fn item_update_status(
        &self,
        provider: AgentId,
        project_path: Option<&str>,
        force: bool,
    ) -> Result<Vec<ItemStatusDto>, AdapterError> {
        let scope = scope_for(project_path);
        let registry = self.load_registry()?;
        let items: Vec<&InstalledItem> = registry.for_provider(provider, &scope);
        // A forced check asks GitHub once per repository; a repository that cannot be reached
        // reports `Unknown` rather than falling back to a memoised answer.
        let mut unreachable: HashSet<RepoRef> = HashSet::new();
        if force {
            let repos: HashSet<RepoRef> = items.iter().map(|item| repo_key(&item.source)).collect();
            for (owner, repo, git_ref) in repos {
                if self.upstream_sha(&owner, &repo, &git_ref, true).is_err() {
                    unreachable.insert(key(&owner, &repo, &git_ref));
                }
            }
        }
        Ok(items
            .into_iter()
            .map(|item| {
                let sha = if unreachable.contains(&repo_key(&item.source)) {
                    None
                } else {
                    self.upstream_sha(
                        &item.source.owner,
                        &item.source.repo,
                        &item.source.git_ref,
                        false,
                    )
                    .ok()
                };
                self.status_of(item, sha.as_deref())
            })
            .collect())
    }

    pub fn update_item(
        &self,
        id: &str,
        mode: UpdateItemMode,
    ) -> Result<ItemStatusDto, AdapterError> {
        let item = self
            .load_registry()?
            .remove(id)
            .ok_or_else(|| AdapterError::message(format!("item not found: {id}")))?;
        let sha = self.upstream_sha(
            &item.source.owner,
            &item.source.repo,
            &item.source.git_ref,
            false,
        )?;
        // Everything network-bound happens before the registry lock.
        let replacement = match mode {
            UpdateItemMode::Dismiss => None,
            UpdateItemMode::Overwrite => {
                let tarball = self.snapshot_at(
                    &item.source.owner,
                    &item.source.repo,
                    &item.source.git_ref,
                    &sha,
                )?;
                let files = write::item_files(&tarball, &item.source.upstream_path, item.kind)?;
                Some((tarball, files))
            }
        };
        self.with_registry(|registry| {
            let item = registry
                .items
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| AdapterError::message(format!("item not found: {id}")))?;
            match replacement {
                None => item.dismissed_sha = Some(sha.clone()),
                Some((tarball, files)) => {
                    let dest = PathBuf::from(&item.target_path);
                    if dest.exists() {
                        self.backups.backup_item(item.provider, &dest)?;
                    }
                    write::place(&dest, item.kind, &files)?;
                    item.installed = registry::Installed {
                        commit_sha: tarball.commit_sha.clone(),
                        plugin_version: manifest::plugin_manifest(
                            &tarball,
                            &item.source.plugin_root,
                        )
                        .and_then(|m| m.version),
                        installed_at: now(),
                    };
                    item.files = write::hash_files(&files);
                    item.dismissed_sha = None;
                }
            }
            Ok((self.status_of(item, Some(&sha)), true))
        })
    }

    pub fn remove_item(&self, id: &str) -> Result<(), AdapterError> {
        self.with_registry(|registry| {
            let item = registry
                .remove(id)
                .ok_or_else(|| AdapterError::message(format!("item not found: {id}")))?;
            let dest = PathBuf::from(&item.target_path);
            if dest.exists() {
                self.backups.backup_item(item.provider, &dest)?;
                write::remove_any(&dest).map_err(|error| write::io_error(error, &dest))?;
            }
            Ok(((), true))
        })
    }

    /// The status of one item; `upstream_sha` is `None` when the check failed.
    fn status_of(&self, item: &InstalledItem, upstream_sha: Option<&str>) -> ItemStatusDto {
        let target = PathBuf::from(&item.target_path);
        // An unreadable folder is neither "missing" nor "modified": leave both flags off.
        let (missing, modified) = match write::hash_tree_on_disk(&target, item.kind) {
            Ok(None) => (true, false),
            Ok(Some(hashes)) => (false, hashes != item.files),
            Err(_) => (false, false),
        };
        let display_name =
            local_display_name(&target, item.kind).unwrap_or_else(|| item.name.clone());
        let upstream = match upstream_sha {
            None => ItemUpstream::Unknown,
            Some(sha) if sha == item.installed.commit_sha => ItemUpstream::Current,
            Some(sha) if item.dismissed_sha.as_deref() == Some(sha) => ItemUpstream::Current,
            Some(sha) => self.compare_upstream(item, sha),
        };
        ItemStatusDto {
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
            source: ItemSourceDto {
                owner: item.source.owner.clone(),
                repo: item.source.repo.clone(),
                git_ref: item.source.git_ref.clone(),
            },
            plugin_name: item.source.plugin_name.clone(),
            upstream_path: item.source.upstream_path.clone(),
            upstream_url: upstream_url(item),
        }
    }

    /// Upstream moved past the installed sha: does this item's content actually differ?
    fn compare_upstream(&self, item: &InstalledItem, sha: &str) -> ItemUpstream {
        let Ok(tarball) = self.snapshot_at(
            &item.source.owner,
            &item.source.repo,
            &item.source.git_ref,
            sha,
        ) else {
            return ItemUpstream::Unknown;
        };
        match write::item_files(&tarball, &item.source.upstream_path, item.kind) {
            // Removed upstream: nothing to update to.
            Err(_) => ItemUpstream::Current,
            Ok(files) if write::hash_files(&files) == item.files => ItemUpstream::Current,
            Ok(_) => ItemUpstream::UpdateAvailable {
                commit_sha: tarball.commit_sha.clone(),
                plugin_version: manifest::plugin_manifest(&tarball, &item.source.plugin_root)
                    .and_then(|m| m.version),
            },
        }
    }
}

/// Everything `install_one` needs that was fetched before the registry lock.
struct Batch<'a> {
    request: &'a InstallItemsRequest,
    primary: &'a Arc<Tarball>,
    plugin_roots: &'a HashMap<String, String>,
    snapshots: &'a HashMap<ItemSourceDto, Result<Arc<Tarball>, AdapterError>>,
    siblings: &'a SiblingIndex,
}

fn plugin_root_of(batch: &Batch<'_>, pick: &ItemPick) -> String {
    match &pick.source {
        Some(_) => String::new(),
        None => batch
            .plugin_roots
            .get(&pick.plugin_name)
            .cloned()
            .unwrap_or_default(),
    }
}

fn target_roots(target: &ItemTarget, resolve: ResolveRoots<'_>) -> Result<ItemRoots, AdapterError> {
    if let ItemScope::Project { project_path } = &target.scope {
        if !Path::new(project_path).is_dir() {
            return Err(AdapterError::message(format!(
                "project folder not found: {project_path}"
            )));
        }
    }
    resolve(target)
}

fn failure(target: &ItemTarget, pick: &ItemPick, reason: String) -> ItemOutcomeDto {
    ItemOutcomeDto {
        provider: target.provider,
        kind: pick.kind,
        name: item_name(&pick.path, pick.kind),
        plugin_name: pick.plugin_name.clone(),
        path: pick.path.clone(),
        target_path: String::new(),
        status: ItemOutcomeStatus::Failed,
        reason: Some(reason),
    }
}

/// GitHub page of an installed item at the commit it was copied from.
pub fn upstream_url(item: &InstalledItem) -> String {
    let view = match item.kind {
        ItemKind::Skill => "tree",
        ItemKind::Agent => "blob",
    };
    format!(
        "https://github.com/{}/{}/{view}/{}/{}",
        item.source.owner, item.source.repo, item.installed.commit_sha, item.source.upstream_path
    )
}

/// The only links the app opens in the browser: pages on github.com over HTTPS.
pub fn is_openable_url(url: &str) -> bool {
    url.starts_with("https://github.com/")
}

fn scope_for(project_path: Option<&str>) -> ItemScope {
    match project_path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(path) => ItemScope::Project {
            project_path: path.to_string(),
        },
        None => ItemScope::Global,
    }
}

fn key(owner: &str, repo: &str, third: &str) -> RepoRef {
    (owner.to_string(), repo.to_string(), third.to_string())
}

fn repo_key(source: &registry::ItemSource) -> RepoRef {
    key(&source.owner, &source.repo, &source.git_ref)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn root_for(roots: &ItemRoots, kind: ItemKind) -> Option<&Path> {
    match kind {
        ItemKind::Skill => Some(roots.skills.as_path()),
        ItemKind::Agent => roots.agents.as_deref(),
    }
}

/// `skills/engineering/tdd` -> `tdd`; `agents/reviewer.md` -> `reviewer`.
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
