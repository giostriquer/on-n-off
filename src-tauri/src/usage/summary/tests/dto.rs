//! The summary exactly as it crosses IPC to the Usage screen, read from transcripts and from the
//! usage history together.

use serde_json::{json, Value};

use super::super::test_support::*;
use super::super::*;
use crate::paths::scratch_dir;
use crate::usage::folding::{fold_history_in, FoldChecks};
use crate::usage::pricing::{self, rates_cache_path};

/// Power-of-two prices, so every cost below is exact in binary and written as a literal. Fetched
/// long before the read, which therefore tries to fetch again and, offline, keeps this table.
fn write_rates(home: &Path) {
    let path = rates_cache_path(home);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let rates = json!({
        "fetchedAtMs": at("2026-08-01T00:00:00Z"),
        "document": {
            "claude-fable-5": {
                "input_cost_per_token": 0.0009765625,
                "output_cost_per_token": 0.00390625,
                "cache_read_input_token_cost": 0.0001220703125,
                "cache_creation_input_token_cost": 0.001953125
            },
            "gpt-5.6-sol": {
                "input_cost_per_token": 0.000244140625,
                "output_cost_per_token": 0.0009765625,
                "cache_read_input_token_cost": 0.00006103515625
            }
        }
    });
    std::fs::write(path, rates.to_string()).unwrap();
}

fn claude_usage(input: u64, cache_read: u64, output: u64) -> Value {
    json!({
        "input_tokens": input,
        "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": cache_read,
        "output_tokens": output
    })
}

/// One Codex turn: its session, model and a single usage event at `at_iso`.
fn write_rollout(path: &Path, session: &str, at_iso: &str, usage: Value, written: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines = [
        json!({ "type": "session_meta", "timestamp": at_iso, "payload": { "id": session } }),
        json!({ "type": "turn_context", "timestamp": at_iso, "payload": { "model": "gpt-5.6-sol" } }),
        json!({
            "type": "event_msg",
            "timestamp": at_iso,
            "payload": { "type": "token_count", "info": { "last_token_usage": usage } }
        }),
    ];
    let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(path, body).unwrap();
    set_mtime(path, written);
}

/// A home read for August after a fold on 2026-08-21, so the watermark sits at 2026-08-14:
/// - Claude `session.jsonl`: `msg_a` on 2026-08-07 (a partial line, then the billed one), folded,
///   and `msg_b` on 2026-08-20, read from the transcript.
/// - Claude `gone.jsonl`: `msg_g` on 2026-08-10, folded, and the transcript deleted after.
/// - Claude `notes.jsonl`: written on 2026-08-19 and holding no usage at all.
/// - Codex: one live rollout and one archived rollout, both on 2026-08-18.
fn write_fixture_home(home: &Path) {
    write_rates(home);
    write_claude_lines(
        home,
        "session.jsonl",
        &[
            claude_usage_line(
                "msg_a",
                "sess-a",
                "2026-08-07T04:05:13.000Z",
                claude_usage(1024, 2048, 1),
            ),
            claude_usage_line(
                "msg_a",
                "sess-a",
                "2026-08-07T04:05:13.944Z",
                claude_usage(1024, 2048, 256),
            ),
            claude_usage_line(
                "msg_b",
                "sess-a",
                "2026-08-20T10:00:00.000Z",
                claude_usage(16, 0, 64),
            ),
        ],
    );
    set_mtime(
        &home.join(".claude/projects/proj/session.jsonl"),
        "2026-08-20T10:00:01Z",
    );
    write_claude_lines(
        home,
        "gone.jsonl",
        &[claude_usage_line(
            "msg_g",
            "sess-gone",
            "2026-08-10T12:00:00.000Z",
            claude_usage(8, 0, 32),
        )],
    );
    set_mtime(
        &home.join(".claude/projects/proj/gone.jsonl"),
        "2026-08-10T12:00:01Z",
    );
    write_claude_lines(
        home,
        "notes.jsonl",
        &[json!({
            "type": "user",
            "timestamp": "2026-08-19T08:00:00.000Z",
            "sessionId": "sess-notes",
            "message": { "role": "user", "content": "placeholder" }
        })
        .to_string()],
    );
    set_mtime(
        &home.join(".claude/projects/proj/notes.jsonl"),
        "2026-08-19T08:00:01Z",
    );
    write_rollout(
        &home.join(".codex/sessions/2026/08/18/rollout-live.jsonl"),
        "session-live",
        "2026-08-18T09:00:00.000Z",
        json!({
            "input_tokens": 4096,
            "cached_input_tokens": 1024,
            "output_tokens": 512,
            "reasoning_output_tokens": 128
        }),
        "2026-08-18T09:00:01Z",
    );
    write_rollout(
        &home.join(".codex/archived_sessions/rollout-archived.jsonl"),
        "session-archived",
        "2026-08-18T09:30:00.000Z",
        json!({ "input_tokens": 512, "cached_input_tokens": 0, "output_tokens": 32 }),
        "2026-08-18T09:30:01Z",
    );
    fold_history_in(home, at("2026-08-21T12:00:00Z"), &mut FoldChecks::default()).unwrap();
    std::fs::remove_file(home.join(".claude/projects/proj/gone.jsonl")).unwrap();
}

/// The summary as JSON, with what differs between runs replaced: when it was read, how long the
/// scan took, and the scratch home every source path starts with.
fn pinned(home: &Path, summary: &UsageSummaryDto) -> Value {
    let mut value = serde_json::to_value(summary).unwrap();
    value["readAt"] = json!("<read at>");
    value["scanDurationMs"] = json!("<scan duration>");
    for source in value["sources"].as_array_mut().unwrap() {
        let path = PathBuf::from(source["resolvedPath"].as_str().unwrap());
        let parts: Vec<String> = path
            .strip_prefix(home)
            .unwrap()
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        source["resolvedPath"] = json!(format!("~/{}", parts.join("/")));
    }
    value
}

/// Costs by hand, at the prices in `write_rates`:
/// - 2026-08-07: 1024 × 2⁻¹⁰ + 2048 × 2⁻¹³ + 256 × 2⁻⁸ = 2.25; saved 2048 × (2⁻¹⁰ − 2⁻¹³) = 1.75.
/// - 2026-08-10: 8 × 2⁻¹⁰ + 32 × 2⁻⁸ = 0.1328125.
/// - 2026-08-18: (4096 − 1024) × 2⁻¹² + 1024 × 2⁻¹⁴ + 512 × 2⁻¹⁰ = 1.3125, plus 512 × 2⁻¹² +
///   32 × 2⁻¹⁰ = 0.15625; saved 1024 × (2⁻¹² − 2⁻¹⁴) = 0.1875.
/// - 2026-08-20: 16 × 2⁻¹⁰ + 64 × 2⁻⁸ = 0.265625.
fn expected(cache_hit: bool) -> Value {
    let totals = |uncached: u64, cached: u64, output: u64, reasoning: u64| {
        json!({
            "uncachedInputTokens": uncached,
            "cachedInputTokens": cached,
            "cacheCreationTokens": 0,
            "outputTokens": output,
            "reasoningTokens": reasoning
        })
    };
    json!({
        "readAt": "<read at>",
        "timeZone": "UTC",
        "sinceDay": "2026-08-01",
        "untilDay": "2026-08-31",
        "buckets": [
            {
                "day": "2026-08-07",
                "provider": "claude",
                "model": "claude-fable-5",
                "totals": totals(1024, 2048, 256, 0),
                "costUsd": 2.25,
                "cacheSavingsUsd": 1.75,
                "costSource": "modelPriced",
                "records": 1,
                "unpricedRecords": 0,
                "sessions": 1
            },
            {
                "day": "2026-08-10",
                "provider": "claude",
                "model": "claude-fable-5",
                "totals": totals(8, 0, 32, 0),
                "costUsd": 0.1328125,
                "cacheSavingsUsd": 0.0,
                "costSource": "modelPriced",
                "records": 1,
                "unpricedRecords": 0,
                "sessions": 1
            },
            {
                "day": "2026-08-18",
                "provider": "codex",
                "model": "gpt-5.6-sol",
                "totals": totals(3584, 1024, 544, 128),
                "costUsd": 1.46875,
                "cacheSavingsUsd": 0.1875,
                "costSource": "modelPriced",
                "records": 2,
                "unpricedRecords": 0,
                "sessions": 2
            },
            {
                "day": "2026-08-20",
                "provider": "claude",
                "model": "claude-fable-5",
                "totals": totals(16, 0, 64, 0),
                "costUsd": 0.265625,
                "cacheSavingsUsd": 0.0,
                "costSource": "modelPriced",
                "records": 1,
                "unpricedRecords": 0,
                "sessions": 1
            }
        ],
        "sources": [
            {
                "provider": "claude",
                "status": "ok",
                "scannedFiles": 1,
                "skippedFiles": 1,
                "malformedRecords": 0,
                "distinctSessions": 2,
                "resolvedPath": "~/.claude/projects"
            },
            {
                "provider": "codex",
                "status": "ok",
                "scannedFiles": 2,
                "skippedFiles": 0,
                "malformedRecords": 0,
                "distinctSessions": 2,
                "resolvedPath": "~/.codex/sessions"
            }
        ],
        "pricing": {
            "status": "cached",
            "source": "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json",
            "fetchedAt": "2026-08-01T00:00:00.000Z",
            "knownModels": 2
        },
        "scanDurationMs": "<scan duration>",
        "cacheHit": cache_hit
    })
}

#[test]
fn a_summary_of_transcripts_and_folded_usage_crosses_ipc_as_pinned() {
    let _serial = pricing::lock_rates_state();
    let home = scratch_dir("usage-summary-dto");
    write_fixture_home(&home);

    let read = read_offline(&home, august_input(false));
    let served = read_offline(&home, august_input(false));

    assert_eq!(pinned(&home, &read), expected(false));
    assert_eq!(pinned(&home, &served), expected(true));
    let _ = std::fs::remove_dir_all(&home);
}
