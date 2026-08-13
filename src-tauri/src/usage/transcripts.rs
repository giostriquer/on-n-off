//! Pure parsers for Claude Code / Codex session transcripts.
//!
//! Line-at-a-time so callers can stream large files. Port of T3 Code's
//! `usageTranscripts.ts` (algorithm only).

use serde_json::Value;

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

    Some(UsageRecord {
        provider: UsageProvider::Claude,
        timestamp_ms,
        model,
        session_id: obj
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        totals: TokenTotals {
            uncached_input_tokens: positive_int(usage.get("input_tokens").unwrap_or(&Value::Null)),
            cached_input_tokens: positive_int(
                usage.get("cache_read_input_tokens").unwrap_or(&Value::Null),
            ),
            cache_creation_tokens: positive_int(
                usage
                    .get("cache_creation_input_tokens")
                    .unwrap_or(&Value::Null),
            ),
            output_tokens: positive_int(usage.get("output_tokens").unwrap_or(&Value::Null)),
            reasoning_tokens: 0,
        },
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
mod tests {
    use super::*;

    fn claude_line(message_id: &str, content_type: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": "2026-08-07T04:05:13.944Z",
            "sessionId": "5a128faa-8253-489e-b935-6c08e8e670c0",
            "message": {
                "id": message_id,
                "role": "assistant",
                "model": "claude-fable-5",
                "content": [{ "type": content_type }],
                "usage": {
                    "input_tokens": 2,
                    "cache_creation_input_tokens": 66818,
                    "cache_read_input_tokens": 1000,
                    "output_tokens": 286
                }
            }
        })
        .to_string()
    }

    #[test]
    fn parse_claude_extracts_totals_and_dedupe_key() {
        let record = parse_claude_line(&claude_line("msg_1", "text")).expect("record");
        assert_eq!(record.provider, UsageProvider::Claude);
        assert_eq!(record.model, "claude-fable-5");
        assert_eq!(record.totals.uncached_input_tokens, 2);
        assert_eq!(record.totals.cached_input_tokens, 1000);
        assert_eq!(record.totals.cache_creation_tokens, 66818);
        assert_eq!(record.totals.output_tokens, 286);
        assert_eq!(record.dedupe_key.as_deref(), Some("msg_1:"));
    }

    #[test]
    fn parse_claude_same_dedupe_key_across_content_blocks() {
        let text = parse_claude_line(&claude_line("msg_2", "text")).unwrap();
        let tool = parse_claude_line(&claude_line("msg_2", "tool_use")).unwrap();
        assert_eq!(text.dedupe_key, tool.dedupe_key);
        assert_eq!(text.totals, tool.totals);
    }

    #[test]
    fn parse_claude_ignores_non_assistant_and_garbage() {
        assert!(parse_claude_line(r#"{"type":"user","message":{}}"#).is_none());
        assert!(parse_claude_line("not json").is_none());
    }

    fn session_meta() -> String {
        serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-08-01T05:17:41.289Z",
            "payload": { "type": "session_meta", "id": "019fbbc1-b12c-7360-a685-28c181f0025f" }
        })
        .to_string()
    }

    fn turn_context() -> String {
        serde_json::json!({
            "type": "turn_context",
            "timestamp": "2026-08-01T05:17:42.694Z",
            "payload": { "type": "turn_context", "model": "gpt-5.6-sol" }
        })
        .to_string()
    }

    fn token_count(input: u64, cached: u64, output: u64, reasoning: u64) -> String {
        serde_json::json!({
            "type": "event_msg",
            "timestamp": "2026-08-01T05:17:49.919Z",
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {
                        "input_tokens": input,
                        "cached_input_tokens": cached,
                        "cache_write_input_tokens": 0,
                        "output_tokens": output,
                        "reasoning_output_tokens": reasoning
                    }
                }
            }
        })
        .to_string()
    }

    #[test]
    fn parse_codex_attributes_model_from_turn_context() {
        let mut state = CodexScanState::new();
        parse_codex_line(&session_meta(), &mut state);
        parse_codex_line(&turn_context(), &mut state);
        let record = parse_codex_line(&token_count(19239, 11008, 299, 116), &mut state).unwrap();
        assert_eq!(record.provider, UsageProvider::Codex);
        assert_eq!(record.model, "gpt-5.6-sol");
        assert_eq!(record.session_id, "019fbbc1-b12c-7360-a685-28c181f0025f");
        assert_eq!(record.totals.uncached_input_tokens, 19239 - 11008);
        assert_eq!(record.totals.cached_input_tokens, 11008);
        assert_eq!(record.totals.reasoning_tokens, 116);
    }

    #[test]
    fn parse_codex_skips_repeated_token_count() {
        let mut state = CodexScanState::new();
        parse_codex_line(&turn_context(), &mut state);
        assert!(parse_codex_line(&token_count(100, 0, 10, 0), &mut state).is_some());
        assert!(parse_codex_line(&token_count(100, 0, 10, 0), &mut state).is_none());
    }

    #[test]
    fn parse_codex_drops_usage_before_model() {
        let mut state = CodexScanState::new();
        assert!(parse_codex_line(&token_count(100, 0, 10, 0), &mut state).is_none());
    }

    #[test]
    fn parse_codex_pre_model_event_does_not_poison_signature() {
        let mut state = CodexScanState::new();
        assert!(parse_codex_line(&token_count(100, 0, 10, 0), &mut state).is_none());
        parse_codex_line(&turn_context(), &mut state);
        assert!(parse_codex_line(&token_count(100, 0, 10, 0), &mut state).is_some());
    }

    fn forked_meta(id: &str, timestamp: &str, forked_from: Option<&str>) -> String {
        let mut payload = serde_json::json!({
            "type": "session_meta",
            "id": id
        });
        if let Some(parent) = forked_from {
            payload["forked_from_id"] = serde_json::json!(parent);
        }
        serde_json::json!({
            "type": "session_meta",
            "timestamp": timestamp,
            "payload": payload
        })
        .to_string()
    }

    fn stamp(timestamp: &str, line: &str) -> String {
        let mut parsed: Value = serde_json::from_str(line).unwrap();
        parsed["timestamp"] = Value::String(timestamp.to_string());
        parsed.to_string()
    }

    #[test]
    fn parse_codex_keeps_child_session_over_ancestor_metas() {
        let mut state = CodexScanState::new();
        parse_codex_line(
            &forked_meta("child", "2026-08-01T05:00:00.000Z", None),
            &mut state,
        );
        parse_codex_line(
            &forked_meta("parent", "2026-08-01T05:00:00.000Z", None),
            &mut state,
        );
        parse_codex_line(&turn_context(), &mut state);
        let record = parse_codex_line(&token_count(100, 0, 10, 0), &mut state).unwrap();
        assert_eq!(record.session_id, "child");
    }

    #[test]
    fn parse_codex_drops_fork_copy_burst() {
        let mut state = CodexScanState::new();
        let fork = "2026-08-01T05:00:00.000Z";
        parse_codex_line(&forked_meta("child", fork, Some("parent")), &mut state);
        parse_codex_line(&forked_meta("parent", fork, None), &mut state);
        parse_codex_line(&stamp(fork, &turn_context()), &mut state);
        assert!(parse_codex_line(
            &stamp("2026-08-01T05:00:00.001Z", &token_count(100, 0, 10, 0)),
            &mut state
        )
        .is_none());
        assert!(parse_codex_line(
            &stamp("2026-08-01T05:00:00.002Z", &token_count(200, 0, 20, 0)),
            &mut state
        )
        .is_none());
        let real = parse_codex_line(
            &stamp("2026-08-01T05:00:06.000Z", &token_count(300, 0, 30, 0)),
            &mut state,
        )
        .unwrap();
        assert_eq!(real.totals.output_tokens, 30);
    }

    #[test]
    fn might_carry_usage_gates() {
        assert!(might_carry_usage(r#"{"usage":{}}"#, UsageProvider::Claude));
        assert!(!might_carry_usage(r#"{"foo":1}"#, UsageProvider::Claude));
        assert!(might_carry_usage(
            r#"{"payload":{"type":"token_count"}}"#,
            UsageProvider::Codex
        ));
    }
}
