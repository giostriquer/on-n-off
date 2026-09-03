use super::*;

fn claude_file(dir: &Path, pid: u32, body: serde_json::Value) {
    fs::write(dir.join(format!("{pid}.json")), body.to_string()).unwrap();
}

#[test]
fn claude_lists_live_interactive_sessions_newest_first() {
    let dir = crate::paths::scratch_dir("notch-claude-sessions");
    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    claude_file(
        &dir,
        100,
        serde_json::json!({
            "pid": 100, "sessionId": "aaaa-11", "name": "on-n-off-11", "entrypoint": "claude-desktop",
            "cwd": "/Users/me/Documents/on-n-off", "status": null,
            "updatedAt": null, "statusUpdatedAt": null
        }),
    );
    claude_file(
        &dir,
        200,
        serde_json::json!({
            "pid": 200, "sessionId": "bbbb-22", "name": "orchestrator-b4", "entrypoint": "cli",
            "cwd": "/Users/me/Documents/g2i/orchestrator", "status": "busy",
            "updatedAt": now_ms - 300_000, "statusUpdatedAt": now_ms - 240_000
        }),
    );
    claude_file(
        &dir,
        300,
        serde_json::json!({
            "pid": 300, "sessionId": "cccc-33", "entrypoint": "sdk-ts",
            "cwd": "/Users/me/Documents/g2i", "status": "idle", "updatedAt": now_ms - 60_000
        }),
    );
    fs::write(dir.join("broken.json"), "{not json").unwrap();
    fs::write(dir.join("300.key"), "secret").unwrap();

    let sessions = read_claude(&dir, now, |pids| {
        assert_eq!(pids.len(), 3);
        [200, 300].into_iter().collect()
    });

    assert_eq!(sessions.len(), 2, "dead pid 100 must be dropped");
    assert_eq!(sessions[0].name, "g2i");
    assert_eq!(sessions[0].place, "SDK");
    assert_eq!(sessions[0].status, SessionStatus::Idle);
    assert_eq!(sessions[1].name, "orchestrator-b4");
    assert_eq!(sessions[1].place, "Terminal");
    assert_eq!(sessions[1].project, "orchestrator");
    assert_eq!(sessions[1].status, SessionStatus::Working);
    assert_eq!(
        sessions[1].last_active_at,
        rfc3339(now_ms - 240_000, now),
        "the newer of statusUpdatedAt / updatedAt"
    );
    let json = serde_json::to_value(&sessions[1]).unwrap();
    assert_eq!(json["status"], "working");
    assert_eq!(json["lastActiveAt"], sessions[1].last_active_at);
    assert!(json.get("cwd").is_none());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn claude_desktop_sessions_without_status_fall_back_to_idle_and_file_time() {
    let dir = crate::paths::scratch_dir("notch-claude-desktop");
    let now = Utc::now();
    claude_file(
        &dir,
        18437,
        serde_json::json!({
            "pid": 18437, "sessionId": "dddd-28", "name": "on-n-off-28",
            "entrypoint": "claude-desktop", "cwd": "/Users/me/Documents/personal/on-n-off"
        }),
    );
    let sessions = read_claude(&dir, now, |pids| pids.iter().copied().collect());
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].place, "Desktop");
    assert_eq!(sessions[0].status, SessionStatus::Idle);
    assert_eq!(
        sessions[0].last_active_at,
        rfc3339(mtime_ms(&dir.join("18437.json")), now)
    );
    assert!(read_claude(&dir.join("missing"), now, |_| HashSet::new()).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn claude_skips_oversize_session_files_without_reading_them() {
    let dir = crate::paths::scratch_dir("notch-claude-oversize");
    let padding = "x".repeat(CLAUDE_MAX_FILE_BYTES);
    claude_file(
        &dir,
        7,
        serde_json::json!({"pid": 7, "name": "big", "note": padding}),
    );
    claude_file(&dir, 8, serde_json::json!({"pid": 8, "name": "small"}));
    let sessions = read_claude(&dir, Utc::now(), |pids| pids.iter().copied().collect());
    assert_eq!(
        sessions.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["small"]
    );
    fs::remove_dir_all(dir).unwrap();
}

fn rollout(root: &Path, now: DateTime<Utc>, name: &str, lines: &[serde_json::Value]) {
    let dir = day_dir(root, now.date_naive());
    fs::create_dir_all(&dir).unwrap();
    let body: Vec<String> = lines.iter().map(ToString::to_string).collect();
    fs::write(dir.join(name), body.join("\n") + "\n").unwrap();
}

fn meta(id: &str, cwd: &str, originator: &str) -> serde_json::Value {
    serde_json::json!({"type": "session_meta", "payload": {"id": id, "cwd": cwd, "originator": originator, "source": "cli"}})
}

fn event(kind: &str) -> serde_json::Value {
    serde_json::json!({"type": "event_msg", "payload": {"type": kind}})
}

#[test]
fn codex_reads_recent_rollouts_and_infers_work_from_task_boundaries() {
    let root = crate::paths::scratch_dir("notch-codex-sessions");
    let now = Utc::now();
    rollout(
        &root,
        now,
        "rollout-a.jsonl",
        &[
            meta(
                "01a0-aaaa-42",
                "/Users/me/Documents/g2i/orchestrator",
                "Codex Desktop",
            ),
            event("task_started"),
            event("task_complete"),
            event("task_started"),
            serde_json::json!({"type": "response_item", "payload": {"type": "message"}}),
        ],
    );
    rollout(
        &root,
        now,
        "rollout-b.jsonl",
        &[
            meta("01a0-bbbb-77", "/Users/me/Documents/g2i", "codex_cli_rs"),
            event("task_started"),
            event("turn_aborted"),
        ],
    );
    rollout(
        &root,
        now,
        "not-a-session.jsonl",
        &[serde_json::json!({"type": "event_msg", "payload": {"type": "task_started"}})],
    );

    let sessions = read_codex(&root, now);
    assert_eq!(sessions.len(), 2);
    let working = sessions
        .iter()
        .find(|s| s.name == "orchestrator-42")
        .unwrap();
    assert_eq!(working.status, SessionStatus::Working);
    assert_eq!(working.place, "Desktop");
    assert_eq!(working.project, "orchestrator");
    assert_eq!(working.id, "01a0-aaaa-42");
    let idle = sessions.iter().find(|s| s.name == "g2i-77").unwrap();
    assert_eq!(idle.status, SessionStatus::Idle);
    assert_eq!(idle.place, "Terminal");

    // A transcript last written 20 minutes ago is no longer working, and after an hour it
    // is no longer listed at all.
    let later = now + Duration::minutes(20);
    let stale = read_codex(&root, later);
    assert!(stale.iter().all(|s| s.status == SessionStatus::Idle));
    assert!(read_codex(&root, now + Duration::hours(2)).is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_treats_a_transcript_still_being_written_as_work_without_boundaries_in_the_tail() {
    let root = crate::paths::scratch_dir("notch-codex-tail");
    let now = Utc::now();
    let mut lines = vec![meta("01a0-cccc-01", "/tmp/project", "codex_cli_rs")];
    lines.push(event("task_started"));
    let filler = "x".repeat(4096);
    for _ in 0..40 {
        lines.push(serde_json::json!({"type": "response_item", "payload": {"type": "text", "text": filler}}));
    }
    rollout(&root, now, "rollout-long.jsonl", &lines);
    let sessions = read_codex(&root, now);
    assert_eq!(sessions[0].status, SessionStatus::Working);
    assert_eq!(
        read_codex(&root, now + Duration::minutes(5))[0].status,
        SessionStatus::Idle
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn other_providers_have_no_sessions_and_ordering_caps_the_list() {
    let home = crate::paths::scratch_dir("notch-sessions-home");
    assert!(read(AgentId::Cursor, &home, Utc::now()).is_empty());
    assert!(read(AgentId::Antigravity, &home, Utc::now()).is_empty());
    let observed: Vec<Observed> = (0..20)
        .map(|index| Observed {
            session: LiveSession {
                id: index.to_string(),
                name: format!("s{index}"),
                place: "Terminal".into(),
                project: "p".into(),
                status: SessionStatus::Idle,
                last_active_at: String::new(),
            },
            last_active_ms: index,
        })
        .collect();
    let sessions = finish(observed);
    assert_eq!(sessions.len(), MAX_SESSIONS);
    assert_eq!(sessions[0].id, "19");
    fs::remove_dir_all(home).unwrap();
}
