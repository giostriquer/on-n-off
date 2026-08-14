//! Durable per-file scan cache keyed by `(path, size, mtime)`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::transcripts::{TokenTotals, UsageProvider, UsageRecord};

pub const USAGE_SCAN_CACHE_VERSION: u32 = 2;

#[derive(Debug, Clone)]
pub struct CachedFile {
    pub size: u64,
    pub mtime_ms: i64,
    pub provider: UsageProvider,
    pub records: Vec<UsageRecord>,
}

pub type ScanCache = HashMap<String, CachedFile>;

#[derive(Debug, Serialize, Deserialize)]
struct SerializedCache {
    version: u32,
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

pub fn encode_scan_cache(cache: &ScanCache) -> Value {
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
        models,
        sessions,
        files,
    })
    .unwrap_or(Value::Null)
}

pub fn decode_scan_cache(document: &Value) -> ScanCache {
    let mut cache = ScanCache::new();
    let Ok(root) = serde_json::from_value::<SerializedCache>(document.clone()) else {
        return cache;
    };
    if root.version != USAGE_SCAN_CACHE_VERSION {
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
            if arr.len() < 10 {
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
            let (Some(uncached), Some(cached), Some(cache_creation), Some(output), Some(reasoning)) =
                (nums(3), nums(4), nums(5), nums(6), nums(7))
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
                records,
            },
        );
    }
    cache
}

pub struct PruneOptions<'a> {
    pub live_paths: &'a HashSet<String>,
    pub walked_roots: &'a [String],
    pub window_start_ms: i64,
    pub retention_cutoff_ms: i64,
}

pub fn prune_scan_cache(cache: &mut ScanCache, options: PruneOptions<'_>) -> usize {
    let mut removed = 0;
    let keys: Vec<String> = cache.keys().cloned().collect();
    for path in keys {
        let Some(entry) = cache.get(&path) else {
            continue;
        };
        let aged_out = entry.mtime_ms < options.retention_cutoff_ms;
        let under_walked = options
            .walked_roots
            .iter()
            .any(|root| path_under_root(&path, root));
        let deleted = under_walked
            && entry.mtime_ms >= options.window_start_ms
            && !options.live_paths.contains(&path);
        if aged_out || deleted {
            cache.remove(&path);
            removed += 1;
        }
    }
    removed
}

fn path_under_root(path: &str, root: &str) -> bool {
    Path::new(path).starts_with(Path::new(root))
}

pub fn dedupe_within_file(records: &[UsageRecord]) -> Vec<UsageRecord> {
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    for record in records {
        if let Some(key) = &record.dedupe_key {
            if !seen.insert(key.clone()) {
                continue;
            }
        }
        kept.push(record.clone());
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record() -> UsageRecord {
        UsageRecord {
            provider: UsageProvider::Claude,
            timestamp_ms: 1_000,
            model: "claude-fable-5".into(),
            session_id: "session-a".into(),
            totals: TokenTotals {
                uncached_input_tokens: 1,
                cached_input_tokens: 2,
                cache_creation_tokens: 3,
                output_tokens: 4,
                reasoning_tokens: 0,
            },
            reported_cost_usd: Some(1.5),
            dedupe_key: Some("msg_1:".into()),
        }
    }

    #[test]
    fn encode_decode_round_trip() {
        let mut cache = ScanCache::new();
        cache.insert(
            "/a.jsonl".into(),
            CachedFile {
                size: 100,
                mtime_ms: 50,
                provider: UsageProvider::Claude,
                records: vec![sample_record()],
            },
        );
        let encoded = encode_scan_cache(&cache);
        let restored = decode_scan_cache(&encoded);
        assert_eq!(
            restored.get("/a.jsonl").unwrap().records[0],
            sample_record()
        );
    }

    #[test]
    fn wrong_version_yields_empty() {
        let doc = serde_json::json!({
            "version": 999,
            "models": [],
            "sessions": [],
            "files": {}
        });
        assert!(decode_scan_cache(&doc).is_empty());
    }

    #[test]
    fn dedupe_within_file_keeps_first() {
        let a = sample_record();
        let mut b = sample_record();
        b.totals.output_tokens = 99;
        let kept = dedupe_within_file(&[a.clone(), b]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].totals.output_tokens, 4);
    }

    #[test]
    fn prune_drops_aged_and_deleted_in_window() {
        let mut cache = ScanCache::new();
        cache.insert(
            "/root/old.jsonl".into(),
            CachedFile {
                size: 1,
                mtime_ms: 100,
                provider: UsageProvider::Claude,
                records: vec![],
            },
        );
        cache.insert(
            "/root/gone.jsonl".into(),
            CachedFile {
                size: 1,
                mtime_ms: 5_000,
                provider: UsageProvider::Claude,
                records: vec![],
            },
        );
        cache.insert(
            "/root/live.jsonl".into(),
            CachedFile {
                size: 1,
                mtime_ms: 5_000,
                provider: UsageProvider::Claude,
                records: vec![],
            },
        );
        let live = HashSet::from(["/root/live.jsonl".to_string()]);
        let roots = vec!["/root".to_string()];
        let removed = prune_scan_cache(
            &mut cache,
            PruneOptions {
                live_paths: &live,
                walked_roots: &roots,
                window_start_ms: 1_000,
                retention_cutoff_ms: 1_000,
            },
        );
        assert_eq!(removed, 2);
        assert!(cache.contains_key("/root/live.jsonl"));
    }
}
