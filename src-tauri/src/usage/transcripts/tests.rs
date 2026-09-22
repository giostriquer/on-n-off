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

/// Claude Code splits cache writes by lifetime; a one-hour write costs 2x input against the
/// five-minute write's 1.25x, so the split has to survive parsing.
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
    let record = parse_claude_line(&claude_usage_line("claude-opus-5", usage(600))).unwrap();
    assert_eq!(record.totals.cache_creation_tokens, 1000);
    assert_eq!(record.totals.cache_creation_1h_tokens, 600);

    let inconsistent = parse_claude_line(&claude_usage_line("claude-opus-5", usage(5000))).unwrap();
    assert_eq!(
        inconsistent.totals.cache_creation_1h_tokens, 1000,
        "the one-hour share never exceeds the writes it is a share of"
    );

    let unsplit = parse_claude_line(&claude_line("msg_3", "text")).unwrap();
    assert_eq!(unsplit.totals.cache_creation_1h_tokens, 0);
}

/// Claude Code writes locally generated assistant lines (API errors, interrupts) as
/// `<synthetic>` with an all-zero usage object; they are not model usage.
#[test]
fn parse_claude_skips_records_without_tokens() {
    let zero = serde_json::json!({
        "input_tokens": 0,
        "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": 0,
        "output_tokens": 0
    });
    assert!(parse_claude_line(&claude_usage_line("<synthetic>", zero)).is_none());
}

#[test]
fn parse_claude_keeps_a_line_with_input_but_no_output() {
    let input_only = serde_json::json!({
        "input_tokens": 5,
        "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": 0,
        "output_tokens": 0
    });
    let record = parse_claude_line(&claude_usage_line("claude-opus-5", input_only)).unwrap();
    assert_eq!(record.totals.uncached_input_tokens, 5);
}

fn usage_record(
    provider: UsageProvider,
    key: Option<&str>,
    session: &str,
    input: u64,
    output: u64,
) -> UsageRecord {
    UsageRecord {
        provider,
        timestamp_ms: 1_000,
        model: "claude-opus-5".into(),
        session_id: session.into(),
        totals: TokenTotals {
            uncached_input_tokens: input,
            output_tokens: output,
            ..TokenTotals::default()
        },
        reported_cost_usd: None,
        dedupe_key: key.map(str::to_string),
    }
}

/// A resumed or forked Claude session copies a message into another transcript, sometimes only its
/// partial first line; whichever file the scan meets first, the billed copy is the one kept.
#[test]
fn richest_copies_keeps_one_record_per_message_whichever_file_holds_it_first() {
    let partial = usage_record(
        UsageProvider::Claude,
        Some("msg_1:req_1"),
        "original",
        100,
        1,
    );
    let billed = usage_record(UsageProvider::Claude, Some("msg_1:req_1"), "fork", 100, 50);
    for files in [
        [vec![partial.clone()], vec![billed.clone()]],
        [vec![billed.clone()], vec![partial.clone()]],
    ] {
        let (kept, dropped) = richest_copies(files.iter().map(Vec::as_slice));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].totals.output_tokens, 50);
        assert_eq!(kept[0].session_id, "fork");
        assert_eq!(dropped, 1);
    }
}

#[test]
fn richest_copies_breaks_output_ties_by_total_tokens_and_keeps_the_held_copy_on_a_full_tie() {
    let held = usage_record(UsageProvider::Claude, Some("k"), "held", 100, 50);
    let same_usage = usage_record(UsageProvider::Claude, Some("k"), "later", 100, 50);
    let more_input = usage_record(UsageProvider::Claude, Some("k"), "richer", 200, 50);

    let tie = [held.clone(), same_usage];
    let (kept, _) = richest_copies([&tie[..]]);
    assert_eq!(kept[0].session_id, "held");
    let richer = [held, more_input];
    let (kept, _) = richest_copies([&richer[..]]);
    assert_eq!(kept[0].session_id, "richer");
}

/// Codex rollouts carry no message id. A rollout moved to `archived_sessions/` can be listed under
/// both roots in one scan (a rename between the two walks, or a stale entry kept after an
/// incomplete walk); it still counts once, while repeated identical events inside one rollout,
/// and another session's identical events, stay distinct.
#[test]
fn richest_copies_counts_a_codex_rollout_once_however_many_times_it_is_listed() {
    let event = |timestamp_ms: i64| UsageRecord {
        timestamp_ms,
        model: "gpt-5.6-sol".into(),
        ..usage_record(UsageProvider::Codex, None, "session-a", 100, 20)
    };
    let rollout = vec![event(1_000), event(5_000), event(1_000)];

    let (kept, dropped) = richest_copies([rollout.as_slice(), rollout.as_slice()]);
    assert_eq!((kept.len(), dropped), (3, 3));

    let (kept, _) = richest_copies([&rollout[..2], rollout.as_slice()]);
    assert_eq!(kept.len(), 3, "a stale, shorter listing adds nothing");

    let other_session: Vec<UsageRecord> = rollout
        .iter()
        .map(|record| UsageRecord {
            session_id: "session-b".into(),
            ..record.clone()
        })
        .collect();
    let (kept, _) = richest_copies([rollout.as_slice(), other_session.as_slice()]);
    assert_eq!(kept.len(), 6);

    let anonymous = [
        UsageRecord {
            session_id: String::new(),
            ..event(1_000)
        },
        UsageRecord {
            session_id: String::new(),
            ..event(1_000)
        },
    ];
    let (kept, _) = richest_copies([&anonymous[..], &anonymous[..]]);
    assert_eq!(
        kept.len(),
        4,
        "without a session there is nothing to tie copies together"
    );
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
