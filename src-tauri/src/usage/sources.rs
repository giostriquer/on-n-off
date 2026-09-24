//! Where Usage finds the transcripts and keeps its files, and the lock every read and fold of
//! them takes.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use crate::paths::{claude_root_for, codex_root_for};

use super::cache_io::atomic_write;
use super::history::{history_path_for, Watermark};
use super::scan_cache::{
    decode_scan_cache, encode_scan_cache, prune_scan_cache, PruneOptions, ScanCache,
};
use super::source_index::{normalize_path, source_index_path_for, SourceRoot, SourceSnapshot};
use super::summary_cache::summary_cache_path_for;
use super::transcripts::UsageProvider;

/// Serializes every read and write of the usage caches and the history within this process.
pub fn lock_usage_files() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The files Usage keeps under `~/.on-n-off/`.
pub struct UsagePaths {
    pub scan_cache: PathBuf,
    pub summary: PathBuf,
    pub source_index: PathBuf,
    pub history: PathBuf,
}

impl UsagePaths {
    pub fn for_home(home: &Path) -> Self {
        Self {
            scan_cache: scan_cache_path_for(home),
            summary: summary_cache_path_for(home),
            source_index: source_index_path_for(home),
            history: history_path_for(home),
        }
    }
}

pub fn scan_cache_path_for(home: &Path) -> PathBuf {
    home.join(".on-n-off").join("usage-scan-cache.json")
}

/// One source per provider, shown under its first root; a source may scan several roots.
pub fn source_roots_for(home: &Path) -> [(UsageProvider, Vec<SourceRoot>); 2] {
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
pub fn all_roots(source_roots: &[(UsageProvider, Vec<SourceRoot>)]) -> Vec<SourceRoot> {
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

pub fn load_scan_cache(path: &Path) -> ScanCache {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ScanCache::new();
    };
    let Ok(doc) = serde_json::from_str(&raw) else {
        return ScanCache::new();
    };
    decode_scan_cache(&doc)
}

/// Drops what the scan cache no longer needs (deleted, outside every root, or held by the
/// history) and saves it when anything changed.
pub fn prune_and_persist_scan_cache(
    path: &Path,
    file_cache: &mut ScanCache,
    snapshot: &SourceSnapshot,
    roots: &[SourceRoot],
    watermark: Watermark,
    dirty: bool,
) {
    let live_paths = snapshot.live_paths();
    let active_roots: Vec<String> = roots
        .iter()
        .map(|root| normalize_path(&root.path))
        .collect();
    let pruned = prune_scan_cache(
        file_cache,
        PruneOptions {
            live_paths: &live_paths,
            active_roots: &active_roots,
            walked_roots: snapshot.successfully_walked_root_paths(),
            watermark,
        },
    );
    if dirty || pruned > 0 {
        let doc = encode_scan_cache(file_cache);
        if let Ok(raw) = serde_json::to_string(&doc) {
            let _ = atomic_write(path, &raw);
        }
    }
}
