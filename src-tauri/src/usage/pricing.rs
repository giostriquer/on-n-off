//! LiteLLM rate table parse + cost arithmetic (pure).
//!
//! Fetch/cache of the JSON lives in `ensure_rates` at the bottom — same URL
//! `ccusage` / T3 Code use.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;

use super::transcripts::TokenTotals;

pub const LITELLM_RATES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

const RATES_TTL_MS: i64 = 24 * 60 * 60 * 1000;

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
    pub table: RateTable,
    pub status: PricingStatus,
    pub fetched_at_ms: Option<i64>,
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

/// Drop half-priced models; fall back cache rates to input when omitted.
pub fn parse_rate_table(document: &Value) -> RateTable {
    let mut table = RateTable::new();
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
        let cache_read = entry
            .get("cache_read_input_token_cost")
            .and_then(finite_number)
            .unwrap_or(input);
        let cache_creation = entry
            .get("cache_creation_input_token_cost")
            .and_then(finite_number)
            .unwrap_or(input);
        table.insert(
            normalize_model_name(name),
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

/// Load rates from disk / network. Never fails the scan — empty table + unavailable.
pub fn ensure_rates(home: &Path, now_ms: i64) -> RatesSnapshot {
    let path = rates_cache_path(home);
    let mut table = RateTable::new();
    let mut fetched_at_ms: Option<i64> = None;
    let mut status = PricingStatus::Unavailable;

    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(doc) = serde_json::from_str::<Value>(&raw) {
            let disk_at = doc.get("fetchedAtMs").and_then(|v| v.as_i64());
            let document = doc.get("document").cloned().unwrap_or(Value::Null);
            let parsed = parse_rate_table(&document);
            if !parsed.is_empty() {
                table = parsed;
                fetched_at_ms = disk_at;
                status = PricingStatus::Cached;
                if let Some(at) = disk_at {
                    if now_ms - at < RATES_TTL_MS {
                        return RatesSnapshot {
                            table,
                            status,
                            fetched_at_ms,
                        };
                    }
                }
            }
        }
    }

    if let Some(document) = fetch_rates_json() {
        let parsed = parse_rate_table(&document);
        if !parsed.is_empty() {
            table = parsed;
            fetched_at_ms = Some(now_ms);
            status = PricingStatus::Fresh;
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
            return RatesSnapshot {
                table,
                status,
                fetched_at_ms,
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
        // Second insert overwrites — both normalize to same key; either ok as long as present.
        assert!(rate.input_cost_per_token > 0.0);
        assert!(rate.output_cost_per_token > 0.0);
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
        let snap = with_test_fetch(None, || ensure_rates(&home, 1_000_000));
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
        let snap = with_test_fetch(None, || ensure_rates(&home, 1_000_000 + 60_000));
        assert_eq!(snap.status, PricingStatus::Cached);
        assert!(!snap.table.is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn fresh_fetch_writes_disk() {
        let home = scratch_dir("usage-rates-fresh");
        let snap = with_test_fetch(Some(sample_doc()), || ensure_rates(&home, 5_000_000));
        assert_eq!(snap.status, PricingStatus::Fresh);
        assert!(rates_cache_path(&home).is_file());
        let _ = std::fs::remove_dir_all(&home);
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
