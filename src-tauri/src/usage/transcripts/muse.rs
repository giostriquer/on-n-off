use std::path::Path;

use serde_json::Value;

use super::{
    positive_int, record_key, CopyKeys, Format, LineReader, TokenTotals, TranscriptDirs,
    UsageRecord,
};
use crate::paths::muse_data_for;
use crate::provider_catalogue::UsageProvider;

pub(super) struct Muse;

impl Format for Muse {
    fn dirs(&self, home: &Path) -> TranscriptDirs {
        TranscriptDirs::only(muse_data_for(home).join("sessions"))
    }

    fn markers(&self) -> &'static [&'static str] {
        &["\"model_completed\""]
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
    let event = obj.get("payload")?.get("event")?.as_object()?;
    if event.get("kind").and_then(|v| v.as_str()) != Some("model_completed") {
        return None;
    }
    let usage = event.get("usage")?.as_object()?;
    let model = event.get("model")?.as_str()?.to_string();
    if model.is_empty() {
        return None;
    }
    let timestamp_ms = obj.get("recorded_at").and_then(|v| v.as_i64())? / 1_000;
    let session_id = obj
        .get("stream")
        .and_then(|stream| stream.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let copy_key = obj
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string);

    let get = |key: &str| positive_int(usage.get(key).unwrap_or(&Value::Null));
    let input_tokens = get("input_tokens");
    let cache_read_tokens = match usage.get("cache_read_tokens") {
        Some(_) => get("cache_read_tokens"),
        None => get("cached_tokens"),
    };
    let cache_creation_tokens = get("cache_write_tokens");
    let output_tokens = get("output_tokens");
    let totals = TokenTotals {
        uncached_input_tokens: input_tokens
            .saturating_sub(cache_read_tokens)
            .saturating_sub(cache_creation_tokens),
        cached_input_tokens: cache_read_tokens,
        cache_creation_tokens,
        cache_creation_1h_tokens: 0,
        output_tokens,
        reasoning_tokens: get("reasoning_tokens").min(output_tokens),
    };
    if totals.total_tokens() == 0 {
        return None;
    }

    Some(UsageRecord {
        provider: UsageProvider::Muse,
        timestamp_ms,
        model,
        session_id,
        totals,
        reported_cost_usd: None,
        copy_key,
    })
}

#[cfg(test)]
mod tests;
