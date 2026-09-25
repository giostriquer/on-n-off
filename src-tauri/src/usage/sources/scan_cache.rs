//! Durable per-file scan cache keyed by `(path, size, mtime)`: each transcript's last parse, so a
//! read parses only what changed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(test)]
use std::cell::Cell;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::usage::cache_io::atomic_write;
use crate::usage::history::Watermark;
use crate::usage::transcripts::USAGE_TRANSCRIPT_PARSER_VERSION;
use crate::usage::transcripts::{richest_copies, TokenTotals, UsageProvider, UsageRecord};

/// v4: rows carry the one-hour cache-write share at index 10.
pub(crate) const USAGE_SCAN_CACHE_VERSION: u32 = 4;

#[cfg(test)]
thread_local! {
    static SCAN_CACHE_DECODE_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_scan_cache_decode_count() {
    SCAN_CACHE_DECODE_COUNT.set(0);
}

#[cfg(test)]
pub(crate) fn scan_cache_decode_count() -> usize {
    SCAN_CACHE_DECODE_COUNT.get()
}

#[derive(Debug, Clone)]
pub(super) struct CachedFile {
    pub(super) size: u64,
    pub(super) mtime_ms: i64,
    pub(super) provider: UsageProvider,
    pub(super) records: Arc<Vec<UsageRecord>>,
}

impl CachedFile {
    /// A parse of the transcript `provider` wrote, as it is now: same size, same mtime.
    pub(super) fn is_parse_of(&self, provider: UsageProvider, size: u64, mtime_ms: i64) -> bool {
        self.provider == provider && self.size == size && self.mtime_ms == mtime_ms
    }
}

pub(super) type ScanCache = HashMap<String, CachedFile>;

#[derive(Debug, Serialize, Deserialize)]
struct SerializedCache {
    version: u32,
    parser_version: u32,
    models: Vec<String>,
    sessions: Vec<String>,
    files: HashMap<String, SerializedFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SerializedFile {
    s: u64,
    m: i64,
    p: String,
    r: Vec<Value>,
}

pub(super) fn scan_cache_path_for(home: &Path) -> PathBuf {
    home.join(".on-n-off").join("usage-scan-cache.json")
}

/// The scan cache kept at `path`: empty when there is none or it does not read.
pub(super) fn load_scan_cache(path: &Path) -> ScanCache {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ScanCache::new();
    };
    let Ok(doc) = serde_json::from_str(&raw) else {
        return ScanCache::new();
    };
    decode_scan_cache(&doc)
}

/// Drops what the scan cache no longer needs: transcripts deleted from a root walked to the end,
/// outside every root, or whose records the history holds (`PruneOptions`). Saves it at `path`
/// when that, or a read (`changed`), changed it.
pub(super) fn prune_and_save(
    path: &Path,
    cache: &mut ScanCache,
    options: &PruneOptions,
    changed: bool,
) {
    let pruned = prune_scan_cache(cache, options);
    if changed || pruned > 0 {
        let doc = encode_scan_cache(cache);
        if let Ok(raw) = serde_json::to_string(&doc) {
            let _ = atomic_write(path, &raw);
        }
    }
}

fn encode_scan_cache(cache: &ScanCache) -> Value {
    let mut models: Vec<String> = Vec::new();
    let mut sessions: Vec<String> = Vec::new();
    let mut model_index: HashMap<String, usize> = HashMap::new();
    let mut session_index: HashMap<String, usize> = HashMap::new();

    let intern =
        |table: &mut Vec<String>, index: &mut HashMap<String, usize>, value: &str| -> usize {
            if let Some(i) = index.get(value) {
                return *i;
            }
            let next = table.len();
            table.push(value.to_string());
            index.insert(value.to_string(), next);
            next
        };

    let mut files = HashMap::new();
    for (path, entry) in cache {
        let rows: Vec<Value> = entry
            .records
            .iter()
            .map(|record| {
                serde_json::json!([
                    record.timestamp_ms,
                    intern(&mut models, &mut model_index, &record.model),
                    intern(&mut sessions, &mut session_index, &record.session_id),
                    record.totals.uncached_input_tokens,
                    record.totals.cached_input_tokens,
                    record.totals.cache_creation_tokens,
                    record.totals.output_tokens,
                    record.totals.reasoning_tokens,
                    record.dedupe_key,
                    record.reported_cost_usd,
                    record.totals.cache_creation_1h_tokens,
                ])
            })
            .collect();
        files.insert(
            path.clone(),
            SerializedFile {
                s: entry.size,
                m: entry.mtime_ms,
                p: match entry.provider {
                    UsageProvider::Claude => "claude".into(),
                    UsageProvider::Codex => "codex".into(),
                },
                r: rows,
            },
        );
    }

    serde_json::to_value(SerializedCache {
        version: USAGE_SCAN_CACHE_VERSION,
        parser_version: USAGE_TRANSCRIPT_PARSER_VERSION,
        models,
        sessions,
        files,
    })
    .unwrap_or(Value::Null)
}

fn decode_scan_cache(document: &Value) -> ScanCache {
    #[cfg(test)]
    SCAN_CACHE_DECODE_COUNT.set(SCAN_CACHE_DECODE_COUNT.get() + 1);
    let mut cache = ScanCache::new();
    let Ok(root) = serde_json::from_value::<SerializedCache>(document.clone()) else {
        return cache;
    };
    if root.version != USAGE_SCAN_CACHE_VERSION
        || root.parser_version != USAGE_TRANSCRIPT_PARSER_VERSION
    {
        return cache;
    }

    for (path, entry) in root.files {
        let provider = match entry.p.as_str() {
            "claude" => UsageProvider::Claude,
            "codex" => UsageProvider::Codex,
            _ => continue,
        };
        let mut records = Vec::new();
        let mut corrupt = false;
        for row in &entry.r {
            let Some(arr) = row.as_array() else {
                corrupt = true;
                break;
            };
            if arr.len() < 11 {
                corrupt = true;
                break;
            }
            let timestamp_ms = match arr[0].as_i64() {
                Some(v) if (v as f64).is_finite() => v,
                _ => {
                    corrupt = true;
                    break;
                }
            };
            let model_i = match arr[1].as_u64() {
                Some(i) => i as usize,
                None => {
                    corrupt = true;
                    break;
                }
            };
            let session_i = match arr[2].as_u64() {
                Some(i) => i as usize,
                None => {
                    corrupt = true;
                    break;
                }
            };
            let Some(model) = root.models.get(model_i).cloned() else {
                corrupt = true;
                break;
            };
            let session_id = root.sessions.get(session_i).cloned().unwrap_or_default();
            let nums = |i: usize| -> Option<u64> {
                arr.get(i)?
                    .as_u64()
                    .or_else(|| arr.get(i)?.as_f64().map(|f| f as u64))
            };
            let (
                Some(uncached),
                Some(cached),
                Some(cache_creation),
                Some(output),
                Some(reasoning),
                Some(cache_creation_1h),
            ) = (nums(3), nums(4), nums(5), nums(6), nums(7), nums(10))
            else {
                corrupt = true;
                break;
            };
            let dedupe_key = match &arr[8] {
                Value::Null => None,
                Value::String(s) => Some(s.clone()),
                _ => {
                    corrupt = true;
                    break;
                }
            };
            let reported_cost_usd = match &arr[9] {
                Value::Null => None,
                Value::Number(n) => n.as_f64(),
                _ => None,
            };
            records.push(UsageRecord {
                provider,
                timestamp_ms,
                model,
                session_id,
                totals: TokenTotals {
                    uncached_input_tokens: uncached,
                    cached_input_tokens: cached,
                    cache_creation_tokens: cache_creation,
                    cache_creation_1h_tokens: cache_creation_1h.min(cache_creation),
                    output_tokens: output,
                    reasoning_tokens: reasoning,
                },
                reported_cost_usd,
                dedupe_key,
            });
        }
        if corrupt {
            continue;
        }
        cache.insert(
            path,
            CachedFile {
                size: entry.s,
                mtime_ms: entry.m,
                provider,
                records: Arc::new(records),
            },
        );
    }
    cache
}

/// What the scan cache is kept for, in paths normalized as the source index keys them.
pub(super) struct PruneOptions {
    /// Every transcript indexed now.
    pub(super) live_paths: HashSet<String>,
    /// Every root: a parse outside all of them leaves the cache.
    pub(super) active_roots: Vec<String>,
    /// The roots walked to the end: a parse under one of them that is not indexed was deleted.
    pub(super) walked_roots: Vec<String>,
    /// Files whose every record the usage history holds leave the cache.
    pub(super) watermark: Watermark,
}

fn prune_scan_cache(cache: &mut ScanCache, options: &PruneOptions) -> usize {
    let mut removed = 0;
    let keys: Vec<String> = cache.keys().cloned().collect();
    for path in keys {
        let under_active = options
            .active_roots
            .iter()
            .any(|root| path_under_root(&path, root));
        let under_walked = options
            .walked_roots
            .iter()
            .any(|root| path_under_root(&path, root));
        let deleted = under_walked && !options.live_paths.contains(&path);
        let folded = cache.get(&path).is_some_and(|cached| {
            let newest = cached
                .records
                .iter()
                .map(|record| record.timestamp_ms)
                .max();
            options.watermark.holds_only_folded(cached.mtime_ms, newest)
        });
        if !under_active || deleted || folded {
            cache.remove(&path);
            removed += 1;
        }
    }
    removed
}

fn path_under_root(path: &str, root: &str) -> bool {
    Path::new(path).starts_with(Path::new(root))
}

/// One file's records with each Claude message's lines collapsed to its richest copy (see
/// `richest_copies`), which keeps the cache small; the scan collapses copies across files again.
pub(super) fn dedupe_within_file(records: &[UsageRecord]) -> Vec<UsageRecord> {
    richest_copies([records]).into_iter().cloned().collect()
}

#[cfg(test)]
mod tests;
