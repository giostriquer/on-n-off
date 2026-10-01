mod scan_cache;
mod source_index;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use crate::paths::{claude_root_for, codex_root_for};

use super::history::Watermark;
use super::reader::MTIME_SLACK_MS;
use super::transcripts::{richest_copies, UsageProvider, UsageRecord};
use scan_cache::{load_scan_cache, prune_and_save, scan_cache_path_for, ScanCache};
use source_index::{
    inventory_sources, prepare_sources, reconcile_inventory, source_index_path_for,
    unchanged_snapshot, PreparedSourceFile, SourceRoot, SourceSnapshot,
};

#[cfg(test)]
pub(crate) use scan_cache::{
    reset_scan_cache_decode_count, scan_cache_decode_count, USAGE_SCAN_CACHE_VERSION,
};
#[cfg(test)]
pub(crate) use source_index::{
    reset_transcript_parse_count, transcript_parse_count, with_live_transcript,
    USAGE_SOURCE_INDEX_VERSION,
};

pub struct UsageFilesLock {
    _guard: MutexGuard<'static, ()>,
}

pub fn lock_usage_files() -> UsageFilesLock {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    UsageFilesLock {
        _guard: LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    }
}

pub struct Sources {
    lock: UsageFilesLock,
    scan_cache_path: PathBuf,
    scan_cache: Option<ScanCache>,
    scan_cache_dirty: bool,
    watermark: Option<Watermark>,
    seen: SeenSources,
}

impl Sources {
    pub fn open(lock: UsageFilesLock, home: &Path, watermark: impl FnOnce() -> Watermark) -> Self {
        let scan_cache_path = scan_cache_path_for(home);
        let source_index = source_index_path_for(home);
        let source_roots = source_roots_for(home);
        let inventory = inventory_sources(&all_roots(&source_roots));
        let (snapshot, scan_cache, scan_cache_dirty, watermark) =
            match unchanged_snapshot(&source_index, &inventory) {
                Some(snapshot) => (snapshot, None, false, None),
                None => {
                    let watermark = watermark();
                    let mut scan_cache = load_scan_cache(&scan_cache_path);
                    let reconciled =
                        reconcile_inventory(&source_index, inventory, &mut scan_cache, watermark);
                    (
                        reconciled.snapshot,
                        Some(scan_cache),
                        reconciled.scan_cache_dirty,
                        Some(watermark),
                    )
                }
            };
        Self {
            lock,
            scan_cache_path,
            scan_cache,
            scan_cache_dirty,
            watermark,
            seen: SeenSources {
                source_index,
                source_roots,
                snapshot,
            },
        }
    }

    pub fn is_complete(&self) -> bool {
        self.seen.snapshot.is_complete()
    }

    pub fn signature(&self, start_ms: i64, end_ms: i64) -> String {
        self.seen.snapshot.signature(start_ms, end_ms)
    }

    pub fn walked_every_root(&self) -> bool {
        self.seen.snapshot.walked_every_root()
    }

    pub fn read(&mut self, from_ms: i64, watermark: impl FnOnce() -> Watermark) -> SourceRead {
        let watermark = *self.watermark.get_or_insert_with(watermark);
        let scan_cache = self
            .scan_cache
            .get_or_insert_with(|| load_scan_cache(&self.scan_cache_path));
        let prepared = prepare_sources(
            &self.seen.snapshot,
            scan_cache,
            from_ms.saturating_sub(MTIME_SLACK_MS),
            watermark,
        );
        self.scan_cache_dirty |= prepared.scan_cache_dirty;
        SourceRead {
            files: prepared.files,
            watermark,
            complete: prepared.complete,
            unread_files: prepared.unread_files,
        }
    }

    pub fn finish(self, watermark: impl FnOnce() -> Watermark) -> SeenSources {
        if let Some(mut scan_cache) = self.scan_cache {
            prune_and_save(
                &self.scan_cache_path,
                &mut scan_cache,
                &self.seen.snapshot.prune_options(watermark()),
                self.scan_cache_dirty,
            );
        }
        drop(self.lock);
        self.seen
    }
}

pub struct SourceRead {
    files: Vec<PreparedSourceFile>,
    watermark: Watermark,
    pub complete: bool,
    pub unread_files: Vec<(String, i64)>,
}

impl SourceRead {
    pub fn records(&self) -> Vec<&UsageRecord> {
        richest_copies(self.files.iter().map(|file| file.records.as_slice()))
            .into_iter()
            .filter(|record| !self.watermark.is_folded(record.timestamp_ms))
            .collect()
    }

    pub fn files_read(&self, provider: UsageProvider) -> FilesRead {
        let mut read = FilesRead {
            scanned: 0,
            skipped: 0,
        };
        for file in self.files.iter().filter(|file| file.provider == provider) {
            if file.records.is_empty() {
                read.skipped += 1;
            } else {
                read.scanned += 1;
            }
        }
        read
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FilesRead {
    pub scanned: u64,
    pub skipped: u64,
}

pub struct SeenSources {
    source_index: PathBuf,
    source_roots: [(UsageProvider, Vec<SourceRoot>); 2],
    snapshot: SourceSnapshot,
}

pub struct ProviderSource<'a> {
    pub provider: UsageProvider,
    pub dir: &'a Path,
    pub present: bool,
}

impl SeenSources {
    pub fn providers(&self) -> impl Iterator<Item = ProviderSource<'_>> {
        self.source_roots
            .iter()
            .map(|(provider, roots)| ProviderSource {
                provider: *provider,
                dir: &roots[0].path,
                present: roots.iter().any(|root| self.snapshot.root_is_present(root)),
            })
    }

    pub fn unchanged(&self, _lock: &UsageFilesLock) -> bool {
        self.snapshot
            .persisted_generation_is_current(&self.source_index)
            && self
                .snapshot
                .inventory_is_current(&all_roots(&self.source_roots))
    }
}

fn source_roots_for(home: &Path) -> [(UsageProvider, Vec<SourceRoot>); 2] {
    [
        (
            UsageProvider::Claude,
            vec![resolve_claude_transcript_dir(home)],
        ),
        (
            UsageProvider::Codex,
            vec![
                resolve_codex_transcript_dir(home),
                resolve_codex_archive_dir(home),
            ],
        ),
    ]
    .map(|(provider, paths)| {
        let roots: Vec<SourceRoot> = paths
            .into_iter()
            .map(|path| SourceRoot { provider, path })
            .collect();
        (provider, roots)
    })
}

fn all_roots(source_roots: &[(UsageProvider, Vec<SourceRoot>)]) -> Vec<SourceRoot> {
    source_roots
        .iter()
        .flat_map(|(_, roots)| roots.iter().cloned())
        .collect()
}

fn resolve_claude_transcript_dir(home: &Path) -> PathBuf {
    let nested = claude_root_for(home).join("projects");
    if nested.is_dir() {
        return nested;
    }
    let flat = home.join("projects");
    if flat.is_dir() {
        return flat;
    }
    nested
}

fn resolve_codex_transcript_dir(home: &Path) -> PathBuf {
    codex_root_for(home).join("sessions")
}

fn resolve_codex_archive_dir(home: &Path) -> PathBuf {
    codex_root_for(home).join("archived_sessions")
}

#[cfg(all(test, unix))]
pub(crate) fn scan_cache_file(home: &Path) -> PathBuf {
    scan_cache_path_for(home)
}

#[cfg(test)]
pub(crate) fn source_index_file(home: &Path) -> PathBuf {
    source_index_path_for(home)
}

#[cfg(test)]
pub(crate) fn cached_record_count(home: &Path, transcript: &Path) -> Option<usize> {
    load_scan_cache(&scan_cache_path_for(home))
        .get(&source_index::normalize_path(transcript))
        .map(|cached| cached.records.len())
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
