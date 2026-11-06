mod scan_cache;
mod source_index;

use std::path::{Path, PathBuf};

use super::history::Watermark;
use super::{Locked, ProviderRead};
use crate::provider_catalogue::UsageProvider;
use crate::usage::transcripts::{richest_copies, transcript_dirs, TranscriptDirs, UsageRecord};
use crate::usage::window::Span;
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

pub struct Sources<'l> {
    _locked: &'l Locked,
    scan_cache_path: PathBuf,
    scan_cache: Option<ScanCache>,
    scan_cache_dirty: bool,
    watermark: Option<Watermark>,
    seen: SeenSources,
}

impl<'l> Sources<'l> {
    pub fn open(locked: &'l Locked, home: &Path, watermark: impl FnOnce() -> Watermark) -> Self {
        let scan_cache_path = scan_cache_path_for(home);
        let source_index = source_index_path_for(home);
        let dirs = dirs_for(home);
        let inventory = inventory_sources(&all_roots(&dirs));
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
            _locked: locked,
            scan_cache_path,
            scan_cache,
            scan_cache_dirty,
            watermark,
            seen: SeenSources {
                source_index,
                dirs,
                snapshot,
            },
        }
    }

    pub fn is_complete(&self) -> bool {
        self.seen.snapshot.is_complete()
    }

    pub fn signature(&self, span: Span) -> String {
        self.seen.snapshot.signature(span)
    }

    pub fn walked_every_root(&self) -> bool {
        self.seen.snapshot.walked_every_root()
    }

    pub fn read(
        &mut self,
        earliest_mtime_ms: i64,
        watermark: impl FnOnce() -> Watermark,
    ) -> SourceRead {
        let watermark = *self.watermark.get_or_insert_with(watermark);
        let scan_cache = self
            .scan_cache
            .get_or_insert_with(|| load_scan_cache(&self.scan_cache_path));
        let prepared = prepare_sources(
            &self.seen.snapshot,
            scan_cache,
            earliest_mtime_ms,
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
        richest_copies(
            self.files
                .iter()
                .map(|file| (file.provider, file.records.as_slice())),
        )
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
    dirs: Vec<(UsageProvider, TranscriptDirs)>,
    snapshot: SourceSnapshot,
}

impl SeenSources {
    pub fn providers<'a>(
        &'a self,
        read: &'a SourceRead,
    ) -> impl Iterator<Item = ProviderRead<'a>> + 'a {
        self.dirs.iter().map(move |(provider, dirs)| ProviderRead {
            provider: *provider,
            dir: dirs.reported(),
            files: dirs
                .all()
                .any(|dir| self.snapshot.root_is_present(*provider, dir))
                .then(|| read.files_read(*provider)),
        })
    }

    pub fn unchanged(&self, _locked: &Locked) -> bool {
        self.snapshot
            .persisted_generation_is_current(&self.source_index)
            && self.snapshot.inventory_is_current(&all_roots(&self.dirs))
    }
}

fn dirs_for(home: &Path) -> Vec<(UsageProvider, TranscriptDirs)> {
    UsageProvider::all()
        .map(|provider| (provider, transcript_dirs(provider, home)))
        .collect()
}

fn all_roots(dirs: &[(UsageProvider, TranscriptDirs)]) -> Vec<SourceRoot> {
    dirs.iter()
        .flat_map(|(provider, dirs)| {
            dirs.all().map(|path| SourceRoot {
                provider: *provider,
                path: path.to_path_buf(),
            })
        })
        .collect()
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
