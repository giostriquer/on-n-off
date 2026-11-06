use super::*;
use crate::paths::scratch_dir;

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
        copy_key: key.map(str::to_string),
    }
}

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
        let kept = richest_copies(
            files
                .iter()
                .map(|file| (UsageProvider::Claude, file.as_slice())),
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].totals.output_tokens, 50);
        assert_eq!(kept[0].session_id, "fork");
    }
}

#[test]
fn richest_copies_keeps_a_messages_billed_line_in_its_first_lines_place() {
    let line = |key: Option<&str>, output: u64| {
        usage_record(UsageProvider::Claude, key, "session-a", 100, output)
    };
    let file = [
        line(Some("msg_1:"), 1),
        line(None, 4),
        line(Some("msg_1:"), 99),
        line(Some("msg_2:"), 4),
        line(Some("msg_1:"), 7),
    ];

    let kept: Vec<(Option<&str>, u64)> = richest_copies([(UsageProvider::Claude, &file[..])])
        .into_iter()
        .map(|record| (record.copy_key.as_deref(), record.totals.output_tokens))
        .collect();

    assert_eq!(kept, [(Some("msg_1:"), 99), (None, 4), (Some("msg_2:"), 4)]);
}

#[test]
fn richest_copies_breaks_output_ties_by_total_tokens_and_keeps_the_held_copy_on_a_full_tie() {
    let held = usage_record(UsageProvider::Claude, Some("k"), "held", 100, 50);
    let same_usage = usage_record(UsageProvider::Claude, Some("k"), "later", 100, 50);
    let more_input = usage_record(UsageProvider::Claude, Some("k"), "richer", 200, 50);

    let tie = [held.clone(), same_usage];
    let kept = richest_copies([(UsageProvider::Claude, &tie[..])]);
    assert_eq!(kept[0].session_id, "held");
    let richer = [held, more_input];
    let kept = richest_copies([(UsageProvider::Claude, &richer[..])]);
    assert_eq!(kept[0].session_id, "richer");
}

#[test]
fn richest_copies_keeps_one_muse_line_per_id_and_every_line_without_one() {
    let line = |key: Option<&str>, session: &str, output: u64| {
        usage_record(UsageProvider::Muse, key, session, 100, output)
    };
    let session = [line(Some("line-1"), "parent", 10), line(None, "parent", 3)];
    let subagent = [line(Some("line-1"), "child", 12), line(None, "child", 3)];

    let kept: Vec<(&str, u64)> = richest_copies([
        (UsageProvider::Muse, &session[..]),
        (UsageProvider::Muse, &subagent[..]),
    ])
    .into_iter()
    .map(|record| (record.session_id.as_str(), record.totals.output_tokens))
    .collect();

    assert_eq!(kept, [("child", 12), ("parent", 3), ("child", 3)]);
}

#[test]
fn richest_copies_never_collapses_two_providers_records_that_share_a_key() {
    let claude = [usage_record(
        UsageProvider::Claude,
        Some("msg_1:req_1"),
        "claude-session",
        100,
        5,
    )];
    let muse = [usage_record(
        UsageProvider::Muse,
        Some("msg_1:req_1"),
        "muse-session",
        100,
        9,
    )];

    let kept: Vec<(UsageProvider, u64)> = richest_copies([
        (UsageProvider::Claude, &claude[..]),
        (UsageProvider::Muse, &muse[..]),
    ])
    .into_iter()
    .map(|record| (record.provider, record.totals.output_tokens))
    .collect();

    assert_eq!(kept, [(UsageProvider::Claude, 5), (UsageProvider::Muse, 9)]);
}

fn codex(files: [&[UsageRecord]; 2]) -> impl Iterator<Item = (UsageProvider, &[UsageRecord])> {
    files.into_iter().map(|file| (UsageProvider::Codex, file))
}

#[test]
fn richest_copies_counts_a_codex_rollout_once_however_many_times_it_is_listed() {
    let event = |timestamp_ms: i64| UsageRecord {
        timestamp_ms,
        model: "gpt-5.6-sol".into(),
        ..usage_record(UsageProvider::Codex, None, "session-a", 100, 20)
    };
    let rollout = vec![event(1_000), event(5_000), event(1_000)];

    let kept = richest_copies(codex([rollout.as_slice(), rollout.as_slice()]));
    assert_eq!(kept.len(), 3);

    let kept = richest_copies(codex([&rollout[..2], rollout.as_slice()]));
    assert_eq!(kept.len(), 3, "a stale, shorter listing adds nothing");

    let other_session: Vec<UsageRecord> = rollout
        .iter()
        .map(|record| UsageRecord {
            session_id: "session-b".into(),
            ..record.clone()
        })
        .collect();
    let kept = richest_copies(codex([rollout.as_slice(), other_session.as_slice()]));
    assert_eq!(kept.len(), 6);

    let later_elsewhere = [event(9_000)];
    let kept = richest_copies(codex([rollout.as_slice(), &later_elsewhere[..]]));
    assert_eq!(
        kept.len(),
        4,
        "the same session's identical tokens at another instant are another event"
    );
    let other_model = [UsageRecord {
        model: "gpt-6-sol".into(),
        ..event(1_000)
    }];
    let kept = richest_copies(codex([rollout.as_slice(), &other_model[..]]));
    assert_eq!(kept.len(), 4, "and so are they under another model");

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
    let kept = richest_copies(codex([&anonymous[..], &anonymous[..]]));
    assert_eq!(
        kept.len(),
        4,
        "without a session there is nothing to tie copies together"
    );
}

#[test]
fn list_finds_jsonl_by_mtime() {
    let root = scratch_dir("usage-list");
    let nested = root.join("a").join("b");
    std::fs::create_dir_all(&nested).unwrap();
    let path = nested.join("session.jsonl");
    std::fs::write(&path, "{}\n").unwrap();
    let inventory = inventory_transcript_files(&root, 0);
    assert!(inventory.complete);
    assert_eq!(inventory.files.len(), 1);
    assert_eq!(inventory.files[0].path, path);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn list_finds_superseded_transcripts_and_nothing_else() {
    let root = scratch_dir("usage-list-superseded");
    std::fs::create_dir_all(&root).unwrap();
    let names = [
        "session.jsonl",
        "session.jsonl.superseded-1758000000000",
        "session.jsonl.tmp",
        "notes.txt",
    ];
    for name in names {
        std::fs::write(root.join(name), "{}\n").unwrap();
    }

    let inventory = inventory_transcript_files(&root, 0);
    let mut found: Vec<String> = inventory
        .files
        .iter()
        .map(|file| {
            file.path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    found.sort();

    assert_eq!(
        found,
        ["session.jsonl", "session.jsonl.superseded-1758000000000"]
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn read_claude_file() {
    let root = scratch_dir("usage-read");
    let path = root.join("t.jsonl");
    let line = serde_json::json!({
        "type": "assistant",
        "timestamp": "2026-08-07T04:05:13.944Z",
        "sessionId": "s1",
        "message": {
            "id": "msg_1",
            "model": "claude-fable-5",
            "usage": {
                "input_tokens": 2,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
                "output_tokens": 10
            }
        }
    })
    .to_string();
    std::fs::write(&path, format!("{line}\n")).unwrap();
    let records = read_transcript(&path, UsageProvider::Claude).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].totals.output_tokens, 10);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn read_error_after_valid_line_rejects_partial_records() {
    let line = serde_json::json!({
        "type": "assistant",
        "timestamp": "2026-08-07T04:05:13.944Z",
        "sessionId": "s1",
        "message": {
            "id": "msg_1",
            "model": "claude-fable-5",
            "usage": {
                "input_tokens": 2,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
                "output_tokens": 10
            }
        }
    })
    .to_string();
    let lines = vec![
        Ok(line),
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "injected read failure",
        )),
    ]
    .into_iter();

    assert!(records_of(lines, format_of(UsageProvider::Claude)).is_none());
}
