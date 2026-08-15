//! Small transcript inventory used to validate cached Usage summaries.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use super::cache_io::atomic_write;
use super::reader::{inventory_transcript_files, read_transcript_records, TranscriptFile};
use super::scan_cache::{dedupe_within_file, CachedFile, ScanCache, USAGE_SCAN_CACHE_VERSION};
use super::transcripts::{UsageProvider, UsageRecord, USAGE_TRANSCRIPT_PARSER_VERSION};

pub const USAGE_SOURCE_INDEX_VERSION: u32 = 3;

#[cfg(test)]
#[path = "source_index_test_support.rs"]
mod test_support;

#[cfg(test)]
pub(crate) use test_support::{reset_transcript_parse_count, transcript_parse_count};

#[derive(Debug, Clone)]
pub struct SourceRoot {
    pub provider: UsageProvider,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SourceEntry {
    provider: UsageProvider,
    path: String,
    size: u64,
    mtime_ms: i64,
    min_record_ms: Option<i64>,
    max_record_ms: Option<i64>,
    resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SourceRootEntry {
    provider: UsageProvider,
    path: String,
    present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SourceIndexFile {
    version: u32,
    parser_version: u32,
    scan_cache_version: u32,
    generation: u64,
    roots: BTreeMap<String, SourceRootEntry>,
    entries: BTreeMap<String, SourceEntry>,
}

#[derive(Debug, Clone)]
pub struct SourceSnapshot {
    generation: u64,
    roots: BTreeMap<String, SourceRootEntry>,
    entries: BTreeMap<String, SourceEntry>,
    successfully_walked_roots: Vec<String>,
    complete: bool,
}

pub struct ReconcileOutcome {
    pub snapshot: SourceSnapshot,
    pub scan_cache_dirty: bool,
}

pub struct SourceInventory {
    roots: Vec<(SourceRoot, RootInventory)>,
}

pub struct PreparedSourceFile {
    pub provider: UsageProvider,
    pub records: Arc<Vec<UsageRecord>>,
}

pub struct PreparedSources {
    pub files: Vec<PreparedSourceFile>,
    pub complete: bool,
    pub scan_cache_dirty: bool,
}

pub fn source_index_path_for(home: &Path) -> PathBuf {
    home.join(".on-n-off").join("usage-source-index.json")
}

#[cfg(test)]
pub fn reconcile(
    path: &Path,
    roots: &[SourceRoot],
    scan_cache: &mut ScanCache,
) -> ReconcileOutcome {
    reconcile_inventory(path, inventory_sources(roots), scan_cache)
}

pub fn inventory_sources(roots: &[SourceRoot]) -> SourceInventory {
    let roots = roots
        .iter()
        .cloned()
        .map(|root| {
            let inventory = inventory_root(&root);
            (root, inventory)
        })
        .collect();
    SourceInventory { roots }
}

pub fn unchanged_snapshot(path: &Path, inventory: &SourceInventory) -> Option<SourceSnapshot> {
    let previous = load_index(path)?;
    let current = current_inventory_from(&inventory.roots);
    if !current.complete
        || current.roots != previous.roots
        || current.entries.len() != previous.entries.len()
        || previous.entries.values().any(|entry| !entry.resolved)
        || !current
            .entries
            .into_iter()
            .all(|(path, (provider, size, mtime_ms))| {
                previous.entries.get(&path).is_some_and(|entry| {
                    entry.provider == provider && entry.size == size && entry.mtime_ms == mtime_ms
                })
            })
    {
        return None;
    }

    Some(snapshot_from_index(previous, &inventory.roots, true))
}

pub fn reconcile_inventory(
    path: &Path,
    inventory: SourceInventory,
    scan_cache: &mut ScanCache,
) -> ReconcileOutcome {
    reconcile_inventories(path, inventory.roots, scan_cache)
}

fn reconcile_inventories(
    path: &Path,
    inventories: Vec<(SourceRoot, RootInventory)>,
    scan_cache: &mut ScanCache,
) -> ReconcileOutcome {
    let previous = load_index(path);
    let mut indexed_roots = BTreeMap::new();
    let mut entries = BTreeMap::new();
    let mut successfully_walked_roots = Vec::new();
    let mut scan_cache_dirty = false;
    let mut complete = true;

    for (root, root_inventory) in inventories {
        complete &= root_inventory.complete;
        let normalized_root = normalize_path(&root.path);
        if root_inventory.complete {
            successfully_walked_roots.push(normalized_root.clone());
        }
        let key = root_key(root.provider, &normalized_root);
        let root_entry = if root_inventory.complete {
            SourceRootEntry {
                provider: root.provider,
                path: normalized_root,
                present: root_inventory.present,
            }
        } else {
            previous
                .as_ref()
                .and_then(|index| index.roots.get(&key))
                .cloned()
                .unwrap_or(SourceRootEntry {
                    provider: root.provider,
                    path: normalized_root,
                    present: root_inventory.present,
                })
        };
        indexed_roots.insert(key, root_entry);
        for observed in root_inventory.files {
            let normalized = normalize_path(&observed.path);
            let previous_entry = previous
                .as_ref()
                .and_then(|index| index.entries.get(&normalized));
            let unchanged = previous_entry
                .filter(|entry| {
                    entry.resolved
                        && entry.provider == root.provider
                        && entry.size == observed.size
                        && entry.mtime_ms == observed.mtime_ms
                })
                .cloned();

            let (entry, resolved) = unchanged.map_or_else(
                || {
                    inspect_changed_file(
                        normalized.clone(),
                        observed,
                        root.provider,
                        scan_cache,
                        &mut scan_cache_dirty,
                    )
                },
                |entry| (entry, true),
            );
            complete &= resolved;
            entries.insert(normalized, entry);
        }

        if !root_inventory.complete {
            retain_previous_under_root(&mut entries, previous.as_ref(), &root);
        }
    }

    let changed = previous.as_ref().is_none_or(|index| {
        index.version != USAGE_SOURCE_INDEX_VERSION
            || index.parser_version != USAGE_TRANSCRIPT_PARSER_VERSION
            || index.scan_cache_version != USAGE_SCAN_CACHE_VERSION
            || index.roots != indexed_roots
            || index.entries != entries
    });
    let generation = previous.as_ref().map_or(1, |index| {
        index.generation.saturating_add(u64::from(changed))
    });
    let file = SourceIndexFile {
        version: USAGE_SOURCE_INDEX_VERSION,
        parser_version: USAGE_TRANSCRIPT_PARSER_VERSION,
        scan_cache_version: USAGE_SCAN_CACHE_VERSION,
        generation,
        roots: indexed_roots.clone(),
        entries: entries.clone(),
    };
    if changed || previous.is_none() {
        persist_index(path, &file);
    }

    ReconcileOutcome {
        snapshot: SourceSnapshot {
            generation,
            roots: indexed_roots,
            entries,
            successfully_walked_roots,
            complete,
        },
        scan_cache_dirty,
    }
}

fn snapshot_from_index(
    index: SourceIndexFile,
    inventories: &[(SourceRoot, RootInventory)],
    complete: bool,
) -> SourceSnapshot {
    let successfully_walked_roots = inventories
        .iter()
        .filter(|(_, inventory)| inventory.complete)
        .map(|(root, _)| normalize_path(&root.path))
        .collect();
    SourceSnapshot {
        generation: index.generation,
        roots: index.roots,
        entries: index.entries,
        successfully_walked_roots,
        complete,
    }
}

struct RootInventory {
    files: Vec<TranscriptFile>,
    present: bool,
    complete: bool,
}

fn inventory_root(root: &SourceRoot) -> RootInventory {
    match std::fs::metadata(&root.path) {
        Ok(metadata) if metadata.is_dir() => {
            let inventory = inventory_transcript_files(&root.path, i64::MIN);
            RootInventory {
                files: inventory.files,
                present: true,
                complete: inventory.complete,
            }
        }
        Ok(_) => RootInventory {
            files: Vec::new(),
            present: false,
            complete: true,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => RootInventory {
            files: Vec::new(),
            present: false,
            complete: true,
        },
        Err(_) => RootInventory {
            files: Vec::new(),
            present: false,
            complete: false,
        },
    }
}

fn retain_previous_under_root(
    entries: &mut BTreeMap<String, SourceEntry>,
    previous: Option<&SourceIndexFile>,
    root: &SourceRoot,
) {
    let normalized_root = normalize_path(&root.path);
    let prefix = format!("{}/", normalized_root.trim_end_matches('/'));
    let Some(previous) = previous else {
        return;
    };
    for (path, entry) in &previous.entries {
        if entry.provider == root.provider
            && (path == &normalized_root || path.starts_with(&prefix))
        {
            entries.entry(path.clone()).or_insert_with(|| entry.clone());
        }
    }
}

impl SourceSnapshot {
    #[cfg(test)]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_complete(&self) -> bool {
        self.complete && self.entries.values().all(|entry| entry.resolved)
    }

    pub fn signature(&self, window_start_ms: i64, window_end_ms: i64) -> String {
        let mut hash = Fnv64::new();
        hash.add_u64(USAGE_SOURCE_INDEX_VERSION as u64);
        hash.add_u64(USAGE_TRANSCRIPT_PARSER_VERSION as u64);
        hash.add_u64(USAGE_SCAN_CACHE_VERSION as u64);
        hash.add_i64(window_start_ms);
        hash.add_i64(window_end_ms);
        for root in self.roots.values() {
            hash.add_bytes(root.provider.as_str().as_bytes());
            hash.add_bytes(root.path.as_bytes());
            hash.add_u64(u64::from(root.present));
        }
        for entry in self
            .entries
            .values()
            .filter(|entry| entry_intersects_window(entry, window_start_ms, window_end_ms))
        {
            hash.add_bytes(entry.provider.as_str().as_bytes());
            hash.add_bytes(entry.path.as_bytes());
            hash.add_u64(entry.size);
            hash.add_i64(entry.mtime_ms);
            hash.add_i64(entry.min_record_ms.unwrap_or(i64::MIN));
            hash.add_i64(entry.max_record_ms.unwrap_or(i64::MAX));
            hash.add_u64(u64::from(entry.resolved));
        }
        format!(
            "v{USAGE_SOURCE_INDEX_VERSION}-p{USAGE_TRANSCRIPT_PARSER_VERSION}-s{USAGE_SCAN_CACHE_VERSION}-{:016x}",
            hash.finish()
        )
    }

    pub fn inventory_is_current(&self, roots: &[SourceRoot]) -> bool {
        let current = current_inventory(roots);
        if !current.complete
            || current.roots != self.roots
            || current.entries.len() != self.entries.len()
        {
            return false;
        }
        current
            .entries
            .into_iter()
            .all(|(path, (provider, size, mtime_ms))| {
                self.entries.get(&path).is_some_and(|entry| {
                    entry.provider == provider && entry.size == size && entry.mtime_ms == mtime_ms
                })
            })
    }

    pub fn persisted_generation_is_current(&self, path: &Path) -> bool {
        load_index(path).is_some_and(|index| {
            index.generation == self.generation
                && index.roots == self.roots
                && index.entries == self.entries
        })
    }

    pub fn root_is_present(&self, root: &SourceRoot) -> bool {
        let normalized = normalize_path(&root.path);
        self.roots
            .get(&root_key(root.provider, &normalized))
            .is_some_and(|entry| entry.present)
    }

    pub fn live_paths(&self) -> HashSet<String> {
        self.entries.keys().cloned().collect()
    }

    pub fn successfully_walked_root_paths(&self) -> &[String] {
        &self.successfully_walked_roots
    }
}

pub fn prepare_sources(
    snapshot: &SourceSnapshot,
    scan_cache: &mut ScanCache,
    window_start_ms: i64,
) -> PreparedSources {
    let mut files = Vec::new();
    let mut complete = snapshot.is_complete();
    let mut scan_cache_dirty = false;

    for entry in snapshot
        .entries
        .values()
        .filter(|entry| entry.mtime_ms >= window_start_ms)
    {
        let records = scan_cache
            .get(&entry.path)
            .filter(|cached| {
                cached.provider == entry.provider
                    && cached.size == entry.size
                    && cached.mtime_ms == entry.mtime_ms
            })
            .map(|cached| Arc::clone(&cached.records))
            .or_else(|| {
                let path = Path::new(&entry.path);
                let (size, mtime_ms, records) =
                    read_stable_records(path, entry.provider, entry.size, entry.mtime_ms)?;
                if size != entry.size || mtime_ms != entry.mtime_ms {
                    return None;
                }
                let records = Arc::new(records);
                scan_cache.insert(
                    entry.path.clone(),
                    CachedFile {
                        size,
                        mtime_ms,
                        provider: entry.provider,
                        records: Arc::clone(&records),
                    },
                );
                scan_cache_dirty = true;
                Some(records)
            });

        let Some(records) = records else {
            complete = false;
            continue;
        };
        files.push(PreparedSourceFile {
            provider: entry.provider,
            records,
        });
    }

    PreparedSources {
        files,
        complete,
        scan_cache_dirty,
    }
}

fn load_index(path: &Path) -> Option<SourceIndexFile> {
    let raw = std::fs::read_to_string(path).ok()?;
    let index: SourceIndexFile = serde_json::from_str(&raw).ok()?;
    (index.version == USAGE_SOURCE_INDEX_VERSION
        && index.parser_version == USAGE_TRANSCRIPT_PARSER_VERSION
        && index.scan_cache_version == USAGE_SCAN_CACHE_VERSION)
        .then_some(index)
}

fn persist_index(path: &Path, index: &SourceIndexFile) {
    if let Ok(raw) = serde_json::to_string(index) {
        let _ = atomic_write(path, &raw);
    }
}

fn inspect_changed_file(
    normalized: String,
    observed: TranscriptFile,
    provider: UsageProvider,
    scan_cache: &mut ScanCache,
    scan_cache_dirty: &mut bool,
) -> (SourceEntry, bool) {
    if let Some(cached) = scan_cache.get(&normalized) {
        if cached.provider == provider
            && cached.size == observed.size
            && cached.mtime_ms == observed.mtime_ms
        {
            let (min_record_ms, max_record_ms) = record_bounds(&cached.records);
            return (
                SourceEntry {
                    provider,
                    path: normalized,
                    size: observed.size,
                    mtime_ms: observed.mtime_ms,
                    min_record_ms,
                    max_record_ms,
                    resolved: true,
                },
                true,
            );
        }
    }

    let parsed = read_stable_records(&observed.path, provider, observed.size, observed.mtime_ms);
    let parsed_ok = parsed.is_some();
    let (size, mtime_ms, records) =
        parsed.unwrap_or((observed.size, observed.mtime_ms, Vec::new()));
    let (min_record_ms, max_record_ms) = record_bounds(&records);
    if parsed_ok {
        scan_cache.insert(
            normalized.clone(),
            CachedFile {
                size,
                mtime_ms,
                provider,
                records: Arc::new(records),
            },
        );
        *scan_cache_dirty = true;
    }
    (
        SourceEntry {
            provider,
            path: normalized,
            size,
            mtime_ms,
            min_record_ms,
            max_record_ms,
            resolved: parsed_ok,
        },
        parsed_ok,
    )
}

pub fn read_stable_records(
    path: &Path,
    provider: UsageProvider,
    initial_size: u64,
    initial_mtime_ms: i64,
) -> Option<(u64, i64, Vec<UsageRecord>)> {
    read_stable_records_with(
        initial_size,
        initial_mtime_ms,
        || {
            #[cfg(test)]
            test_support::note_transcript_parse();
            read_transcript_records(path, provider).map(|records| dedupe_within_file(&records))
        },
        || file_identity(path),
    )
}

fn read_stable_records_with(
    initial_size: u64,
    initial_mtime_ms: i64,
    mut read: impl FnMut() -> Option<Vec<UsageRecord>>,
    mut identity: impl FnMut() -> Option<(u64, i64)>,
) -> Option<(u64, i64, Vec<UsageRecord>)> {
    let mut size = initial_size;
    let mut mtime_ms = initial_mtime_ms;
    for _ in 0..=1 {
        let records = read()?;
        let (after_size, after_mtime_ms) = identity()?;
        if after_size == size && after_mtime_ms == mtime_ms {
            return Some((size, mtime_ms, records));
        }
        size = after_size;
        mtime_ms = after_mtime_ms;
    }
    None
}

fn file_identity(path: &Path) -> Option<(u64, i64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime_ms = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((meta.len(), mtime_ms))
}

struct CurrentInventory {
    roots: BTreeMap<String, SourceRootEntry>,
    entries: BTreeMap<String, (UsageProvider, u64, i64)>,
    complete: bool,
}

fn current_inventory(roots: &[SourceRoot]) -> CurrentInventory {
    let inventory = inventory_sources(roots);
    current_inventory_from(&inventory.roots)
}

fn current_inventory_from(inventories: &[(SourceRoot, RootInventory)]) -> CurrentInventory {
    let mut indexed_roots = BTreeMap::new();
    let mut entries = BTreeMap::new();
    let mut complete = true;
    for (root, root_inventory) in inventories {
        complete &= root_inventory.complete;
        let normalized_root = normalize_path(&root.path);
        indexed_roots.insert(
            root_key(root.provider, &normalized_root),
            SourceRootEntry {
                provider: root.provider,
                path: normalized_root,
                present: root_inventory.present,
            },
        );
        for file in &root_inventory.files {
            entries.insert(
                normalize_path(&file.path),
                (root.provider, file.size, file.mtime_ms),
            );
        }
    }
    CurrentInventory {
        roots: indexed_roots,
        entries,
        complete,
    }
}

fn root_key(provider: UsageProvider, normalized_path: &str) -> String {
    format!("{}:{normalized_path}", provider.as_str())
}

fn record_bounds(records: &[UsageRecord]) -> (Option<i64>, Option<i64>) {
    let min = records.iter().map(|record| record.timestamp_ms).min();
    let max = records.iter().map(|record| record.timestamp_ms).max();
    (min, max)
}

fn entry_intersects_window(entry: &SourceEntry, start_ms: i64, end_ms: i64) -> bool {
    match (entry.min_record_ms, entry.max_record_ms) {
        (Some(min), Some(max)) => max >= start_ms && min < end_ms,
        _ => true,
    }
}

pub fn normalize_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

struct Fnv64(u64);

impl Fnv64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn add_bytes(&mut self, value: &[u8]) {
        self.add_u64(value.len() as u64);
        for byte in value {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn add_u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn add_i64(&mut self, value: i64) {
        self.add_u64(value as u64);
    }

    fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
#[path = "source_index_tests.rs"]
mod tests;
