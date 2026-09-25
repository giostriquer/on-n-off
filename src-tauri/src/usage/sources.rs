//! Transcript sources: the transcripts every provider wrote, which the Usage summary counts and the
//! background fold keeps (`summary`, `folding`), read the same way for both.
//!
//! [`Sources`] works in two phases under the usage-files lock. [`Sources::open`] walks every root
//! and brings the source index up to date (`source_index`), which is enough to answer
//! [`Sources::signature`] and [`Sources::is_complete`] before any transcript is read for its
//! records: the summary checks its cache there. [`Sources::read`] then reads the records, from the
//! scan cache's parse of each transcript where it still holds (`scan_cache`). [`Sources::finish`]
//! saves what the reads parsed, prunes the scan cache with the watermark it is given, and releases
//! the lock.
//!
//! Both phases leave out transcripts whose records the usage history already holds (`history`):
//! below the watermark, usage counts only from the folded rows.

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

/// Held while the usage caches and the history are read or written: every read and write of them
/// within this process takes it.
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

/// The transcript sources under the usage-files lock; see the module documentation for the two
/// phases. Every path out of a read ends in [`Sources::finish`], or what it parsed is lost.
pub struct Sources {
    lock: UsageFilesLock,
    scan_cache_path: PathBuf,
    /// Loaded only once a transcript's parse is needed: an unchanged index answers without it.
    scan_cache: Option<ScanCache>,
    scan_cache_dirty: bool,
    /// The watermark the sources were read with, once one was asked for.
    watermark: Option<Watermark>,
    seen: SeenSources,
}

impl Sources {
    /// Walks every root and brings the source index up to date, holding `lock` until
    /// [`Sources::finish`]. `watermark` is asked for only when a transcript changed since the
    /// index was written: a transcript whose records the history holds is indexed unread.
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

    /// Every root was walked and every transcript's record bounds are known: nothing can be
    /// missing from [`Sources::signature`].
    pub fn is_complete(&self) -> bool {
        self.seen.snapshot.is_complete()
    }

    /// Changes whenever a transcript that may hold a record in `[start_ms, end_ms)` does.
    pub fn signature(&self, start_ms: i64, end_ms: i64) -> String {
        self.seen.snapshot.signature(start_ms, end_ms)
    }

    /// Every root was walked to the end this time: no transcript can be missing from a read.
    pub fn walked_every_root(&self) -> bool {
        self.seen.snapshot.walked_every_root()
    }

    /// The records of every transcript that may hold one from `from_ms` on, except those holding
    /// only records the history already has. A transcript last written [`MTIME_SLACK_MS`] before
    /// `from_ms` holds none. `watermark` is asked for only when [`Sources::open`] did not ask.
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

    /// Saves the scan cache when the reads changed it, after dropping what it no longer needs:
    /// transcripts deleted, outside every root, or whose records `watermark` says the history
    /// holds. Then releases the lock. `watermark` is asked for only when the scan cache was loaded.
    pub fn finish(self, watermark: impl FnOnce() -> Watermark) -> SeenSources {
        if let Some(mut scan_cache) = self.scan_cache {
            prune_and_save(
                &self.scan_cache_path,
                &mut scan_cache,
                &self.seen.snapshot,
                &all_roots(&self.seen.source_roots),
                watermark(),
                self.scan_cache_dirty,
            );
        }
        drop(self.lock);
        self.seen
    }
}

/// What [`Sources::read`] found.
pub struct SourceRead {
    files: Vec<PreparedSourceFile>,
    watermark: Watermark,
    /// The sources were complete ([`Sources::is_complete`]) and every transcript was read to a parse
    /// that is final: none was still being written, changed since the walk, or failed to read.
    pub complete: bool,
    /// The paths and mtimes of transcripts no parse succeeded on and no cached parse stands in
    /// for: their records are unknown.
    pub unread_files: Vec<(String, i64)>,
}

impl SourceRead {
    /// One record per group of copies, its richest copy (`transcripts::richest_copies`), from the
    /// watermark on. Copies collapse before the watermark splits them, so a message whose copies
    /// straddle it counts once, whole, or not at all.
    pub fn records(&self) -> Vec<&UsageRecord> {
        richest_copies(self.files.iter().map(|file| file.records.as_slice()))
            .into_iter()
            .filter(|record| !self.watermark.is_folded(record.timestamp_ms))
            .collect()
    }

    /// How many of `provider`'s transcripts were read.
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

/// The transcripts one provider had read.
#[derive(Debug, PartialEq, Eq)]
pub struct FilesRead {
    /// Holding records.
    pub scanned: u64,
    /// Holding none.
    pub skipped: u64,
}

/// The transcript sources as a read saw them, kept once the lock is released.
pub struct SeenSources {
    source_index: PathBuf,
    source_roots: [(UsageProvider, Vec<SourceRoot>); 2],
    snapshot: SourceSnapshot,
}

/// One provider's transcripts as the Usage screen lists them.
pub struct ProviderSource<'a> {
    pub provider: UsageProvider,
    /// Its first root, which it is shown under.
    pub dir: &'a Path,
    /// Any of its roots was there.
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

    /// Nothing changed since the read: the source index on disk is still the one it brought up to
    /// date, and a new walk finds every transcript as it did. Walks every root again.
    pub fn unchanged(&self, _lock: &UsageFilesLock) -> bool {
        self.snapshot
            .persisted_generation_is_current(&self.source_index)
            && self
                .snapshot
                .inventory_is_current(&all_roots(&self.source_roots))
    }
}

/// One source per provider, shown under its first root; a source may scan several roots.
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

/// Every root of every source.
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

/// Where Codex moves a session's rollout when the session is archived, mtime intact. The usage it
/// recorded is still Codex usage, reported under the same source as `sessions/`.
fn resolve_codex_archive_dir(home: &Path) -> PathBuf {
    codex_root_for(home).join("archived_sessions")
}

/// Where the scan cache for `home` is kept, for tests that damage or restore it.
#[cfg(test)]
pub(crate) fn scan_cache_file(home: &Path) -> PathBuf {
    scan_cache_path_for(home)
}

/// Where the source index for `home` is kept, for tests that remove it.
#[cfg(test)]
pub(crate) fn source_index_file(home: &Path) -> PathBuf {
    source_index_path_for(home)
}

/// How many records the scan cache on disk holds for `transcript`: `None` when it holds no parse
/// of it.
#[cfg(test)]
pub(crate) fn cached_record_count(home: &Path, transcript: &Path) -> Option<usize> {
    load_scan_cache(&scan_cache_path_for(home))
        .get(&source_index::normalize_path(transcript))
        .map(|cached| cached.records.len())
}

#[cfg(test)]
mod tests;
