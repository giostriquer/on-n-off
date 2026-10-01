use super::*;

fn sample_record() -> UsageRecord {
    UsageRecord {
        provider: UsageProvider::Claude,
        timestamp_ms: 1_000,
        model: "claude-fable-5".into(),
        session_id: "session-a".into(),
        // Every field distinct, so a column swapped in the row format cannot round-trip.
        totals: TokenTotals {
            uncached_input_tokens: 1,
            cached_input_tokens: 2,
            cache_creation_tokens: 5,
            cache_creation_1h_tokens: 3,
            output_tokens: 4,
            reasoning_tokens: 6,
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
fn a_one_hour_share_larger_than_its_cache_writes_decodes_clamped() {
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
    let mut encoded = encode_scan_cache(&cache);
    encoded["files"]["/a.jsonl"]["r"][0][10] = serde_json::json!(99);

    let restored = decode_scan_cache(&encoded);
    let totals = &restored.get("/a.jsonl").unwrap().records[0].totals;
    assert_eq!(totals.cache_creation_1h_tokens, 5);
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
    let removed = prune_scan_cache(
        &mut cache,
        &PruneOptions {
            live_paths: HashSet::from([
                "/root/old.jsonl".to_string(),
                "/root/live.jsonl".to_string(),
            ]),
            active_roots: vec!["/root".to_string(), "/pending".to_string()],
            walked_roots: vec!["/root".to_string()],
            watermark: crate::usage::history::Watermark::NONE,
        },
    );
    assert_eq!(removed, 2);
    assert!(cache.contains_key("/root/old.jsonl"));
    assert!(cache.contains_key("/root/live.jsonl"));
    assert!(cache.contains_key("/pending/unknown.jsonl"));
}
