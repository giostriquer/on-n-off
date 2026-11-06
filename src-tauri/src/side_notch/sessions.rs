use crate::dto::AgentId;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::Path, time::SystemTime};

mod rollouts;

pub const MAX_SESSIONS: usize = 12;
const CLAUDE_MAX_FILE_BYTES: usize = 64 * 1024;

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
    pub place: String,
    pub project: String,
    pub status: SessionStatus,
    pub last_active_at: String,
}

struct Observed {
    session: LiveSession,
    last_active_ms: i64,
}

pub fn read(agent: AgentId, home: &Path, now: DateTime<Utc>) -> Vec<LiveSession> {
    match agent {
        AgentId::Claude => read_claude(
            &crate::paths::claude_root_for(home).join("sessions"),
            now,
            live_pids,
        ),
        AgentId::Codex => {
            rollouts::read(&rollouts::CODEX, &crate::paths::codex_root_for(home), now)
        }
        AgentId::Muse => rollouts::read(&rollouts::MUSE, &crate::paths::muse_data_for(home), now),
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
mod tests;
