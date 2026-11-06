use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use super::{
    parse_timestamp_ms, positive_int, CopyKeys, Format, LineReader, TokenTotals, TranscriptDirs,
    UsageRecord,
};
use crate::paths::codex_root_for;
use crate::provider_catalogue::UsageProvider;

pub(super) struct Codex;

impl Format for Codex {
    fn dirs(&self, home: &Path) -> TranscriptDirs {
        let root = codex_root_for(home);
        TranscriptDirs {
            reported: root.join("sessions"),
            more: vec![root.join("archived_sessions")],
        }
    }

    fn markers(&self) -> &'static [&'static str] {
        &["\"token_count\"", "\"turn_context\"", "\"session_meta\""]
    }

    fn line_reader(&self) -> LineReader {
        let mut state = ScanState::default();
        Box::new(move |line| parse_line(line, &mut state))
    }

    fn copy_keys(&self) -> CopyKeys {
        let mut occurrences: HashMap<String, u32> = HashMap::new();
        Box::new(move |record| {
            if record.session_id.is_empty() {
                return None;
            }
            let event = event_key(record);
            let occurrence = occurrences.entry(event.clone()).or_insert(0);
            *occurrence += 1;
            Some(format!("{event}#{occurrence}"))
        })
    }
}

fn event_key(record: &UsageRecord) -> String {
    let t = &record.totals;
    format!(
        "{}\0{}\0{}\0{}:{}:{}:{}:{}:{}",
        record.session_id,
        record.timestamp_ms,
        record.model,
        t.uncached_input_tokens,
        t.cached_input_tokens,
        t.cache_creation_tokens,
        t.cache_creation_1h_tokens,
        t.output_tokens,
        t.reasoning_tokens
    )
}

const FORK_COPY_MAX_GAP_MS: i64 = 1000;

#[derive(Debug, Clone, Default)]
struct ScanState {
    model: String,
    session_id: String,
    last_usage_signature: Option<String>,
    saw_session_meta: bool,
    suppressing_fork_copies: bool,
    fork_copy_anchor_ms: i64,
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

fn parse_line(line: &str, state: &mut ScanState) -> Option<UsageRecord> {
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
        copy_key: None,
    })
}

#[cfg(test)]
mod tests;
