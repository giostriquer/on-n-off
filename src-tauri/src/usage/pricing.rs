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
mod tests;
