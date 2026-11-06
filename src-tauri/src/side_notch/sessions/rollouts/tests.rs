use super::*;
use crate::dto::AgentId;
use crate::side_notch::sessions::MAX_SESSIONS;

fn codex(home: &Path, now: DateTime<Utc>) -> Vec<LiveSession> {
    crate::side_notch::sessions::read(AgentId::Codex, home, now)
}

fn muse(data: &Path, now: DateTime<Utc>) -> Vec<LiveSession> {
    read(&MUSE, data, now)
}

fn names(sessions: &[LiveSession]) -> Vec<&str> {
    let mut names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
    names.sort_unstable();
    names
}

fn set_mtime(path: &Path, ms: i64) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64),
        )
        .unwrap();
}

fn jsonl(path: &Path, lines: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let body: Vec<String> = lines.iter().map(ToString::to_string).collect();
    fs::write(path, body.join("\n") + "\n").unwrap();
}

fn codex_rollout(home: &Path, day: NaiveDate, name: &str, lines: &[Value]) -> PathBuf {
    let path = day_dir(&home.join(".codex").join("sessions"), day).join(name);
    jsonl(&path, lines);
    path
}

fn meta(id: &str, cwd: &str, originator: &str) -> Value {
    serde_json::json!({"type": "session_meta", "payload": {"id": id, "cwd": cwd, "originator": originator, "source": "cli"}})
}

fn event(kind: &str) -> Value {
    serde_json::json!({"type": "event_msg", "payload": {"type": kind}})
}

fn muse_transcript(data: &Path, day: NaiveDate, id: &str, lines: &[Value]) -> PathBuf {
    let path = day_dir(&data.join("sessions"), day)
        .join(id)
        .join("session.jsonl");
    jsonl(&path, lines);
    path
}

fn muse_record(workspace: &str) -> Value {
    serde_json::json!({"schema_version": 1, "id": "r1", "payload": {"record": {"workspace_root": workspace}}})
}

fn muse_event(kind: &str) -> Value {
    serde_json::json!({"payload": {"event": {"kind": kind}}})
}

fn register_muse(data: &Path, id: &str, name: Option<&str>, workspace_label: &str) -> PathBuf {
    let dir = data.join("runtime").join("muse").join("sessions");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{id}.json"));
    fs::write(
        &path,
        serde_json::json!({
            "schema_version": 1,
            "session_id": id,
            "session_name": name,
            "endpoint_hint": "ms-acme.sock",
            "workspace_label": workspace_label,
            "target_eligibility": "message_capable",
            "process_generation_hint": "gen-1"
        })
        .to_string(),
    )
    .unwrap();
    path
}

#[test]
fn a_turn_reads_as_working_only_while_it_is_fresh() {
    let minutes = |count: i64| count * 60_000;
    assert_eq!(Turn::Open.status(minutes(15)), SessionStatus::Working);
    assert_eq!(Turn::Open.status(minutes(15) + 1), SessionStatus::Idle);
    assert_eq!(Turn::Unmarked.status(minutes(2)), SessionStatus::Working);
    assert_eq!(Turn::Unmarked.status(minutes(2) + 1), SessionStatus::Idle);
    assert_eq!(Turn::Closed.status(0), SessionStatus::Idle);
    assert_eq!(Turn::Closed.status(-1), SessionStatus::Idle);
}

#[test]
fn codex_reads_recent_rollouts_and_infers_work_from_task_boundaries() {
    let home = crate::paths::scratch_dir("notch-codex-sessions");
    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let newer = codex_rollout(
        &home,
        now.date_naive(),
        "rollout-a.jsonl",
        &[
            meta(
                "01a0-aaaa-42",
                "/Users/me/Documents/acme/webapp",
                "Codex Desktop",
            ),
            event("task_started"),
            event("task_complete"),
            event("task_started"),
            serde_json::json!({"type": "response_item", "payload": {"type": "message"}}),
        ],
    );
    set_mtime(&newer, now_ms - 60_000);
    let older = codex_rollout(
        &home,
        now.date_naive(),
        "rollout-b.jsonl",
        &[
            meta("01a0-bbbb-77", "/Users/me/Documents/acme", "codex_cli_rs"),
            event("task_started"),
            event("turn_aborted"),
        ],
    );
    set_mtime(&older, now_ms - 5 * 60_000);
    codex_rollout(
        &home,
        now.date_naive(),
        "not-a-session.jsonl",
        &[serde_json::json!({"type": "event_msg", "payload": {"type": "task_started"}})],
    );

    let sessions = codex(&home, now);
    assert_eq!(sessions.len(), 2);
    let working = &sessions[0];
    assert_eq!(working.name, "webapp-42");
    assert_eq!(working.status, SessionStatus::Working);
    assert_eq!(working.place, "Desktop");
    assert_eq!(working.project, "webapp");
    assert_eq!(working.id, "01a0-aaaa-42");
    assert_eq!(working.last_active_at, rfc3339(now_ms - 60_000, now));
    let idle = &sessions[1];
    assert_eq!(idle.name, "acme-77");
    assert_eq!(idle.status, SessionStatus::Idle);
    assert_eq!(idle.place, "Terminal");
    assert_eq!(idle.last_active_at, rfc3339(now_ms - 5 * 60_000, now));

    let later = now + Duration::minutes(20);
    let stale = codex(&home, later);
    assert_eq!(stale.len(), 2);
    assert!(stale.iter().all(|s| s.status == SessionStatus::Idle));
    assert!(codex(&home, now + Duration::hours(2)).is_empty());
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn codex_treats_a_transcript_still_being_written_as_work_without_boundaries_in_the_tail() {
    let home = crate::paths::scratch_dir("notch-codex-tail");
    let now = Utc::now();
    let mut lines = vec![meta("01a0-cccc-01", "/tmp/project", "codex_cli_rs")];
    lines.push(event("task_started"));
    let filler = "x".repeat(4096);
    for _ in 0..40 {
        lines.push(serde_json::json!({"type": "response_item", "payload": {"type": "text", "text": filler}}));
    }
    codex_rollout(&home, now.date_naive(), "rollout-long.jsonl", &lines);
    let sessions = codex(&home, now);
    assert_eq!(sessions[0].status, SessionStatus::Working);
    assert_eq!(
        codex(&home, now + Duration::minutes(5))[0].status,
        SessionStatus::Idle
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn codex_head_and_tail_reads_reach_past_sixteen_kib_of_long_lines() {
    let home = crate::paths::scratch_dir("notch-codex-long-lines");
    let now = Utc::now();
    let mut long_meta = meta("01a0-aaaa-1a", "/Users/me/acme", "codex_cli_rs");
    long_meta["payload"]["base_instructions"] = Value::String("x".repeat(16 * 1024));
    codex_rollout(&home, now.date_naive(), "long-meta.jsonl", &[long_meta]);
    let mut lines = vec![
        meta("01a0-aaaa-1b", "/Users/me/webapp", "codex_cli_rs"),
        event("task_started"),
    ];
    let filler = "x".repeat(4096);
    for _ in 0..4 {
        lines.push(serde_json::json!({"type": "response_item", "payload": {"type": "text", "text": filler}}));
    }
    codex_rollout(&home, now.date_naive(), "long-output.jsonl", &lines);
    let sessions = codex(&home, now + Duration::minutes(5));
    assert_eq!(names(&sessions), ["acme-1a", "webapp-1b"]);
    let open = sessions.iter().find(|s| s.id == "01a0-aaaa-1b").unwrap();
    assert_eq!(open.status, SessionStatus::Working);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn muse_lists_registered_and_recent_sessions_with_working_state() {
    let data = crate::paths::scratch_dir("notch-muse-sessions");
    let now = Utc::now();
    muse_transcript(
        &data,
        now.date_naive(),
        "aaaa-1111-2222-3333-444455556666",
        &[
            muse_record("/Users/me/Documents/acme/webapp"),
            muse_event("model_response_created"),
            muse_event("started"),
        ],
    );
    register_muse(
        &data,
        "aaaa-1111-2222-3333-444455556666",
        Some("acme-grus"),
        "storefront",
    );
    muse_transcript(
        &data,
        now.date_naive(),
        "bbbb-1111-2222-3333-444455557777",
        &[
            muse_record("/Users/me/Documents/acme"),
            muse_event("model_response_created"),
            muse_event("model_completed"),
            muse_event("assistant_message_committed"),
        ],
    );

    let sessions = muse(&data, now);
    assert_eq!(sessions.len(), 2);
    let working = sessions
        .iter()
        .find(|session| session.id.starts_with("aaaa"))
        .expect("registered session");
    assert_eq!(working.name, "acme-grus");
    assert_eq!(working.project, "storefront");
    assert_eq!(working.place, "Terminal");
    assert_eq!(working.status, SessionStatus::Working);
    let idle = sessions
        .iter()
        .find(|session| session.id.starts_with("bbbb"))
        .expect("recent session");
    assert_eq!(idle.name, "acme-77");
    assert_eq!(idle.project, "acme");
    assert_eq!(idle.status, SessionStatus::Idle);

    let later = muse(&data, now + Duration::minutes(90));
    assert_eq!(later.len(), 1);
    assert_eq!(later[0].status, SessionStatus::Idle);
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn muse_missing_dirs_list_nothing() {
    let data = crate::paths::scratch_dir("notch-muse-missing");
    assert!(muse(&data, Utc::now()).is_empty());
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn codex_walks_jsonl_rollouts_no_older_than_yesterday() {
    let home = crate::paths::scratch_dir("notch-codex-days");
    let now = Utc::now();
    let today = now.date_naive();
    for (offset, id) in [
        (0, "01a0-aaaa-01"),
        (1, "01a0-aaaa-02"),
        (2, "01a0-aaaa-03"),
    ] {
        codex_rollout(
            &home,
            today - Duration::days(offset),
            "rollout.jsonl",
            &[meta(id, "/Users/me/acme", "codex_cli_rs")],
        );
    }
    codex_rollout(
        &home,
        today,
        "rollout.json",
        &[meta("01a0-aaaa-04", "/Users/me/acme", "codex_cli_rs")],
    );
    let sessions = codex(&home, now);
    assert_eq!(names(&sessions), ["acme-01", "acme-02"]);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn codex_reads_only_the_48_newest_rollouts() {
    let home = crate::paths::scratch_dir("notch-codex-cap");
    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let noise: Vec<PathBuf> = (0..48)
        .map(|index| {
            let path = codex_rollout(
                &home,
                now.date_naive(),
                &format!("noise-{index:02}.jsonl"),
                &[event("task_started")],
            );
            set_mtime(&path, now_ms - 1_000 * (index + 1));
            path
        })
        .collect();
    let older = codex_rollout(
        &home,
        now.date_naive(),
        "older.jsonl",
        &[meta("01a0-aaaa-99", "/Users/me/acme", "codex_cli_rs")],
    );
    set_mtime(&older, now_ms - 60_000);
    assert!(
        codex(&home, now).is_empty(),
        "the 49th newest rollout is never opened"
    );
    fs::remove_file(&noise[47]).unwrap();
    assert_eq!(names(&codex(&home, now)), ["acme-99"]);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn codex_names_a_session_from_its_meta_line() {
    let home = crate::paths::scratch_dir("notch-codex-meta");
    let now = Utc::now();
    let today = now.date_naive();
    codex_rollout(
        &home,
        today,
        "a.jsonl",
        &[
            serde_json::json!({"type": "session_meta", "payload": {"session_id": "01a0-ab", "cwd": "/Users/me/webapp", "originator": "codex_vscode"}}),
        ],
    );
    codex_rollout(
        &home,
        today,
        "b.jsonl",
        &[
            serde_json::json!({"type": "session_meta", "payload": {"id": "c", "originator": "codex_exec", "source": "vscode"}}),
        ],
    );
    codex_rollout(
        &home,
        today,
        "c.jsonl",
        &[serde_json::json!({"type": "session_meta", "payload": {"cwd": "/Users/me/acme"}})],
    );
    codex_rollout(
        &home,
        today,
        "d.jsonl",
        &[
            serde_json::json!({"type": "response_item", "payload": {"id": "01a0-dd", "cwd": "/Users/me/acme"}}),
        ],
    );
    let sessions = codex(&home, now);
    assert_eq!(names(&sessions), ["Home-c", "webapp-ab"]);
    let vscode = sessions.iter().find(|s| s.id == "01a0-ab").unwrap();
    assert_eq!(vscode.place, "VS Code");
    assert_eq!(vscode.project, "webapp");
    let by_source = sessions.iter().find(|s| s.id == "c").unwrap();
    assert_eq!(by_source.place, "VS Code");
    assert_eq!(by_source.project, "Home");
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn codex_working_follows_the_last_event_message_marker_and_its_age() {
    let home = crate::paths::scratch_dir("notch-codex-turns");
    let now = Utc::now();
    let today = now.date_naive();
    let start = meta("01a0-aaaa-0a", "/Users/me/open", "codex_cli_rs");
    codex_rollout(
        &home,
        today,
        "open.jsonl",
        &[
            start,
            event("task_started"),
            serde_json::json!({"type": "response_item", "payload": {"type": "task_complete"}}),
        ],
    );
    codex_rollout(
        &home,
        today,
        "closed.jsonl",
        &[
            meta("01a0-aaaa-0b", "/Users/me/closed", "codex_cli_rs"),
            event("task_started"),
            event("task_complete"),
        ],
    );
    codex_rollout(
        &home,
        today,
        "unmarked.jsonl",
        &[meta("01a0-aaaa-0c", "/Users/me/unmarked", "codex_cli_rs")],
    );
    codex_rollout(
        &home,
        today,
        "quoted.jsonl",
        &[
            meta("01a0-aaaa-0d", "/Users/me/quoted", "codex_cli_rs"),
            event("task_started"),
            serde_json::json!({"type": "event_msg", "payload": {"type": "agent_message", "message": "task_complete"}}),
        ],
    );
    let status = |minutes: i64, project: &str| {
        codex(&home, now + Duration::minutes(minutes))
            .into_iter()
            .find(|s| s.project == project)
            .unwrap()
            .status
    };
    assert_eq!(status(0, "open"), SessionStatus::Working);
    assert_eq!(status(14, "open"), SessionStatus::Working);
    assert_eq!(status(16, "open"), SessionStatus::Idle);
    assert_eq!(status(0, "closed"), SessionStatus::Idle);
    assert_eq!(status(1, "unmarked"), SessionStatus::Working);
    assert_eq!(status(3, "unmarked"), SessionStatus::Idle);
    assert_eq!(
        status(5, "quoted"),
        SessionStatus::Working,
        "an end kind named outside the marker's kind leaves the turn open"
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn muse_walks_visible_session_folders_and_keeps_the_newest_day_of_an_id() {
    let data = crate::paths::scratch_dir("notch-muse-walk");
    let now = Utc::now();
    let today = now.date_naive();
    muse_transcript(&data, today, "aaaa-01", &[muse_record("/Users/me/today")]);
    muse_transcript(
        &data,
        today - Duration::days(1),
        "aaaa-01",
        &[muse_record("/Users/me/yesterday")],
    );
    muse_transcript(
        &data,
        today - Duration::days(1),
        "aaaa-02",
        &[muse_record("/Users/me/acme")],
    );
    muse_transcript(
        &data,
        today - Duration::days(2),
        "aaaa-03",
        &[muse_record("/Users/me/acme")],
    );
    muse_transcript(&data, today, ".aaaa-04", &[muse_record("/Users/me/acme")]);
    fs::create_dir_all(day_dir(&data.join("sessions"), today).join("aaaa-05")).unwrap();
    fs::write(day_dir(&data.join("sessions"), today).join("aaaa-06"), "{}").unwrap();
    let sessions = muse(&data, now);
    assert_eq!(names(&sessions), ["acme-02", "today-01"]);
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn muse_reads_only_the_48_newest_transcripts() {
    let data = crate::paths::scratch_dir("notch-muse-cap");
    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    for index in 0..48 {
        let path = muse_transcript(&data, now.date_naive(), &format!("bbbb-{index:02}"), &[]);
        set_mtime(&path, now_ms - 60_000 * (index + 1));
    }
    let old = muse_transcript(&data, now.date_naive(), "cccc-99", &[]);
    set_mtime(&old, now_ms - 50 * 60_000);
    register_muse(&data, "cccc-99", Some("acme-late"), "acme");
    let sessions = muse(&data, now);
    assert_eq!(sessions.len(), MAX_SESSIONS);
    assert!(
        sessions.iter().all(|s| s.id != "cccc-99"),
        "the registered session's transcript is the 49th newest"
    );
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn muse_activity_is_the_later_write_but_working_follows_the_transcript() {
    let data = crate::paths::scratch_dir("notch-muse-activity");
    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let started = muse_event("model_response_created");
    for (id, minutes) in [("dddd-10", 10), ("dddd-20", 20)] {
        let path = muse_transcript(&data, now.date_naive(), id, std::slice::from_ref(&started));
        set_mtime(&path, now_ms - minutes * 60_000);
        let registration = register_muse(&data, id, None, "");
        set_mtime(&registration, now_ms - 1_000);
    }
    let sessions = muse(&data, now);
    let ten = sessions.iter().find(|s| s.id == "dddd-10").unwrap();
    assert_eq!(ten.status, SessionStatus::Working);
    assert_eq!(ten.last_active_at, rfc3339(now_ms - 1_000, now));
    assert_eq!(ten.project, "Home");
    assert_eq!(ten.name, "Home-10");
    let twenty = sessions.iter().find(|s| s.id == "dddd-20").unwrap();
    assert_eq!(twenty.status, SessionStatus::Idle);
    assert_eq!(twenty.last_active_at, rfc3339(now_ms - 1_000, now));
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn muse_workspace_is_the_first_head_line_naming_one() {
    let data = crate::paths::scratch_dir("notch-muse-workspace");
    let now = Utc::now();
    let today = now.date_naive();
    muse_transcript(
        &data,
        today,
        "eeee-01",
        &[
            serde_json::json!({"workspace_root": "  "}),
            serde_json::json!({"payload": {"note": "no workspace here"}}),
            serde_json::json!({"a": {"cwd": "/Users/me/by-cwd"}, "b": [{"workspace_root": "/Users/me/by-root"}]}),
            serde_json::json!({"workspace_root": "/Users/me/later"}),
        ],
    );
    muse_transcript(
        &data,
        today,
        "eeee-02",
        &[serde_json::json!({"deep": {"deeper": {"cwd": "/Users/me/webapp"}}})],
    );
    let sessions = muse(&data, now);
    assert_eq!(names(&sessions), ["by-root-01", "webapp-02"]);
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn muse_registry_entries_name_by_file_stem_and_skip_blank_oversize_and_foreign_files() {
    let data = crate::paths::scratch_dir("notch-muse-registry");
    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let registry = data.join("runtime/muse/sessions");
    fs::create_dir_all(&registry).unwrap();
    for id in ["ffff-01", "ffff-02", "ffff-03", "ffff-04", "ffff-05"] {
        let path = muse_transcript(
            &data,
            now.date_naive(),
            id,
            &[serde_json::json!({"workspace_root": "/Users/me/acme"})],
        );
        set_mtime(&path, now_ms - 3 * 60 * 60_000);
    }
    fs::write(
        registry.join("ffff-01.json"),
        serde_json::json!({"session_id": " ", "session_name": "  ", "workspace_label": ""})
            .to_string(),
    )
    .unwrap();
    fs::write(
        registry.join("ffff-02.json"),
        serde_json::json!({"session_id": "ffff-02", "session_name": "acme-big", "note": "x".repeat(REGISTRY_MAX_FILE_BYTES as usize)})
            .to_string(),
    )
    .unwrap();
    fs::write(
        registry.join("ffff-03.txt"),
        serde_json::json!({"session_id": "ffff-03"}).to_string(),
    )
    .unwrap();
    fs::write(
        registry.join("ffff-04.json"),
        serde_json::json!({"session_id": "ffff-04", "session_name": 4}).to_string(),
    )
    .unwrap();
    let at_the_cap = |note: usize| {
        serde_json::json!({"session_id": "ffff-05", "session_name": "acme-full", "note": "x".repeat(note)})
            .to_string()
    };
    let full = registry.join("ffff-05.json");
    fs::write(&full, at_the_cap(64 * 1024 - at_the_cap(0).len())).unwrap();
    assert_eq!(fs::metadata(&full).unwrap().len(), 64 * 1024);
    let sessions = muse(&data, now);
    assert_eq!(names(&sessions), ["acme-01", "acme-full"]);
    assert_eq!(sessions[0].project, "acme");
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn muse_counts_every_start_and_end_marker_kind() {
    let data = crate::paths::scratch_dir("notch-muse-markers");
    let now = Utc::now();
    let cases = [
        (
            "gggg-01",
            vec!["model_input_trace_recorded"],
            SessionStatus::Working,
        ),
        (
            "gggg-02",
            vec!["model_response_created", "model_completed"],
            SessionStatus::Idle,
        ),
        (
            "gggg-03",
            vec!["model_response_created", "assistant_message_committed"],
            SessionStatus::Idle,
        ),
        (
            "gggg-04",
            vec!["model_response_created", "terminal"],
            SessionStatus::Idle,
        ),
        (
            "gggg-05",
            vec!["terminal", "model_input_trace_recorded", "started"],
            SessionStatus::Working,
        ),
    ];
    for (id, kinds, _) in &cases {
        let lines: Vec<Value> = kinds.iter().map(|kind| muse_event(kind)).collect();
        muse_transcript(&data, now.date_naive(), id, &lines);
    }
    muse_transcript(
        &data,
        now.date_naive(),
        "gggg-06",
        &[
            muse_event("model_response_created"),
            serde_json::json!({"payload": {"event": {"kind": "tool_call_started", "target": "terminal"}}}),
        ],
    );
    let later = now + Duration::minutes(5);
    let sessions = muse(&data, later);
    for (id, _, status) in cases {
        assert_eq!(
            sessions.iter().find(|s| s.id == id).unwrap().status,
            status,
            "{id}"
        );
    }
    assert_eq!(
        sessions.iter().find(|s| s.id == "gggg-06").unwrap().status,
        SessionStatus::Working,
        "an end kind named outside the event's kind leaves the turn open"
    );
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn the_walk_reads_the_day_a_zone_east_of_utc_has_already_reached() {
    let home = crate::paths::scratch_dir("notch-codex-east");
    let data = crate::paths::scratch_dir("notch-muse-east");
    let now = Utc::now();
    let today = now.date_naive();
    let tomorrow = today + Duration::days(1);
    codex_rollout(
        &home,
        tomorrow,
        "rollout.jsonl",
        &[meta("01a0-aaaa-0e", "/Users/me/acme", "codex_cli_rs")],
    );
    muse_transcript(
        &data,
        tomorrow,
        "hhhh-0e",
        &[muse_record("/Users/me/webapp")],
    );
    muse_transcript(&data, today, "hhhh-0e", &[muse_record("/Users/me/acme")]);
    assert_eq!(names(&codex(&home, now)), ["acme-0e"]);
    assert_eq!(
        names(&muse(&data, now)),
        ["webapp-0e"],
        "the newest day's folder names the session"
    );
    fs::remove_dir_all(home).unwrap();
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn around_utc_midnight_the_walk_holds_every_zones_local_today() {
    let rows = [
        (
            "2026-02-28T23:30:00Z",
            ["2026-03-01", "2026-02-28", "2026-02-27"],
        ),
        (
            "2026-02-28T23:59:59Z",
            ["2026-03-01", "2026-02-28", "2026-02-27"],
        ),
        (
            "2026-03-01T00:00:00Z",
            ["2026-03-02", "2026-03-01", "2026-02-28"],
        ),
        (
            "2026-03-01T00:30:00Z",
            ["2026-03-02", "2026-03-01", "2026-02-28"],
        ),
        (
            "2026-12-31T23:59:59Z",
            ["2027-01-01", "2026-12-31", "2026-12-30"],
        ),
        (
            "2027-01-01T00:00:00Z",
            ["2027-01-02", "2027-01-01", "2026-12-31"],
        ),
    ];
    for (instant, walked) in rows {
        let now: DateTime<Utc> = instant.parse().unwrap();
        let walk = days(now);
        assert_eq!(
            walk,
            walked.map(|day| day.parse::<NaiveDate>().unwrap()),
            "{instant}"
        );
        for quarter_hours in -12 * 4..=14 * 4 {
            let local_today = (now + Duration::minutes(15 * quarter_hours)).date_naive();
            assert!(
                walk.contains(&local_today),
                "{instant} at UTC{:+} min",
                15 * quarter_hours
            );
        }
    }
}
