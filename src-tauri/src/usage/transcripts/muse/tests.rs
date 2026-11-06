use super::super::may_matter;
use super::*;

fn muse_completed_line() -> String {
    serde_json::json!({
        "schema_version": 1,
        "id": "01acme-line-1",
        "stream": {"kind": "session", "id": "01acme-session"},
        "sequence": 65,
        "recorded_at": 1_792_013_706_016_282i64,
        "record_type": "event",
        "payload": {
            "kind": "run",
            "event": {
                "kind": "model_completed",
                "usage": {
                    "input_tokens": 56645,
                    "output_tokens": 337,
                    "cached_tokens": 34292,
                    "cache_write_tokens": 120,
                    "cache_read_tokens": 34292,
                    "reasoning_tokens": 400
                },
                "duration_ms": 900,
                "finish_reason": "tool_calls",
                "model": "muse-acme-1.0"
            }
        }
    })
    .to_string()
}

#[test]
fn parse_muse_counts_model_completed_once_with_cache_split() {
    let record = parse_line(&muse_completed_line()).expect("record");
    assert_eq!(record.provider, UsageProvider::Muse);
    assert_eq!(record.provider.as_str(), "muse");
    assert_eq!(record.model, "muse-acme-1.0");
    assert_eq!(record.session_id, "01acme-session");
    assert_eq!(record.timestamp_ms, 1_792_013_706_016);
    assert_eq!(record.totals.uncached_input_tokens, 56645 - 34292 - 120);
    assert_eq!(record.totals.cached_input_tokens, 34292);
    assert_eq!(record.totals.cache_creation_tokens, 120);
    assert_eq!(record.totals.output_tokens, 337);
    assert_eq!(
        record.totals.reasoning_tokens, 337,
        "reasoning is an output subset"
    );
    assert_eq!(record.copy_key.as_deref(), Some("01acme-line-1"));
    assert_eq!(record.reported_cost_usd, None);
}

#[test]
fn parse_muse_skips_attribution_records_and_other_lines() {
    let attribution = serde_json::json!({
        "id": "01acme-line-0",
        "stream": {"kind": "session", "id": "01acme-session"},
        "recorded_at": 1_792_013_706_002_824i64,
        "payload": {"event": {"kind": "goal_usage_attribution", "record": {
            "usage_id": "usage-acme",
            "usage_family": "provider",
            "quantity": {"input_tokens": 56645, "output_tokens": 337},
            "owner": {"session_id": "01acme-session"}
        }}}
    })
    .to_string();
    assert!(
        parse_line(&attribution).is_none(),
        "the attribution twin must not double-count"
    );
    assert!(parse_line("{not json").is_none());
    assert!(parse_line("{}").is_none());
    assert!(may_matter(&Muse, &muse_completed_line()));
    assert!(!may_matter(&Muse, &attribution));
}

#[test]
fn parse_muse_drops_zero_and_modelless_usage() {
    let zero = muse_completed_line()
        .replace("56645", "0")
        .replace("337", "0")
        .replace("34292", "0")
        .replace("120", "0");
    assert!(parse_line(&zero).is_none());
    let modelless = muse_completed_line().replace("\"muse-acme-1.0\"", "\"\"");
    assert!(parse_line(&modelless).is_none());
}
