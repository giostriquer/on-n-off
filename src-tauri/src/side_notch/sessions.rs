//! Live agent sessions listed under a provider's quota windows in the side notch. Claude Code
//! keeps `~/.claude/sessions/<pid>.json` while a session runs; Codex appends rollout transcripts
//! under `~/.codex/sessions/YYYY/MM/DD/`. Both are read-only and stay on this machine.

use crate::dto::AgentId;
use chrono::{DateTime, Datelike, Duration, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Rows per provider; the popover is a glance, not a session manager. The helper enforces the
/// same cap when it validates a message.
pub const MAX_SESSIONS: usize = 12;
const CLAUDE_MAX_FILE_BYTES: usize = 64 * 1024;
const CODEX_MAX_FILES: usize = 48;
const CODEX_TAIL_BYTES: u64 = 128 * 1024;
const CODEX_RECENT_MINUTES: i64 = 60;
/// A `task_started` older than this without a later boundary is a stale transcript, not work.
const CODEX_WORKING_MINUTES: i64 = 15;
/// A transcript still being written counts as work even when its boundary events are out of
/// the tail window.
const CODEX_ACTIVE_WRITE_MINUTES: i64 = 2;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Working,
    Idle,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LiveSession {
    pub id: String,
    pub name: String,
    /// Where the session runs: "Desktop", "Terminal", "VS Code", …
    pub place: String,
    /// The working directory's last path component.
    pub project: String,
    pub status: SessionStatus,
    /// RFC 3339 instant of the last observed activity.
    pub last_active_at: String,
}

struct Observed {
    session: LiveSession,
    last_active_ms: i64,
}

pub fn read(agent: AgentId, home: &Path, now: DateTime<Utc>) -> Vec<LiveSession> {
    match agent {
        AgentId::Claude => read_claude(&home.join(".claude").join("sessions"), now, live_pids),
        AgentId::Codex => read_codex(&home.join(".codex").join("sessions"), now),
        AgentId::Antigravity | AgentId::Cursor => Vec::new(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeSessionFile {
    pid: Option<u32>,
    session_id: Option<String>,
    name: Option<String>,
    entrypoint: Option<String>,
    cwd: Option<String>,
    status: Option<String>,
    updated_at: Option<i64>,
    status_updated_at: Option<i64>,
}

fn read_claude(
    dir: &Path,
    now: DateTime<Utc>,
    live: impl FnOnce(&[u32]) -> HashSet<u32>,
) -> Vec<LiveSession> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        if fs::metadata(&path)
            .ok()
            .is_none_or(|meta| meta.len() > CLAUDE_MAX_FILE_BYTES as u64)
        {
            continue;
        }
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = serde_json::from_str::<ClaudeSessionFile>(&body) else {
            continue;
        };
        let Some(pid) = file.pid else {
            continue;
        };
        candidates.push((pid, file, mtime_ms(&path)));
    }
    let pids: Vec<u32> = candidates.iter().map(|(pid, _, _)| *pid).collect();
    let alive = live(&pids);
    let observed = candidates
        .into_iter()
        .filter(|(pid, _, _)| alive.contains(pid))
        .map(|(pid, file, mtime)| {
            let project = project_name(file.cwd.as_deref());
            let last_active_ms = file
                .status_updated_at
                .max(file.updated_at)
                .filter(|ms| *ms > 0)
                .unwrap_or(mtime);
            let name = file
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| project.clone());
            Observed {
                session: LiveSession {
                    id: file.session_id.unwrap_or_else(|| pid.to_string()),
                    name,
                    place: claude_place(file.entrypoint.as_deref()),
                    project,
                    status: if file.status.as_deref() == Some("busy") {
                        SessionStatus::Working
                    } else {
                        SessionStatus::Idle
                    },
                    last_active_at: rfc3339(last_active_ms, now),
                },
                last_active_ms,
            }
        })
        .collect();
    finish(observed)
}

fn claude_place(entrypoint: Option<&str>) -> String {
    let entrypoint = entrypoint.unwrap_or("cli").to_ascii_lowercase();
    if entrypoint.contains("desktop") {
        "Desktop".into()
    } else if entrypoint == "cli" {
        "Terminal".into()
    } else if entrypoint.starts_with("sdk") {
        "SDK".into()
    } else {
        capitalize(&entrypoint)
    }
}

/// `ps` reports only the processes that still exist. A missing, failing, or hung `ps` keeps
/// every row rather than blocking the supervisor or hiding live sessions.
#[cfg(unix)]
fn live_pids(pids: &[u32]) -> HashSet<u32> {
    use crate::process::{wait_with_deadline, CommandOutcome};
    use std::process::{Command, Stdio};
    if pids.is_empty() {
        return HashSet::new();
    }
    let list = pids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let everything = || pids.iter().copied().collect();
    let Ok(child) = Command::new("/bin/ps")
        .args(["-o", "pid=", "-p", &list])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return everything();
    };
    match wait_with_deadline(child, std::time::Duration::from_secs(2)) {
        Ok(CommandOutcome::Exited { stdout, .. }) => stdout
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect(),
        Ok(CommandOutcome::TimedOut) | Err(_) => everything(),
    }
}

#[cfg(not(unix))]
fn live_pids(pids: &[u32]) -> HashSet<u32> {
    pids.iter().copied().collect()
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: Option<String>,
    payload: Option<serde_json::Value>,
}

fn read_codex(root: &Path, now: DateTime<Utc>) -> Vec<LiveSession> {
    let now_ms = now.timestamp_millis();
    let mut files: Vec<(PathBuf, i64)> = Vec::new();
    for offset in 0..=1 {
        let dir = day_dir(root, (now - Duration::days(offset)).date_naive());
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_none_or(|extension| extension != "jsonl")
            {
                continue;
            }
            let mtime = mtime_ms(&path);
            if now_ms - mtime <= CODEX_RECENT_MINUTES * 60_000 {
                files.push((path, mtime));
            }
        }
    }
    files.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));
    files.truncate(CODEX_MAX_FILES);
    let observed = files
        .into_iter()
        .filter_map(|(path, mtime)| codex_session(&path, mtime, now))
        .collect();
    finish(observed)
}

fn codex_session(path: &Path, mtime_ms: i64, now: DateTime<Utc>) -> Option<Observed> {
    let mut file = fs::File::open(path).ok()?;
    let mut first = Vec::new();
    BufReader::new(&file)
        .take(CODEX_TAIL_BYTES)
        .read_until(b'\n', &mut first)
        .ok()?;
    let meta: Envelope = serde_json::from_slice(&first).ok()?;
    if meta.kind.as_deref() != Some("session_meta") {
        return None;
    }
    let payload = meta.payload?;
    let text = |key: &str| {
        payload
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let id = text("id").or_else(|| text("session_id"))?;
    let project = project_name(text("cwd").as_deref());
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(CODEX_TAIL_BYTES)))
        .ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    let boundary = last_task_boundary(&tail);
    let age_ms = now.timestamp_millis() - mtime_ms;
    let status = match boundary {
        Some(true) if age_ms <= CODEX_WORKING_MINUTES * 60_000 => SessionStatus::Working,
        None if age_ms <= CODEX_ACTIVE_WRITE_MINUTES * 60_000 => SessionStatus::Working,
        _ => SessionStatus::Idle,
    };
    let suffix: String = id
        .chars()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Some(Observed {
        session: LiveSession {
            id,
            name: if suffix.is_empty() {
                project.clone()
            } else {
                format!("{project}-{suffix}")
            },
            place: codex_place(text("originator").as_deref(), text("source").as_deref()),
            project,
            status,
            last_active_at: rfc3339(mtime_ms, now),
        },
        last_active_ms: mtime_ms,
    })
}

/// `Some(true)` after a `task_started` with no later `task_complete` / `turn_aborted`.
fn last_task_boundary(tail: &[u8]) -> Option<bool> {
    let mut state = None;
    for line in tail.split(|byte| *byte == b'\n') {
        let Ok(text) = std::str::from_utf8(line) else {
            continue;
        };
        if !["\"task_started\"", "\"task_complete\"", "\"turn_aborted\""]
            .iter()
            .any(|needle| text.contains(needle))
        {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Envelope>(text) else {
            continue;
        };
        if event.kind.as_deref() != Some("event_msg") {
            continue;
        }
        match event
            .payload
            .as_ref()
            .and_then(|payload| payload.get("type"))
            .and_then(|kind| kind.as_str())
        {
            Some("task_started") => state = Some(true),
            Some("task_complete" | "turn_aborted") => state = Some(false),
            _ => {}
        }
    }
    state
}

fn codex_place(originator: Option<&str>, source: Option<&str>) -> String {
    let originator = originator.unwrap_or_default().to_ascii_lowercase();
    if originator.contains("desktop") {
        "Desktop".into()
    } else if originator.contains("vscode") || source == Some("vscode") {
        "VS Code".into()
    } else {
        "Terminal".into()
    }
}

/// `<root>/YYYY/MM/DD`, the layout Codex uses for its rollouts.
fn day_dir(root: &Path, day: NaiveDate) -> PathBuf {
    root.join(format!("{:04}", day.year()))
        .join(format!("{:02}", day.month()))
        .join(format!("{:02}", day.day()))
}

fn finish(mut observed: Vec<Observed>) -> Vec<LiveSession> {
    observed.sort_by(|a, b| {
        b.last_active_ms
            .cmp(&a.last_active_ms)
            .then_with(|| a.session.name.cmp(&b.session.name))
    });
    observed
        .into_iter()
        .take(MAX_SESSIONS)
        .map(|entry| entry.session)
        .collect()
}

fn project_name(cwd: Option<&str>) -> String {
    cwd.map(Path::new)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(|| "Home".to_string(), str::to_string)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn mtime_ms(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_millis() as i64)
}

fn rfc3339(ms: i64, now: DateTime<Utc>) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .unwrap_or(now)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
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
}
