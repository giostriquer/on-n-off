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
            records: Arc::new(vec![sample_record()]),
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
        "parser_version": USAGE_TRANSCRIPT_PARSER_VERSION,
        "models": [],
        "sessions": [],
        "files": {}
    });
    assert!(decode_scan_cache(&doc).is_empty());
}

#[test]
fn parser_incompatible_cache_drops_stale_records() {
    let mut stale_record = sample_record();
    stale_record.totals.output_tokens = 999;
    let mut cache = ScanCache::new();
    cache.insert(
        "/a.jsonl".into(),
        CachedFile {
            size: 100,
            mtime_ms: 50,
            provider: UsageProvider::Claude,
            records: Arc::new(vec![stale_record]),
        },
    );
    let mut encoded = encode_scan_cache(&cache);
    encoded["parser_version"] = serde_json::json!(USAGE_TRANSCRIPT_PARSER_VERSION + 1);

    assert!(decode_scan_cache(&encoded).is_empty());
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
fn prune_keeps_live_history_and_incomplete_roots() {
    let mut cache = ScanCache::new();
    cache.insert(
        "/root/old.jsonl".into(),
        CachedFile {
            size: 1,
            mtime_ms: 100,
            provider: UsageProvider::Claude,
            records: Arc::new(vec![]),
        },
    );
    cache.insert(
        "/root/gone.jsonl".into(),
        CachedFile {
            size: 1,
            mtime_ms: 5_000,
            provider: UsageProvider::Claude,
            records: Arc::new(vec![]),
        },
    );
    cache.insert(
        "/root/live.jsonl".into(),
        CachedFile {
            size: 1,
            mtime_ms: 5_000,
            provider: UsageProvider::Claude,
            records: Arc::new(vec![]),
        },
    );
    cache.insert(
        "/pending/unknown.jsonl".into(),
        CachedFile {
            size: 1,
            mtime_ms: 100,
            provider: UsageProvider::Claude,
            records: Arc::new(vec![]),
        },
    );
    cache.insert(
        "/outside/stale.jsonl".into(),
        CachedFile {
            size: 1,
            mtime_ms: 100,
            provider: UsageProvider::Claude,
            records: Arc::new(vec![]),
        },
    );
    let live = HashSet::from([
        "/root/old.jsonl".to_string(),
        "/root/live.jsonl".to_string(),
    ]);
    let active_roots = vec!["/root".to_string(), "/pending".to_string()];
    let walked_roots = vec!["/root".to_string()];
    let removed = prune_scan_cache(
        &mut cache,
        PruneOptions {
            live_paths: &live,
            active_roots: &active_roots,
            walked_roots: &walked_roots,
        },
    );
    assert_eq!(removed, 2);
    assert!(cache.contains_key("/root/old.jsonl"));
    assert!(cache.contains_key("/root/live.jsonl"));
    assert!(cache.contains_key("/pending/unknown.jsonl"));
}
