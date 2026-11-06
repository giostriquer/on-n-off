use super::*;

fn write_lines(path: &Path, lines: &[Value]) {
    let lines: Vec<String> = lines.iter().map(Value::to_string).collect();
    write_records(path, &lines);
}

fn codex_rollout(home: &Path) -> PathBuf {
    home.join(".codex")
        .join("sessions")
        .join("2026")
        .join("08")
        .join("07")
        .join("rollout-acme.jsonl")
}

fn token_count(timestamp: &str, input: u64, output: u64) -> Value {
    json!({
        "type": "event_msg",
        "timestamp": timestamp,
        "payload": {
            "type": "token_count",
            "info": { "last_token_usage": { "input_tokens": input, "output_tokens": output } }
        }
    })
}

fn muse_completed(id: &str, recorded_at: &str, output: u64) -> Value {
    json!({
        "id": id,
        "stream": { "kind": "session", "id": "muse-session" },
        "recorded_at": at(recorded_at) * 1_000,
        "payload": { "event": {
            "kind": "model_completed",
            "model": "muse-acme-1.0",
            "usage": { "input_tokens": 100, "output_tokens": output }
        } }
    })
}

type Row = (UsageProvider, String, String, i64, u64, Option<String>);

fn rows(read: &SourceRead) -> Vec<Row> {
    let mut rows: Vec<Row> = read
        .records()
        .into_iter()
        .map(|record| {
            (
                record.provider,
                record.model.clone(),
                record.session_id.clone(),
                record.timestamp_ms,
                record.totals.output_tokens,
                record.copy_key.clone(),
            )
        })
        .collect();
    rows.sort_by_key(|row| (row.0, row.3));
    rows
}

#[test]
fn each_provider_reports_its_first_transcript_directory_and_whether_any_exists() {
    let home = scratch_dir("usage-sources-dirs");
    let dirs = |home: &Path| -> Vec<(UsageProvider, PathBuf, bool)> {
        let (read, seen) = with_sources(home, Watermark::NONE, |sources| {
            sources.read(i64::MIN, || Watermark::NONE)
        });
        seen.providers(&read)
            .map(|source| {
                (
                    source.provider,
                    source.dir.to_path_buf(),
                    source.files.is_some(),
                )
            })
            .collect()
    };
    let nested = home.join(".claude").join("projects");
    let flat = home.join("projects");
    let sessions = home.join(".codex").join("sessions");
    let muse = crate::paths::muse_data_for(&home).join("sessions");

    assert_eq!(
        dirs(&home),
        [
            (UsageProvider::Claude, nested.clone(), false),
            (UsageProvider::Codex, sessions.clone(), false),
            (UsageProvider::Muse, muse.clone(), false),
        ]
    );

    std::fs::create_dir_all(&flat).unwrap();
    std::fs::create_dir_all(home.join(".codex").join("archived_sessions")).unwrap();
    std::fs::create_dir_all(&muse).unwrap();
    assert_eq!(
        dirs(&home),
        [
            (UsageProvider::Claude, flat, true),
            (UsageProvider::Codex, sessions.clone(), true),
            (UsageProvider::Muse, muse.clone(), true),
        ]
    );

    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(dirs(&home)[0], (UsageProvider::Claude, nested, true));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn each_provider_reads_its_own_transcript_lines() {
    let home = scratch_dir("usage-sources-formats");
    write_lines(
        &transcript_path(&home, "claude.jsonl"),
        &[
            json!({ "type": "user", "message": { "content": "usage" } }),
            serde_json::from_str(&record("2026-08-07T04:05:13.944Z", "msg-1", 20)).unwrap(),
            json!({
                "type": "assistant",
                "timestamp": "2026-08-07T04:06:00.000Z",
                "sessionId": "sources-session",
                "message": { "id": "msg-0", "model": "<synthetic>", "usage": { "output_tokens": 0 } }
            }),
        ],
    );
    write_lines(
        &codex_rollout(&home),
        &[
            json!({ "type": "session_meta", "timestamp": "2026-08-07T04:00:00.000Z", "payload": { "id": "codex-session" } }),
            token_count("2026-08-07T04:00:00.500Z", 50, 5),
            json!({ "type": "turn_context", "timestamp": "2026-08-07T04:00:01.000Z", "payload": { "model": "gpt-5.6-sol" } }),
            json!({ "type": "event_msg", "timestamp": "2026-08-07T04:00:02.000Z", "payload": { "type": "agent_message" } }),
            token_count("2026-08-07T04:05:00.000Z", 100, 10),
            token_count("2026-08-07T04:05:01.000Z", 100, 10),
            token_count("2026-08-07T04:06:00.000Z", 200, 20),
        ],
    );
    let muse = crate::paths::muse_data_for(&home)
        .join("sessions")
        .join("2026")
        .join("08")
        .join("07")
        .join("muse-session")
        .join("session.jsonl");
    write_lines(
        &muse,
        &[
            json!({
                "id": "muse-twin",
                "stream": { "kind": "session", "id": "muse-session" },
                "recorded_at": at("2026-08-07T04:00:00Z") * 1_000,
                "payload": { "event": { "kind": "goal_usage_attribution", "record": {
                    "quantity": { "input_tokens": 100, "output_tokens": 7 }
                } } }
            }),
            muse_completed("muse-line", "2026-08-07T04:00:00Z", 7),
            muse_completed("muse-empty", "2026-08-07T04:01:00Z", 0)
                .to_string()
                .replace("\"input_tokens\":100", "\"input_tokens\":0")
                .parse()
                .unwrap(),
        ],
    );

    let read = read_all(&home, Watermark::NONE);

    assert_eq!(
        rows(&read),
        [
            (
                UsageProvider::Claude,
                "claude-fable-5".into(),
                "sources-session".into(),
                at("2026-08-07T04:05:13.944Z"),
                20,
                Some("msg-1:".into())
            ),
            (
                UsageProvider::Codex,
                "gpt-5.6-sol".into(),
                "codex-session".into(),
                at("2026-08-07T04:05:00.000Z"),
                10,
                None
            ),
            (
                UsageProvider::Codex,
                "gpt-5.6-sol".into(),
                "codex-session".into(),
                at("2026-08-07T04:06:00.000Z"),
                20,
                None
            ),
            (
                UsageProvider::Muse,
                "muse-acme-1.0".into(),
                "muse-session".into(),
                at("2026-08-07T04:00:00Z"),
                7,
                Some("muse-line".into())
            ),
        ]
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_transcript_is_cached_with_one_record_per_copy_group_it_holds() {
    let home = scratch_dir("usage-sources-copy-groups");
    let claude = transcript_path(&home, "copies.jsonl");
    write_records(
        &claude,
        &[
            record("2026-08-07T04:05:13.944Z", "msg-1", 1),
            record("2026-08-07T04:05:13.944Z", "msg-2", 5),
            record("2026-08-07T04:05:13.944Z", "msg-1", 30),
        ],
    );
    let codex = codex_rollout(&home);
    write_lines(
        &codex,
        &[
            json!({ "type": "session_meta", "timestamp": "2026-08-07T04:00:00.000Z", "payload": { "id": "codex-session" } }),
            json!({ "type": "turn_context", "timestamp": "2026-08-07T04:00:01.000Z", "payload": { "model": "gpt-5.6-sol" } }),
            token_count("2026-08-07T04:05:00.000Z", 100, 10),
            token_count("2026-08-07T04:06:00.000Z", 200, 20),
            token_count("2026-08-07T04:05:00.000Z", 100, 10),
        ],
    );

    let read = read_all(&home, Watermark::NONE);

    assert_eq!(cached_record_count(&home, &claude), Some(2));
    assert_eq!(cached_record_count(&home, &codex), Some(3));
    let outputs: Vec<(UsageProvider, u64)> = read
        .records()
        .into_iter()
        .map(|record| (record.provider, record.totals.output_tokens))
        .collect();
    assert_eq!(
        outputs,
        [
            (UsageProvider::Claude, 30),
            (UsageProvider::Claude, 5),
            (UsageProvider::Codex, 10),
            (UsageProvider::Codex, 20),
            (UsageProvider::Codex, 10),
        ]
    );
    let _ = std::fs::remove_dir_all(home);
}
