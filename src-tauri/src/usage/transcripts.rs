//! Pure parsers for Claude Code / Codex session transcripts.
//!
//! Line-at-a-time so callers can stream large files. Port of T3 Code's
//! `usageTranscripts.ts` (algorithm only).

use serde_json::Value;

/// Increment whenever transcript parsing semantics change.
/// v2: the one-hour share of Claude cache writes, and zero-token Claude lines dropped.
pub const USAGE_TRANSCRIPT_PARSER_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsageProvider {
    Claude,
    Codex,
}

impl UsageProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TokenTotals {
    pub uncached_input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_tokens: u64,
    /// The part of `cache_creation_tokens` written with a one-hour lifetime, which Anthropic bills
    /// at 2x input against the five-minute write's 1.25x. Never more than `cache_creation_tokens`.
    pub cache_creation_1h_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
}

impl TokenTotals {
    pub fn total_tokens(&self) -> u64 {
        // reasoning is a subset of output — do not add again.
        self.uncached_input_tokens
            + self.cached_input_tokens
            + self.cache_creation_tokens
            + self.output_tokens
    }

    pub fn add(&self, other: &Self) -> Self {
        Self {
            uncached_input_tokens: self.uncached_input_tokens + other.uncached_input_tokens,
            cached_input_tokens: self.cached_input_tokens + other.cached_input_tokens,
            cache_creation_tokens: self.cache_creation_tokens + other.cache_creation_tokens,
            cache_creation_1h_tokens: self.cache_creation_1h_tokens
                + other.cache_creation_1h_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            reasoning_tokens: self.reasoning_tokens + other.reasoning_tokens,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageRecord {
    pub provider: UsageProvider,
    pub timestamp_ms: i64,
    pub model: String,
    pub session_id: String,
    pub totals: TokenTotals,
    pub reported_cost_usd: Option<f64>,
    /// Cross-file de-duplication key; `None` means unique / no dedupe.
    pub dedupe_key: Option<String>,
}

impl UsageRecord {
    /// Whether this copy of a de-duplicated record should replace `kept`. Claude Code writes one
    /// line per content block of a message, and the early lines carry a partial `output_tokens`
    /// (often 1 for a thinking block), so the copy with the most output is the message as billed.
    /// Ties keep the copy already held.
    pub fn is_richer_than(&self, kept: &Self) -> bool {
        (self.totals.output_tokens, self.totals.total_tokens())
            > (kept.totals.output_tokens, kept.totals.total_tokens())
    }
}

pub fn might_carry_usage(line: &str, provider: UsageProvider) -> bool {
    match provider {
        UsageProvider::Claude => line.contains("\"usage\""),
        UsageProvider::Codex => line.contains("\"token_count\""),
    }
}

fn positive_int(value: &Value) -> u64 {
    match value {
        Value::Number(n) => n
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.0)
            .map(|v| v.trunc() as u64)
            .unwrap_or(0),
        _ => 0,
    }
}

fn parse_timestamp_ms(value: &Value) -> Option<i64> {
    let raw = value.as_str()?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp_millis())
        .or_else(|| {
            // Some transcripts omit offset; try appending Z.
            chrono::DateTime::parse_from_rfc3339(&format!("{raw}Z"))
                .ok()
                .map(|dt| dt.timestamp_millis())
        })
}

/* -------------------------------------------------------------------------- */
/* Claude                                                                     */
/* -------------------------------------------------------------------------- */

pub fn parse_claude_line(line: &str) -> Option<UsageRecord> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    let obj = parsed.as_object()?;
    if obj.get("type").and_then(|v| v.as_str()) != Some("assistant") {
        return None;
    }
    let message = obj.get("message")?.as_object()?;
    let usage = message.get("usage")?.as_object()?;
    let timestamp_ms = parse_timestamp_ms(obj.get("timestamp")?)?;
    let model = message.get("model")?.as_str()?.to_string();
    if model.is_empty() {
        return None;
    }

    let message_id = message.get("id").and_then(|v| v.as_str());
    let request_id = obj.get("requestId").and_then(|v| v.as_str());
    let dedupe_key = match (message_id, request_id) {
        (None, None) => None,
        (mid, rid) => Some(format!("{}:{}", mid.unwrap_or(""), rid.unwrap_or(""))),
    };

    let reported_cost_usd = obj
        .get("costUSD")
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite());

    let cache_creation_tokens = positive_int(
        usage
            .get("cache_creation_input_tokens")
            .unwrap_or(&Value::Null),
    );
    let cache_creation_1h_tokens = usage
        .get("cache_creation")
        .and_then(|split| split.get("ephemeral_1h_input_tokens"))
        .map_or(0, positive_int)
        .min(cache_creation_tokens);
    let totals = TokenTotals {
        uncached_input_tokens: positive_int(usage.get("input_tokens").unwrap_or(&Value::Null)),
        cached_input_tokens: positive_int(
            usage.get("cache_read_input_tokens").unwrap_or(&Value::Null),
        ),
        cache_creation_tokens,
        cache_creation_1h_tokens,
        output_tokens: positive_int(usage.get("output_tokens").unwrap_or(&Value::Null)),
        reasoning_tokens: 0,
    };
    // Locally generated lines (API errors, interrupts) come as `<synthetic>` with zero usage.
    if totals.total_tokens() == 0 {
        return None;
    }

    Some(UsageRecord {
        provider: UsageProvider::Claude,
        timestamp_ms,
        model,
        session_id: obj
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        totals,
        reported_cost_usd,
        dedupe_key,
    })
}

/* -------------------------------------------------------------------------- */
/* Codex                                                                      */
/* -------------------------------------------------------------------------- */

const FORK_COPY_MAX_GAP_MS: i64 = 1000;

#[derive(Debug, Clone)]
pub struct CodexScanState {
    pub model: String,
    pub session_id: String,
    last_usage_signature: Option<String>,
    saw_session_meta: bool,
    suppressing_fork_copies: bool,
    fork_copy_anchor_ms: i64,
}

impl Default for CodexScanState {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexScanState {
    pub fn new() -> Self {
        Self {
            model: String::new(),
            session_id: String::new(),
            last_usage_signature: None,
            saw_session_meta: false,
            suppressing_fork_copies: false,
            fork_copy_anchor_ms: 0,
        }
    }
}

fn is_forked_session_meta(payload: &serde_json::Map<String, Value>) -> bool {
    if payload
        .get("forked_from_id")
        .and_then(|v| v.as_str())
        .is_some()
    {
        return true;
    }
    let Some(source) = payload.get("source").and_then(|v| v.as_object()) else {
        return false;
    };
    let Some(subagent) = source.get("subagent").and_then(|v| v.as_object()) else {
        return false;
    };
    let Some(spawn) = subagent.get("thread_spawn").and_then(|v| v.as_object()) else {
        return false;
    };
    spawn
        .get("parent_thread_id")
        .and_then(|v| v.as_str())
        .is_some()
}

pub fn parse_codex_line(line: &str, state: &mut CodexScanState) -> Option<UsageRecord> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    let obj = parsed.as_object()?;
    let payload = obj.get("payload")?.as_object()?;
    let record_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if record_type == "session_meta" {
        if state.saw_session_meta {
            return None;
        }
        state.saw_session_meta = true;
        if let Some(id) = payload
            .get("id")
            .or_else(|| payload.get("session_id"))
            .and_then(|v| v.as_str())
        {
            state.session_id = id.to_string();
        }
        if let Some(meta_ts) = obj.get("timestamp").and_then(parse_timestamp_ms) {
            if is_forked_session_meta(payload) {
                state.suppressing_fork_copies = true;
                state.fork_copy_anchor_ms = meta_ts;
            }
        }
        return None;
    }

    if record_type == "turn_context" {
        if let Some(model) = payload.get("model").and_then(|v| v.as_str()) {
            state.model = model.to_string();
        }
        return None;
    }

    if payload.get("type").and_then(|v| v.as_str()) != Some("token_count") {
        return None;
    }

    let info = payload.get("info")?.as_object()?;
    let last = info.get("last_token_usage")?.as_object()?;
    let timestamp_ms = parse_timestamp_ms(obj.get("timestamp")?)?;
    if state.model.is_empty() {
        return None;
    }

    let signature = serde_json::to_string(last).ok()?;
    if state.last_usage_signature.as_deref() == Some(signature.as_str()) {
        return None;
    }
    state.last_usage_signature = Some(signature);

    if state.suppressing_fork_copies {
        if timestamp_ms - state.fork_copy_anchor_ms < FORK_COPY_MAX_GAP_MS {
            state.fork_copy_anchor_ms = timestamp_ms;
            return None;
        }
        state.suppressing_fork_copies = false;
    }

    let input_tokens = positive_int(last.get("input_tokens").unwrap_or(&Value::Null));
    let cached_input_tokens = positive_int(last.get("cached_input_tokens").unwrap_or(&Value::Null));
    let cache_creation_tokens =
        positive_int(last.get("cache_write_input_tokens").unwrap_or(&Value::Null));
    let output_tokens = positive_int(last.get("output_tokens").unwrap_or(&Value::Null));
    let reasoning_raw = positive_int(last.get("reasoning_output_tokens").unwrap_or(&Value::Null));

    let totals = TokenTotals {
        uncached_input_tokens: input_tokens
            .saturating_sub(cached_input_tokens)
            .saturating_sub(cache_creation_tokens),
        cached_input_tokens,
        cache_creation_tokens,
        cache_creation_1h_tokens: 0,
        output_tokens,
        reasoning_tokens: reasoning_raw.min(output_tokens),
    };
    if totals.total_tokens() == 0 {
        return None;
    }

    Some(UsageRecord {
        provider: UsageProvider::Codex,
        timestamp_ms,
        model: state.model.clone(),
        session_id: state.session_id.clone(),
        totals,
        reported_cost_usd: None,
        dedupe_key: None,
    })
}

#[cfg(test)]
mod tests;
