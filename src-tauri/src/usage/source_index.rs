//! Small transcript inventory used to validate cached Usage summaries.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use super::cache_io::atomic_write;
use super::reader::{inventory_transcript_files, read_transcript_records, TranscriptFile};
use super::scan_cache::{dedupe_within_file, CachedFile, ScanCache, USAGE_SCAN_CACHE_VERSION};
use super::transcripts::{UsageProvider, UsageRecord, USAGE_TRANSCRIPT_PARSER_VERSION};

pub const USAGE_SOURCE_INDEX_VERSION: u32 = 3;

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
    complete: bool,
}

pub struct ReconcileOutcome {
    pub snapshot: SourceSnapshot,
    pub scan_cache_dirty: bool,
}

pub struct PreparedSourceFile {
    pub provider: UsageProvider,
    pub path: String,
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

pub fn reconcile(
    path: &Path,
    roots: &[SourceRoot],
    scan_cache: &mut ScanCache,
) -> ReconcileOutcome {
    let inventories = roots
        .iter()
        .cloned()
        .map(|root| {
            let inventory = inventory_root(&root);
            (root, inventory)
        })
        .collect();
    reconcile_inventories(path, inventories, scan_cache)
}

fn reconcile_inventories(
    path: &Path,
    inventories: Vec<(SourceRoot, RootInventory)>,
    scan_cache: &mut ScanCache,
) -> ReconcileOutcome {
    let previous = load_index(path);
    let mut indexed_roots = BTreeMap::new();
    let mut entries = BTreeMap::new();
    let mut scan_cache_dirty = false;
    let mut complete = true;

    for (root, root_inventory) in inventories {
        complete &= root_inventory.complete;
        let normalized_root = normalize_path(&root.path);
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
            complete,
        },
        scan_cache_dirty,
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
            path: entry.path.clone(),
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
        || read_transcript_records(path, provider).map(|records| dedupe_within_file(&records)),
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
    let mut indexed_roots = BTreeMap::new();
    let mut entries = BTreeMap::new();
    let mut complete = true;
    for root in roots {
        let root_inventory = inventory_root(root);
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
        for file in root_inventory.files {
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
mod tests {
    use super::*;
    use crate::paths::scratch_dir;
    use std::thread;
    use std::time::Duration;

    const JULY_START: i64 = 1_783_036_800_000;
    const AUGUST_START: i64 = 1_785_715_200_000;
    const SEPTEMBER_START: i64 = 1_788_393_600_000;

    fn roots(home: &Path) -> Vec<SourceRoot> {
        vec![SourceRoot {
            provider: UsageProvider::Claude,
            path: home.join(".claude").join("projects"),
        }]
    }

    fn transcript_path(home: &Path, name: &str) -> PathBuf {
        home.join(".claude")
            .join("projects")
            .join("fixture")
            .join(name)
    }

    fn record(timestamp: &str, message_id: &str, output_tokens: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": timestamp,
            "sessionId": "source-index-session",
            "message": {
                "id": message_id,
                "model": "claude-fable-5",
                "usage": {
                    "input_tokens": 1,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": 0,
                    "output_tokens": output_tokens
                }
            }
        })
        .to_string()
    }

    fn write_records(path: &Path, records: &[String]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("{}\n", records.join("\n"))).unwrap();
    }

    fn reconcile_home(home: &Path, cache: &mut ScanCache) -> ReconcileOutcome {
        reconcile(&source_index_path_for(home), &roots(home), cache)
    }

    #[test]
    fn unchanged_sources_keep_generation_and_window_signature() {
        let home = scratch_dir("usage-source-index-unchanged");
        write_records(
            &transcript_path(&home, "august.jsonl"),
            &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
        );
        let mut cache = ScanCache::new();
        let first = reconcile_home(&home, &mut cache);
        let first_signature = first.snapshot.signature(AUGUST_START, SEPTEMBER_START);
        assert!(first.scan_cache_dirty);
        assert!(first
            .snapshot
            .persisted_generation_is_current(&source_index_path_for(&home)));

        let second = reconcile_home(&home, &mut cache);
        assert!(!second.scan_cache_dirty);
        assert_eq!(second.snapshot.generation(), first.snapshot.generation());
        assert_eq!(
            second.snapshot.signature(AUGUST_START, SEPTEMBER_START),
            first_signature
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn created_source_changes_intersecting_window_signature() {
        let home = scratch_dir("usage-source-index-create");
        let mut cache = ScanCache::new();
        let empty = reconcile_home(&home, &mut cache);
        let before = empty.snapshot.signature(AUGUST_START, SEPTEMBER_START);

        write_records(
            &transcript_path(&home, "created.jsonl"),
            &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
        );
        let created = reconcile_home(&home, &mut cache);
        assert!(!empty
            .snapshot
            .persisted_generation_is_current(&source_index_path_for(&home)));
        assert!(created
            .snapshot
            .persisted_generation_is_current(&source_index_path_for(&home)));
        assert_ne!(
            created.snapshot.signature(AUGUST_START, SEPTEMBER_START),
            before
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn appended_source_changes_intersecting_window_signature() {
        let home = scratch_dir("usage-source-index-append");
        let path = transcript_path(&home, "append.jsonl");
        let first_record = record("2026-08-07T04:05:13.944Z", "msg-1", 20);
        write_records(&path, std::slice::from_ref(&first_record));
        let mut cache = ScanCache::new();
        let first = reconcile_home(&home, &mut cache);
        let before = first.snapshot.signature(AUGUST_START, SEPTEMBER_START);

        write_records(
            &path,
            &[
                first_record,
                record("2026-08-08T04:05:13.944Z", "msg-2", 25),
            ],
        );
        let appended = reconcile_home(&home, &mut cache);
        assert_ne!(
            appended.snapshot.signature(AUGUST_START, SEPTEMBER_START),
            before
        );
        assert_eq!(cache.get(&normalize_path(&path)).unwrap().records.len(), 2);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn same_size_rewrite_with_new_mtime_changes_time_spans() {
        let home = scratch_dir("usage-source-index-rewrite");
        let path = transcript_path(&home, "rewrite.jsonl");
        let august = record("2026-08-07T04:05:13.944Z", "msg-a", 20);
        let july = record("2026-07-07T04:05:13.944Z", "msg-j", 20);
        assert_eq!(august.len(), july.len());
        write_records(&path, &[august]);
        let mut cache = ScanCache::new();
        let first = reconcile_home(&home, &mut cache);
        let august_before = first.snapshot.signature(AUGUST_START, SEPTEMBER_START);
        let july_before = first.snapshot.signature(JULY_START, AUGUST_START);
        let old_mtime = file_identity(&path).unwrap().1;

        thread::sleep(Duration::from_millis(20));
        write_records(&path, &[july]);
        if file_identity(&path).unwrap().1 == old_mtime {
            thread::sleep(Duration::from_millis(20));
            let same = std::fs::read_to_string(&path).unwrap();
            std::fs::write(&path, same).unwrap();
        }
        let rewritten = reconcile_home(&home, &mut cache);
        assert_ne!(
            rewritten.snapshot.signature(AUGUST_START, SEPTEMBER_START),
            august_before
        );
        assert_ne!(
            rewritten.snapshot.signature(JULY_START, AUGUST_START),
            july_before
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn deleted_source_changes_its_previous_window_signature() {
        let home = scratch_dir("usage-source-index-delete");
        let path = transcript_path(&home, "delete.jsonl");
        write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
        let mut cache = ScanCache::new();
        let first = reconcile_home(&home, &mut cache);
        let before = first.snapshot.signature(AUGUST_START, SEPTEMBER_START);

        std::fs::remove_file(path).unwrap();
        let deleted = reconcile_home(&home, &mut cache);
        assert_ne!(
            deleted.snapshot.signature(AUGUST_START, SEPTEMBER_START),
            before
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn disjoint_window_signature_ignores_unrelated_source_change() {
        let home = scratch_dir("usage-source-index-disjoint");
        let july_path = transcript_path(&home, "july.jsonl");
        let august_path = transcript_path(&home, "august.jsonl");
        write_records(
            &july_path,
            &[record("2026-07-07T04:05:13.944Z", "msg-j", 10)],
        );
        write_records(
            &august_path,
            &[record("2026-08-07T04:05:13.944Z", "msg-a", 20)],
        );
        let mut cache = ScanCache::new();
        let first = reconcile_home(&home, &mut cache);
        let july_before = first.snapshot.signature(JULY_START, AUGUST_START);
        let august_before = first.snapshot.signature(AUGUST_START, SEPTEMBER_START);

        write_records(
            &august_path,
            &[
                record("2026-08-07T04:05:13.944Z", "msg-a", 20),
                record("2026-08-08T04:05:13.944Z", "msg-b", 25),
            ],
        );
        let changed = reconcile_home(&home, &mut cache);
        assert_eq!(
            changed.snapshot.signature(JULY_START, AUGUST_START),
            july_before
        );
        assert_ne!(
            changed.snapshot.signature(AUGUST_START, SEPTEMBER_START),
            august_before
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn empty_source_signature_includes_requested_bounds() {
        let home = scratch_dir("usage-source-index-bounds");
        let mut cache = ScanCache::new();
        let snapshot = reconcile_home(&home, &mut cache).snapshot;
        assert_ne!(
            snapshot.signature(JULY_START, AUGUST_START),
            snapshot.signature(AUGUST_START, SEPTEMBER_START)
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn empty_root_presence_changes_signature_in_both_directions() {
        let home = scratch_dir("usage-source-index-root-presence");
        let root = roots(&home).remove(0);
        let mut cache = ScanCache::new();

        let missing = reconcile_home(&home, &mut cache);
        let missing_signature = missing.snapshot.signature(AUGUST_START, SEPTEMBER_START);
        assert!(!missing.snapshot.root_is_present(&root));

        std::fs::create_dir_all(&root.path).unwrap();
        let present = reconcile_home(&home, &mut cache);
        let present_signature = present.snapshot.signature(AUGUST_START, SEPTEMBER_START);
        assert!(present.snapshot.root_is_present(&root));
        assert_ne!(present_signature, missing_signature);

        std::fs::remove_dir_all(home.join(".claude")).unwrap();
        let missing_again = reconcile_home(&home, &mut cache);
        assert!(!missing_again.snapshot.root_is_present(&root));
        assert_ne!(
            missing_again
                .snapshot
                .signature(AUGUST_START, SEPTEMBER_START),
            present_signature
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn incomplete_root_walk_retains_previous_entries_and_disables_hits() {
        let home = scratch_dir("usage-source-index-failed-walk");
        write_records(
            &transcript_path(&home, "retained.jsonl"),
            &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
        );
        let mut cache = ScanCache::new();
        let first = reconcile_home(&home, &mut cache);
        let generation = first.snapshot.generation();

        let root = roots(&home).remove(0);
        let failed = reconcile_inventories(
            &source_index_path_for(&home),
            vec![(
                root,
                RootInventory {
                    files: Vec::new(),
                    present: true,
                    complete: false,
                },
            )],
            &mut cache,
        );
        assert!(!failed.snapshot.is_complete());
        assert_eq!(failed.snapshot.generation(), generation);
        assert_eq!(failed.snapshot.entries.len(), 1);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn unresolved_file_is_pending_and_retried_on_next_reconciliation() {
        let home = scratch_dir("usage-source-index-pending");
        let path = transcript_path(&home, "pending.jsonl");
        let root = roots(&home).remove(0);
        let observed = TranscriptFile {
            path: path.clone(),
            size: 1,
            mtime_ms: 1,
        };
        let mut cache = ScanCache::new();
        let pending = reconcile_inventories(
            &source_index_path_for(&home),
            vec![(
                root,
                RootInventory {
                    files: vec![observed],
                    present: true,
                    complete: true,
                },
            )],
            &mut cache,
        );
        assert!(!pending.snapshot.is_complete());
        assert!(cache.is_empty());

        write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
        let resolved = reconcile_home(&home, &mut cache);
        assert!(resolved.snapshot.is_complete());
        assert_eq!(cache.get(&normalize_path(&path)).unwrap().records.len(), 1);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn file_that_changes_during_both_parse_attempts_remains_pending() {
        let identities = std::cell::RefCell::new(vec![(2, 2), (3, 3)].into_iter());
        let result =
            read_stable_records_with(1, 1, || Some(Vec::new()), || identities.borrow_mut().next());
        assert!(result.is_none());
    }

    #[test]
    fn incompatible_v1_index_is_rebuilt() {
        let home = scratch_dir("usage-source-index-v1");
        write_records(
            &transcript_path(&home, "valid.jsonl"),
            &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
        );
        let index_path = source_index_path_for(&home);
        std::fs::create_dir_all(index_path.parent().unwrap()).unwrap();
        std::fs::write(&index_path, r#"{"version":1,"generation":99,"entries":{}}"#).unwrap();
        let mut cache = ScanCache::new();
        let rebuilt = reconcile_home(&home, &mut cache);
        assert!(rebuilt.snapshot.is_complete());
        assert_eq!(rebuilt.snapshot.generation(), 1);
        let persisted: SourceIndexFile =
            serde_json::from_str(&std::fs::read_to_string(index_path).unwrap()).unwrap();
        assert_eq!(persisted.version, USAGE_SOURCE_INDEX_VERSION);
        assert_eq!(persisted.parser_version, USAGE_TRANSCRIPT_PARSER_VERSION);
        assert_eq!(persisted.scan_cache_version, USAGE_SCAN_CACHE_VERSION);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn corrupt_index_is_rebuilt_from_disposable_sources() {
        let home = scratch_dir("usage-source-index-corrupt");
        let path = transcript_path(&home, "valid.jsonl");
        write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
        let index_path = source_index_path_for(&home);
        std::fs::create_dir_all(index_path.parent().unwrap()).unwrap();
        std::fs::write(&index_path, "{partial").unwrap();

        let mut cache = ScanCache::new();
        let rebuilt = reconcile_home(&home, &mut cache);
        assert_eq!(rebuilt.snapshot.generation(), 1);
        assert_eq!(cache.len(), 1);
        let persisted: SourceIndexFile =
            serde_json::from_str(&std::fs::read_to_string(index_path).unwrap()).unwrap();
        assert_eq!(persisted.version, USAGE_SOURCE_INDEX_VERSION);
        assert_eq!(persisted.entries.len(), 1);
        let _ = std::fs::remove_dir_all(home);
    }
}
