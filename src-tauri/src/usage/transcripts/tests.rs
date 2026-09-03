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
