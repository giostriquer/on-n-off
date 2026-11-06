use std::path::Path;

use serde_json::Value;

use super::{
    parse_timestamp_ms, positive_int, record_key, CopyKeys, Format, LineReader, TokenTotals,
    TranscriptDirs, UsageRecord,
};
use crate::paths::claude_root_for;
use crate::provider_catalogue::UsageProvider;

pub(super) struct Claude;

impl Format for Claude {
    fn dirs(&self, home: &Path) -> TranscriptDirs {
        let nested = claude_root_for(home).join("projects");
        let flat = home.join("projects");
        TranscriptDirs::only(if !nested.is_dir() && flat.is_dir() {
            flat
        } else {
            nested
        })
    }

    fn markers(&self) -> &'static [&'static str] {
        &["\"usage\""]
    }

    fn line_reader(&self) -> LineReader {
        Box::new(parse_line)
    }

    fn copy_keys(&self) -> CopyKeys {
        Box::new(record_key)
    }
}

fn parse_line(line: &str) -> Option<UsageRecord> {
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
    let copy_key = match (message_id, request_id) {
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
        copy_key,
    })
}

#[cfg(test)]
mod tests;
