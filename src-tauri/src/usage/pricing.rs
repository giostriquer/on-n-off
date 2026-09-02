//! LiteLLM rate table parse + cost arithmetic (pure).
//!
//! Fetch/cache of the JSON lives in `ensure_rates` at the bottom — same URL
//! `ccusage` / T3 Code use.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use super::transcripts::TokenTotals;

pub const LITELLM_RATES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

const RATES_TTL_MS: i64 = 24 * 60 * 60 * 1000;
/// How often the table may be re-fetched because a scan met a model it could not price: a
/// model released today shows up in LiteLLM within hours, not a day.
const RATES_EARLY_TTL_MS: i64 = 60 * 60 * 1000;

/// Set when a scan prices a model the table does not carry; `ensure_rates` then re-fetches after
/// the shorter TTL and clears the flag once a fetch succeeds, so it survives any number of
/// summary-cache hits in between.
static UNPRICED_SEEN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelRate {
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
    pub cache_read_cost_per_token: f64,
    pub cache_creation_cost_per_token: f64,
}

pub type RateTable = HashMap<String, ModelRate>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostSource {
    ProviderReported,
    ModelPriced,
    Unpriced,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricedUsage {
    pub cost_usd: f64,
    pub cost_source: CostSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingStatus {
    Fresh,
    Cached,
    Unavailable,
}

impl PricingStatus {
    pub fn to_dto(self) -> crate::dto::UsagePricingStatus {
        match self {
            Self::Fresh => crate::dto::UsagePricingStatus::Fresh,
            Self::Cached => crate::dto::UsagePricingStatus::Cached,
            Self::Unavailable => crate::dto::UsagePricingStatus::Unavailable,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RatesSnapshot {
    pub table: Arc<RateTable>,
    pub status: PricingStatus,
    pub fetched_at_ms: Option<i64>,
}

/// The parsed on-disk table, keyed on the file's identity so the summary fast path never
/// re-parses a multi-megabyte document it has already seen.
struct RatesMemo {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    fetched_at_ms: Option<i64>,
    table: Arc<RateTable>,
}

static RATES_MEMO: Mutex<Option<RatesMemo>> = Mutex::new(None);

fn file_identity(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

/// The on-disk table (memoised) with the instant it was fetched, if the file parses.
fn load_disk_rates(path: &Path) -> Option<(Arc<RateTable>, Option<i64>)> {
    let (len, modified) = file_identity(path)?;
    {
        let memo = RATES_MEMO
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = memo.as_ref() {
            if entry.path == path && entry.len == len && entry.modified == modified {
                return Some((entry.table.clone(), entry.fetched_at_ms));
            }
        }
    }
    let raw = std::fs::read_to_string(path).ok()?;
    let doc = serde_json::from_str::<Value>(&raw).ok()?;
    let fetched_at_ms = doc.get("fetchedAtMs").and_then(|v| v.as_i64());
    let parsed = parse_rate_table(doc.get("document").unwrap_or(&Value::Null));
    if parsed.is_empty() {
        return None;
    }
    let table = Arc::new(parsed);
    remember_rates(path, len, modified, fetched_at_ms, table.clone());
    Some((table, fetched_at_ms))
}

fn remember_rates(
    path: &Path,
    len: u64,
    modified: Option<SystemTime>,
    fetched_at_ms: Option<i64>,
    table: Arc<RateTable>,
) {
    *RATES_MEMO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(RatesMemo {
        path: path.to_path_buf(),
        len,
        modified,
        fetched_at_ms,
        table,
    });
}

fn finite_number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|v| v.is_finite())
}

/// Strip `provider/` prefix and lowercase for table lookup.
pub fn normalize_model_name(model: &str) -> String {
    let trimmed = model.trim().to_lowercase();
    match trimmed.rfind('/') {
        Some(i) => trimmed[i + 1..].to_string(),
        None => trimmed,
    }
}

static UNPRICEABLE: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn unpriceable_models() -> &'static HashSet<&'static str> {
    UNPRICEABLE.get_or_init(|| {
        HashSet::from([
            "<synthetic>",
            "synthetic",
            "opus",
            "sonnet",
            "haiku",
            "fable",
        ])
    })
}

/// Cache rates omitted from an entry derive the standard discounts (read
/// 0.1×, write 1.25× input) — ccusage's defaults — never the full input rate.
/// The multipliers are Anthropic-shaped; OpenAI bills cache writes at the
/// plain input rate, but its entries publish cache pricing and Codex reports
/// zero write tokens, so the 1.25× write default has no priced traffic.
const DERIVED_CACHE_READ_MULTIPLIER: f64 = 0.1;
const DERIVED_CACHE_CREATION_MULTIPLIER: f64 = 1.25;

/// Drop half-priced models; derive omitted cache rates. Several LiteLLM keys
/// can normalize to one model name (`claude-fable-5`, `vertex_ai/…`,
/// `deepinfra/anthropic/…`), and reseller entries often omit cache pricing,
/// so collisions resolve by rank instead of document order: the canonical
/// bare key wins, then prefixed entries carrying published cache pricing.
pub fn parse_rate_table(document: &Value) -> RateTable {
    let mut table = RateTable::new();
    let mut ranks: HashMap<String, u8> = HashMap::new();
    let Some(obj) = document.as_object() else {
        return table;
    };
    for (name, raw) in obj {
        let Some(entry) = raw.as_object() else {
            continue;
        };
        let Some(input) = entry.get("input_cost_per_token").and_then(finite_number) else {
            continue;
        };
        let Some(output) = entry.get("output_cost_per_token").and_then(finite_number) else {
            continue;
        };
        let explicit_cache_read = entry
            .get("cache_read_input_token_cost")
            .and_then(finite_number);
        let cache_read = explicit_cache_read.unwrap_or(input * DERIVED_CACHE_READ_MULTIPLIER);
        let cache_creation = entry
            .get("cache_creation_input_token_cost")
            .and_then(finite_number)
            .unwrap_or(input * DERIVED_CACHE_CREATION_MULTIPLIER);
        let rank = if !name.contains('/') {
            2 // canonical bare key
        } else if explicit_cache_read.is_some() {
            1 // prefixed, but carries published cache pricing
        } else {
            0 // prefixed reseller without cache pricing
        };
        let key = normalize_model_name(name);
        if ranks.get(&key).is_some_and(|&existing| existing >= rank) {
            continue;
        }
        ranks.insert(key.clone(), rank);
        table.insert(
            key,
            ModelRate {
                input_cost_per_token: input,
                output_cost_per_token: output,
                cache_read_cost_per_token: cache_read,
                cache_creation_cost_per_token: cache_creation,
            },
        );
    }
    table
}

pub fn lookup_rate<'a>(table: &'a RateTable, model: &str) -> Option<&'a ModelRate> {
    let normalized = normalize_model_name(model);
    if normalized.is_empty() || unpriceable_models().contains(normalized.as_str()) {
        return None;
    }
    table.get(&normalized)
}

pub fn price_usage(
    table: &RateTable,
    model: &str,
    totals: &TokenTotals,
    reported_cost_usd: Option<f64>,
) -> PricedUsage {
    if let Some(cost) = reported_cost_usd.filter(|v| v.is_finite()) {
        return PricedUsage {
            cost_usd: cost,
            cost_source: CostSource::ProviderReported,
        };
    }
    let Some(rate) = lookup_rate(table, model) else {
        let normalized = normalize_model_name(model);
        if !normalized.is_empty() && !unpriceable_models().contains(normalized.as_str()) {
            UNPRICED_SEEN.store(true, Ordering::Release);
        }
        return PricedUsage {
            cost_usd: 0.0,
            cost_source: CostSource::Unpriced,
        };
    };
    let cost_usd = totals.uncached_input_tokens as f64 * rate.input_cost_per_token
        + totals.cached_input_tokens as f64 * rate.cache_read_cost_per_token
        + totals.cache_creation_tokens as f64 * rate.cache_creation_cost_per_token
        + totals.output_tokens as f64 * rate.output_cost_per_token;
    PricedUsage {
        cost_usd,
        cost_source: CostSource::ModelPriced,
    }
}

pub fn cache_savings_usd(table: &RateTable, model: &str, totals: &TokenTotals) -> f64 {
    let Some(rate) = lookup_rate(table, model) else {
        return 0.0;
    };
    totals.cached_input_tokens as f64 * (rate.input_cost_per_token - rate.cache_read_cost_per_token)
}

pub fn rates_cache_path(home: &Path) -> PathBuf {
    home.join(".on-n-off").join("usage-model-rates.json")
}

fn fetch_rates_json() -> Option<Value> {
    #[cfg(test)]
    if let Some(override_result) = test_fetch_override() {
        return override_result;
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .user_agent("on-n-off/0.1")
        .build();
    let body = agent
        .get(LITELLM_RATES_URL)
        .call()
        .ok()?
        .into_string()
        .ok()?;
    serde_json::from_str(&body).ok()
}

#[cfg(test)]
fn test_fetch_override() -> Option<Option<Value>> {
    TEST_FETCH.with(|cell| cell.borrow().clone())
}

#[cfg(test)]
thread_local! {
    static TEST_FETCH: std::cell::RefCell<Option<Option<Value>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn with_test_fetch<R>(response: Option<Value>, f: impl FnOnce() -> R) -> R {
    TEST_FETCH.with(|cell| {
        *cell.borrow_mut() = Some(response);
    });
    let result = f();
    TEST_FETCH.with(|cell| {
        *cell.borrow_mut() = None;
    });
    result
}

/// Load rates from disk / network. Never fails the scan — empty table + unavailable. The disk
/// copy is reused for a day, for an hour once a scan has met a model it could not price, and
/// not at all when the user asked for a refresh (`force`).
pub fn ensure_rates(home: &Path, now_ms: i64, force: bool) -> RatesSnapshot {
    let path = rates_cache_path(home);
    let ttl_ms = if force {
        0
    } else if UNPRICED_SEEN.load(Ordering::Acquire) {
        RATES_EARLY_TTL_MS
    } else {
        RATES_TTL_MS
    };
    let mut table = Arc::new(RateTable::new());
    let mut fetched_at_ms: Option<i64> = None;
    let mut status = PricingStatus::Unavailable;

    if let Some((disk_table, disk_at)) = load_disk_rates(&path) {
        table = disk_table;
        fetched_at_ms = disk_at;
        status = PricingStatus::Cached;
        if let Some(at) = disk_at {
            if now_ms - at < ttl_ms {
                return RatesSnapshot {
                    table,
                    status,
                    fetched_at_ms,
                };
            }
        }
    }

    if let Some(document) = fetch_rates_json() {
        let parsed = parse_rate_table(&document);
        if !parsed.is_empty() {
            let payload = serde_json::json!({
                "fetchedAtMs": now_ms,
                "document": document,
            });
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(raw) = serde_json::to_string(&payload) {
                let _ = std::fs::write(&path, raw);
            }
            let table = Arc::new(parsed);
            if let Some((len, modified)) = file_identity(&path) {
                remember_rates(&path, len, modified, Some(now_ms), table.clone());
            }
            UNPRICED_SEEN.store(false, Ordering::Release);
            return RatesSnapshot {
                table,
                status: PricingStatus::Fresh,
                fetched_at_ms: Some(now_ms),
            };
        }
    }

    // Refresh failed; keep stale table as cached if we have one.
    if !table.is_empty() {
        status = PricingStatus::Cached;
    }
    RatesSnapshot {
        table,
        status,
        fetched_at_ms,
    }
}

#[cfg(test)]
mod tests {
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
}
