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
fn parse_claude_extracts_totals_and_copy_key() {
    let record = parse_line(&claude_line("msg_1", "text")).expect("record");
    assert_eq!(record.provider, UsageProvider::Claude);
    assert_eq!(record.model, "claude-fable-5");
    assert_eq!(record.totals.uncached_input_tokens, 2);
    assert_eq!(record.totals.cached_input_tokens, 1000);
    assert_eq!(record.totals.cache_creation_tokens, 66818);
    assert_eq!(record.totals.output_tokens, 286);
    assert_eq!(record.copy_key.as_deref(), Some("msg_1:"));
}

#[test]
fn parse_claude_same_copy_key_across_content_blocks() {
    let text = parse_line(&claude_line("msg_2", "text")).unwrap();
    let tool = parse_line(&claude_line("msg_2", "tool_use")).unwrap();
    assert_eq!(text.copy_key, tool.copy_key);
    assert_eq!(text.totals, tool.totals);
}

fn claude_usage_line(model: &str, usage: serde_json::Value) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": "2026-08-07T04:05:13.944Z",
        "sessionId": "session-a",
        "requestId": "req_1",
        "message": { "id": "msg_1", "model": model, "usage": usage }
    })
    .to_string()
}

#[test]
fn parse_claude_keeps_the_one_hour_share_of_cache_writes() {
    let usage = |one_hour: u64| {
        serde_json::json!({
            "input_tokens": 2,
            "cache_creation_input_tokens": 1000,
            "cache_read_input_tokens": 0,
            "output_tokens": 5,
            "cache_creation": {
                "ephemeral_5m_input_tokens": 1000 - one_hour.min(1000),
                "ephemeral_1h_input_tokens": one_hour
            }
        })
    };
    let record = parse_line(&claude_usage_line("claude-opus-5", usage(600))).unwrap();
    assert_eq!(record.totals.cache_creation_tokens, 1000);
    assert_eq!(record.totals.cache_creation_1h_tokens, 600);

    let inconsistent = parse_line(&claude_usage_line("claude-opus-5", usage(5000))).unwrap();
    assert_eq!(
        inconsistent.totals.cache_creation_1h_tokens, 1000,
        "the one-hour share never exceeds the writes it is a share of"
    );

    let unsplit = parse_line(&claude_line("msg_3", "text")).unwrap();
    assert_eq!(unsplit.totals.cache_creation_1h_tokens, 0);
}

#[test]
fn parse_claude_skips_records_without_tokens() {
    let zero = serde_json::json!({
        "input_tokens": 0,
        "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": 0,
        "output_tokens": 0
    });
    assert!(parse_line(&claude_usage_line("<synthetic>", zero)).is_none());
}

#[test]
fn parse_claude_keeps_a_line_with_input_but_no_output() {
    let input_only = serde_json::json!({
        "input_tokens": 5,
        "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": 0,
        "output_tokens": 0
    });
    let record = parse_line(&claude_usage_line("claude-opus-5", input_only)).unwrap();
    assert_eq!(record.totals.uncached_input_tokens, 5);
}

#[test]
fn parse_claude_ignores_non_assistant_and_garbage() {
    assert!(parse_line(r#"{"type":"user","message":{}}"#).is_none());
    assert!(parse_line("not json").is_none());
}
