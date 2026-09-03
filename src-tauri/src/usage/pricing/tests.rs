use super::*;
use crate::paths::scratch_dir;

fn sample_doc() -> Value {
    serde_json::json!({
        "half-priced": {
            "input_cost_per_token": 1e-5
        },
        "anthropic/claude-fable-5": {
            "input_cost_per_token": 1e-5,
            "output_cost_per_token": 5e-5,
            "cache_read_input_token_cost": 1e-6,
            "cache_creation_input_token_cost": 1.25e-5
        }
    })
}

#[test]
fn parse_drops_half_priced_and_normalizes() {
    let table = parse_rate_table(&sample_doc());
    assert!(table.contains_key("claude-fable-5"));
    assert!(!table.contains_key("half-priced"));
    assert!(!table.contains_key("anthropic/claude-fable-5"));
    let rate = table.get("claude-fable-5").unwrap();
    assert!(rate.input_cost_per_token > 0.0);
    assert!(rate.output_cost_per_token > 0.0);
}

#[test]
fn bare_entry_wins_collisions_regardless_of_document_order() {
    // LiteLLM lists the same model under several provider prefixes, and
    // reseller entries often omit cache pricing (the real
    // `deepinfra/anthropic/claude-*` rows). The canonical bare key must
    // win the normalized-key collision in either document order. Every
    // rate differs between the two entries — and the bare entry's cache
    // rates are not 0.1×/1.25× of either input — so the assertions
    // identify the winner rather than pass via derived defaults.
    let bare = serde_json::json!({
        "input_cost_per_token": 1e-5,
        "output_cost_per_token": 5e-5,
        "cache_read_input_token_cost": 2e-6,
        "cache_creation_input_token_cost": 1.5e-5
    });
    let reseller = serde_json::json!({
        "input_cost_per_token": 3e-5,
        "output_cost_per_token": 9e-5
    });
    for doc in [
        serde_json::json!({
            "claude-x": bare.clone(),
            "deepinfra/anthropic/claude-x": reseller.clone(),
        }),
        serde_json::json!({
            "deepinfra/anthropic/claude-x": reseller,
            "claude-x": bare,
        }),
    ] {
        let table = parse_rate_table(&doc);
        let rate = table.get("claude-x").unwrap();
        assert_eq!(rate.input_cost_per_token, 1e-5);
        assert_eq!(rate.output_cost_per_token, 5e-5);
        assert_eq!(rate.cache_read_cost_per_token, 2e-6);
        assert_eq!(rate.cache_creation_cost_per_token, 1.5e-5);
    }
}

#[test]
fn explicit_cache_pricing_wins_among_prefixed_entries() {
    // No bare key: the prefixed entry carrying published cache pricing
    // must beat the cache-less one in either document order. Distinct
    // input rates identify the winner independently of cache derivation.
    let with_cache = serde_json::json!({
        "input_cost_per_token": 1e-5,
        "output_cost_per_token": 5e-5,
        "cache_read_input_token_cost": 2e-6,
        "cache_creation_input_token_cost": 1.5e-5
    });
    let without_cache = serde_json::json!({
        "input_cost_per_token": 3e-5,
        "output_cost_per_token": 9e-5
    });
    for doc in [
        serde_json::json!({
            "aaa/model-y": with_cache.clone(),
            "zzz/model-y": without_cache.clone(),
        }),
        serde_json::json!({
            "zzz/model-y": without_cache,
            "aaa/model-y": with_cache,
        }),
    ] {
        let table = parse_rate_table(&doc);
        let rate = table.get("model-y").unwrap();
        assert_eq!(rate.input_cost_per_token, 1e-5);
        assert_eq!(rate.cache_read_cost_per_token, 2e-6);
    }
}

#[test]
fn missing_cache_rates_use_standard_discount_multipliers() {
    // No published cache pricing → derive the standard discounts
    // (read 0.1×, write 1.25× input) rather than billing cache reads
    // at the full input rate.
    let doc = serde_json::json!({
        "model-z": {
            "input_cost_per_token": 1e-5,
            "output_cost_per_token": 5e-5
        }
    });
    let table = parse_rate_table(&doc);
    let rate = table.get("model-z").unwrap();
    assert!((rate.cache_read_cost_per_token - 1e-6).abs() < 1e-18);
    assert!((rate.cache_creation_cost_per_token - 1.25e-5).abs() < 1e-18);
}

#[test]
fn normalize_strips_provider_prefix() {
    assert_eq!(
        normalize_model_name("Anthropic/Claude-Opus-5"),
        "claude-opus-5"
    );
    assert_eq!(normalize_model_name("  gpt-5.6  "), "gpt-5.6");
}

#[test]
fn reported_cost_wins_over_table() {
    let table = parse_rate_table(&sample_doc());
    let totals = TokenTotals {
        uncached_input_tokens: 100,
        cached_input_tokens: 0,
        cache_creation_tokens: 0,
        output_tokens: 50,
        reasoning_tokens: 0,
    };
    let priced = price_usage(&table, "claude-fable-5", &totals, Some(1.25));
    assert_eq!(priced.cost_source, CostSource::ProviderReported);
    assert_eq!(priced.cost_usd, 1.25);
}

#[test]
fn model_priced_matches_hand_calc() {
    let table = parse_rate_table(&sample_doc());
    let totals = TokenTotals {
        uncached_input_tokens: 100,
        cached_input_tokens: 1000,
        cache_creation_tokens: 10,
        output_tokens: 50,
        reasoning_tokens: 0,
    };
    let priced = price_usage(&table, "claude-fable-5", &totals, None);
    assert_eq!(priced.cost_source, CostSource::ModelPriced);
    let expected = 100.0 * 1e-5 + 1000.0 * 1e-6 + 10.0 * 1.25e-5 + 50.0 * 5e-5;
    assert!((priced.cost_usd - expected).abs() < 1e-12);
    let savings = cache_savings_usd(&table, "claude-fable-5", &totals);
    assert!((savings - 1000.0 * (1e-5 - 1e-6)).abs() < 1e-12);
}

#[test]
fn unavailable_when_no_cache_and_fetch_fails() {
    let home = scratch_dir("usage-rates-miss");
    let snap = with_test_fetch(None, || ensure_rates(&home, 1_000_000, false));
    assert_eq!(snap.status, PricingStatus::Unavailable);
    assert!(snap.table.is_empty());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn uses_disk_cache_within_ttl() {
    let home = scratch_dir("usage-rates-cache");
    let path = rates_cache_path(&home);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let payload = serde_json::json!({
        "fetchedAtMs": 1_000_000,
        "document": sample_doc(),
    });
    std::fs::write(&path, payload.to_string()).unwrap();
    // Fetch would fail; still within TTL → cached.
    let snap = with_test_fetch(None, || ensure_rates(&home, 1_000_000 + 60_000, false));
    assert_eq!(snap.status, PricingStatus::Cached);
    assert!(!snap.table.is_empty());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn fresh_fetch_writes_disk() {
    let _serial = flag_lock().lock().unwrap_or_else(|e| e.into_inner());
    let home = scratch_dir("usage-rates-fresh");
    let snap = with_test_fetch(Some(sample_doc()), || ensure_rates(&home, 5_000_000, false));
    assert_eq!(snap.status, PricingStatus::Fresh);
    assert!(rates_cache_path(&home).is_file());
    let _ = std::fs::remove_dir_all(&home);
}

/// Tests that fetch successfully clear the process-wide early-refresh flag; serialise them.
fn flag_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

#[test]
fn an_unknown_model_keeps_asking_for_an_early_refresh_until_a_fetch_succeeds() {
    let _serial = flag_lock().lock().unwrap_or_else(|e| e.into_inner());
    let home = crate::paths::scratch_dir("rates-refresh");
    let fetched_at = 1_000_000;
    let stale = serde_json::json!({ "fetchedAtMs": fetched_at, "document": sample_doc() });
    std::fs::create_dir_all(home.join(".on-n-off")).unwrap();
    std::fs::write(rates_cache_path(&home), stale.to_string()).unwrap();
    let newer = serde_json::json!({
        "claude-fable-5-1": {"input_cost_per_token": 1e-5, "output_cost_per_token": 5e-5}
    });
    let table = parse_rate_table(&sample_doc());
    let totals = TokenTotals {
        uncached_input_tokens: 10,
        ..TokenTotals::default()
    };

    UNPRICED_SEEN.store(false, Ordering::Release);
    price_usage(&table, "<synthetic>", &totals, None);
    assert!(
        !UNPRICED_SEEN.load(Ordering::Acquire),
        "placeholders never ask for a refresh"
    );
    price_usage(&table, "claude-fable-5-1", &totals, None);
    assert!(UNPRICED_SEEN.load(Ordering::Acquire));

    // Ten minutes later the disk copy still serves, and the request survives that hit…
    let early = with_test_fetch(Some(newer.clone()), || {
        ensure_rates(&home, fetched_at + 10 * 60_000, false)
    });
    assert_eq!(early.status, PricingStatus::Cached);
    assert!(
        UNPRICED_SEEN.load(Ordering::Acquire),
        "a cache hit must not consume the flag"
    );
    // …so two hours later the table is re-fetched, well inside the scheduled day, and the
    // flag clears only now.
    let refetched = with_test_fetch(Some(newer.clone()), || {
        ensure_rates(&home, fetched_at + 2 * 60 * 60_000, false)
    });
    assert_eq!(refetched.status, PricingStatus::Fresh);
    assert!(refetched.table.contains_key("claude-fable-5-1"));
    assert!(!UNPRICED_SEEN.load(Ordering::Acquire));
    // A failed fetch keeps the flag for the next scan.
    UNPRICED_SEEN.store(true, Ordering::Release);
    std::fs::write(rates_cache_path(&home), stale.to_string()).unwrap();
    let failed = with_test_fetch(None, || {
        ensure_rates(&home, fetched_at + 2 * 60 * 60_000, false)
    });
    assert_eq!(failed.status, PricingStatus::Cached);
    assert!(UNPRICED_SEEN.load(Ordering::Acquire));
    UNPRICED_SEEN.store(false, Ordering::Release);

    // Without the flag the day applies; a forced refresh ignores the disk copy.
    let scheduled = with_test_fetch(Some(newer.clone()), || {
        ensure_rates(&home, fetched_at + 2 * 60 * 60_000, false)
    });
    assert_eq!(
        scheduled.status,
        PricingStatus::Cached,
        "the day is not over"
    );
    let forced = with_test_fetch(Some(newer), || ensure_rates(&home, fetched_at + 1, true));
    assert_eq!(forced.status, PricingStatus::Fresh);
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn the_disk_table_is_parsed_once_per_file_version() {
    let home = crate::paths::scratch_dir("rates-memo");
    std::fs::create_dir_all(home.join(".on-n-off")).unwrap();
    let path = rates_cache_path(&home);
    let first = serde_json::json!({ "fetchedAtMs": 1_000_000, "document": sample_doc() });
    std::fs::write(&path, first.to_string()).unwrap();
    let a = with_test_fetch(None, || ensure_rates(&home, 1_000_000 + 1, false));
    let b = with_test_fetch(None, || ensure_rates(&home, 1_000_000 + 2, false));
    assert!(
        Arc::ptr_eq(&a.table, &b.table),
        "an unchanged file reuses the parsed table"
    );
    let mut doc = sample_doc();
    doc["claude-fable-5-1"] =
        serde_json::json!({"input_cost_per_token": 1e-5, "output_cost_per_token": 5e-5});
    let second = serde_json::json!({ "fetchedAtMs": 1_000_000, "document": doc });
    std::fs::write(&path, second.to_string()).unwrap();
    let c = with_test_fetch(None, || ensure_rates(&home, 1_000_000 + 3, false));
    assert!(
        c.table.contains_key("claude-fable-5-1"),
        "a rewritten file is parsed again"
    );
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn bare_family_names_are_unpriced() {
    let table = parse_rate_table(&sample_doc());
    let totals = TokenTotals::default();
    assert_eq!(
        price_usage(&table, "sonnet", &totals, None).cost_source,
        CostSource::Unpriced
    );
}
