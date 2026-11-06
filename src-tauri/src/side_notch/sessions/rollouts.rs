use super::{finish, mtime_ms, project_name, rfc3339, LiveSession, Observed, SessionStatus};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 48;
const HEAD_BYTES: u64 = 128 * 1024;
const TAIL_BYTES: u64 = 128 * 1024;
const RECENT_MINUTES: i64 = 60;
const WORKING_MINUTES: i64 = 15;
const ACTIVE_WRITE_MINUTES: i64 = 2;
const REGISTRY_MAX_FILE_BYTES: u64 = 64 * 1024;

pub(super) struct Rollouts {
    layout: Layout,
    markers: Markers,
}

enum Layout {
    Files(Files),
    Folders(Folders),
}

struct Files {
    extension: &'static str,
    meta: Meta,
}

struct Meta {
    kind: Field,
    id: &'static [&'static str],
    workspace: &'static str,
    originator: &'static str,
    source: &'static str,
}

struct Folders {
    transcript: &'static str,
    workspace: &'static [&'static str],
    registry: Registry,
}

enum Registry {
    Muse,
}

struct Markers {
    kind_at: &'static str,
    only_in: Option<Field>,
    starts: &'static [&'static str],
    ends: &'static [&'static str],
}

#[derive(Clone, Copy)]
struct Field {
    at: &'static str,
    is: &'static str,
}

pub(super) const CODEX: Rollouts = Rollouts {
    layout: Layout::Files(Files {
        extension: "jsonl",
        meta: Meta {
            kind: Field {
                at: "/type",
                is: "session_meta",
            },
            id: &["/payload/id", "/payload/session_id"],
            workspace: "/payload/cwd",
            originator: "/payload/originator",
            source: "/payload/source",
        },
    }),
    markers: Markers {
        kind_at: "/payload/type",
        only_in: Some(Field {
            at: "/type",
            is: "event_msg",
        }),
        starts: &["task_started"],
        ends: &["task_complete", "turn_aborted"],
    },
};

pub(super) const MUSE: Rollouts = Rollouts {
    layout: Layout::Folders(Folders {
        transcript: "session.jsonl",
        workspace: &["workspace_root", "cwd"],
        registry: Registry::Muse,
    }),
    markers: Markers {
        kind_at: "/payload/event/kind",
        only_in: None,
        starts: &["model_response_created", "model_input_trace_recorded"],
        ends: &["model_completed", "assistant_message_committed", "terminal"],
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    Open,
    Closed,
    Unmarked,
}

impl Turn {
    fn status(self, age_ms: i64) -> SessionStatus {
        match self {
            Self::Open if age_ms <= WORKING_MINUTES * 60_000 => SessionStatus::Working,
            Self::Unmarked if age_ms <= ACTIVE_WRITE_MINUTES * 60_000 => SessionStatus::Working,
            Self::Open | Self::Closed | Self::Unmarked => SessionStatus::Idle,
        }
    }
}

struct Candidate<K> {
    key: K,
    transcript: PathBuf,
    written_ms: i64,
}

struct Found {
    id: String,
    name: Option<String>,
    place: String,
    project: String,
    transcript: PathBuf,
    written_ms: i64,
    active_ms: i64,
}

struct Registration {
    name: Option<String>,
    project: Option<String>,
    mtime_ms: i64,
}

type Registered = HashMap<String, Registration>;

pub(super) fn read(rollouts: &Rollouts, root: &Path, now: DateTime<Utc>) -> Vec<LiveSession> {
    let found = match &rollouts.layout {
        Layout::Files(files) => files.found(root, now),
        Layout::Folders(folders) => folders.found(root, now),
    };
    finish(
        found
            .into_iter()
            .map(|found| found.observe(&rollouts.markers, now))
            .collect(),
    )
}

fn newest<K>(
    root: &Path,
    now: DateTime<Utc>,
    mut session_at: impl FnMut(PathBuf) -> Option<(K, PathBuf)>,
    registered: impl Fn(&K) -> bool,
) -> Vec<Candidate<K>> {
    let dated = root.join("sessions");
    let now_ms = now.timestamp_millis();
    let mut candidates = Vec::new();
    for day in days(now) {
        let Ok(entries) = fs::read_dir(day_dir(&dated, day)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some((key, transcript)) = session_at(entry.path()) else {
                continue;
            };
            let written_ms = mtime_ms(&transcript);
            if registered(&key) || now_ms - written_ms <= RECENT_MINUTES * 60_000 {
                candidates.push(Candidate {
                    key,
                    transcript,
                    written_ms,
                });
            }
        }
    }
    candidates.sort_by_key(|candidate| Reverse(candidate.written_ms));
    candidates.truncate(MAX_FILES);
    candidates
}

impl Files {
    fn found(&self, root: &Path, now: DateTime<Utc>) -> Vec<Found> {
        newest(root, now, |path| self.rollout_at(path), |()| false)
            .into_iter()
            .filter_map(|candidate| self.meta.name(candidate))
            .collect()
    }

    fn rollout_at(&self, path: PathBuf) -> Option<((), PathBuf)> {
        path.extension()
            .is_some_and(|found| found == self.extension)
            .then_some(((), path))
    }
}

impl Meta {
    fn name(&self, candidate: Candidate<()>) -> Option<Found> {
        let line = self.line(&candidate.transcript)?;
        let id = self.id.iter().find_map(|at| str_at(&line, at))?.to_string();
        Some(Found {
            id,
            name: None,
            place: self.place(&line),
            project: project_name(str_at(&line, self.workspace)),
            transcript: candidate.transcript,
            written_ms: candidate.written_ms,
            active_ms: candidate.written_ms,
        })
    }

    fn line(&self, transcript: &Path) -> Option<Value> {
        let file = fs::File::open(transcript).ok()?;
        let mut first = Vec::new();
        BufReader::new(file)
            .take(HEAD_BYTES)
            .read_until(b'\n', &mut first)
            .ok()?;
        let line: Value = serde_json::from_slice(&first).ok()?;
        self.kind.holds(&line).then_some(line)
    }

    fn place(&self, line: &Value) -> String {
        let originator = str_at(line, self.originator)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if originator.contains("desktop") {
            "Desktop".into()
        } else if originator.contains("vscode") || str_at(line, self.source) == Some("vscode") {
            "VS Code".into()
        } else {
            "Terminal".into()
        }
    }
}

impl Folders {
    fn found(&self, root: &Path, now: DateTime<Utc>) -> Vec<Found> {
        let registered = self.registry.read(root);
        let mut seen = HashSet::new();
        newest(
            root,
            now,
            |path| self.session_at(path, &mut seen),
            |id| registered.contains_key(id),
        )
        .into_iter()
        .map(|candidate| {
            let registration = registered.get(&candidate.key);
            self.name(candidate, registration)
        })
        .collect()
    }

    fn session_at(&self, path: PathBuf, seen: &mut HashSet<String>) -> Option<(String, PathBuf)> {
        if !path.is_dir() {
            return None;
        }
        let id = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.starts_with('.'))?
            .to_string();
        let transcript = path.join(self.transcript);
        (transcript.is_file() && seen.insert(id.clone())).then_some((id, transcript))
    }

    fn name(&self, candidate: Candidate<String>, registration: Option<&Registration>) -> Found {
        let project = registration
            .and_then(|entry| entry.project.clone())
            .unwrap_or_else(|| {
                project_name(workspace_in_head(&candidate.transcript, self.workspace).as_deref())
            });
        Found {
            id: candidate.key,
            name: registration.and_then(|entry| entry.name.clone()),
            place: "Terminal".into(),
            project,
            transcript: candidate.transcript,
            written_ms: candidate.written_ms,
            active_ms: registration.map_or(candidate.written_ms, |entry| {
                entry.mtime_ms.max(candidate.written_ms)
            }),
        }
    }
}

impl Found {
    fn observe(self, markers: &Markers, now: DateTime<Utc>) -> Observed {
        let status = markers
            .turn(&self.transcript)
            .status(now.timestamp_millis() - self.written_ms);
        let name = self
            .name
            .unwrap_or_else(|| suffixed(&self.project, &self.id));
        Observed {
            session: LiveSession {
                id: self.id,
                name,
                place: self.place,
                project: self.project,
                status,
                last_active_at: rfc3339(self.active_ms, now),
            },
            last_active_ms: self.active_ms,
        }
    }
}

fn workspace_in_head(transcript: &Path, keys: &[&str]) -> Option<String> {
    let file = fs::File::open(transcript).ok()?;
    let mut head = Vec::new();
    BufReader::new(file)
        .take(HEAD_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    let needles = quoted(keys);
    for line in head.split(|byte| *byte == b'\n') {
        let Ok(text) = std::str::from_utf8(line) else {
            continue;
        };
        if !needles.iter().any(|needle| text.contains(needle.as_str())) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        if let Some(root) = keys
            .iter()
            .find_map(|key| find_text(&value, key))
            .filter(|root| !root.trim().is_empty())
        {
            return Some(root);
        }
    }
    None
}

fn find_text(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(text) = map.get(key).and_then(Value::as_str) {
                return Some(text.to_string());
            }
            map.values().find_map(|entry| find_text(entry, key))
        }
        Value::Array(items) => items.iter().find_map(|entry| find_text(entry, key)),
        _ => None,
    }
}

impl Markers {
    fn turn(&self, transcript: &Path) -> Turn {
        let Some(tail) = tail(transcript) else {
            return Turn::Unmarked;
        };
        let needles = quoted(self.starts.iter().chain(self.ends));
        let mut turn = Turn::Unmarked;
        for line in tail.split(|byte| *byte == b'\n') {
            let Ok(text) = std::str::from_utf8(line) else {
                continue;
            };
            if !needles.iter().any(|needle| text.contains(needle.as_str())) {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            if self.only_in.is_some_and(|field| !field.holds(&value)) {
                continue;
            }
            match str_at(&value, self.kind_at) {
                Some(kind) if self.starts.contains(&kind) => turn = Turn::Open,
                Some(kind) if self.ends.contains(&kind) => turn = Turn::Closed,
                _ => {}
            }
        }
        turn
    }
}

fn tail(transcript: &Path) -> Option<Vec<u8>> {
    let mut file = fs::File::open(transcript).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    Some(tail)
}

fn quoted<'a>(kinds: impl IntoIterator<Item = &'a &'a str>) -> Vec<String> {
    kinds
        .into_iter()
        .map(|kind| format!("\"{kind}\""))
        .collect()
}

fn str_at<'v>(value: &'v Value, at: &str) -> Option<&'v str> {
    value.pointer(at).and_then(Value::as_str)
}

impl Field {
    fn holds(&self, value: &Value) -> bool {
        str_at(value, self.at) == Some(self.is)
    }
}

fn suffixed(project: &str, id: &str) -> String {
    let start = id.char_indices().rev().nth(1).map_or(0, |(index, _)| index);
    let suffix = &id[start..];
    if suffix.is_empty() {
        project.to_string()
    } else {
        format!("{project}-{suffix}")
    }
}

impl Registry {
    fn read(&self, root: &Path) -> Registered {
        match self {
            Self::Muse => muse_registry(&root.join("runtime").join("muse").join("sessions")),
        }
    }
}

#[derive(Deserialize)]
struct MuseRegistryFile {
    session_id: Option<String>,
    session_name: Option<String>,
    workspace_label: Option<String>,
}

fn muse_registry(dir: &Path) -> Registered {
    let mut out = Registered::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        if fs::metadata(&path)
            .ok()
            .is_none_or(|meta| meta.len() > REGISTRY_MAX_FILE_BYTES)
        {
            continue;
        }
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = serde_json::from_str::<MuseRegistryFile>(&body) else {
            continue;
        };
        let Some(id) = file
            .session_id
            .filter(|id| !id.trim().is_empty())
            .or_else(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_string)
            })
        else {
            continue;
        };
        out.insert(
            id,
            Registration {
                name: file.session_name.filter(|name| !name.trim().is_empty()),
                project: file
                    .workspace_label
                    .filter(|label| !label.trim().is_empty()),
                mtime_ms: mtime_ms(&path),
            },
        );
    }
    out
}

fn days(now: DateTime<Utc>) -> [NaiveDate; 3] {
    [1, 0, -1].map(|offset| (now + Duration::days(offset)).date_naive())
}

fn day_dir(root: &Path, day: NaiveDate) -> PathBuf {
    root.join(format!("{:04}", day.year()))
        .join(format!("{:02}", day.month()))
        .join(format!("{:02}", day.day()))
}

#[cfg(test)]
mod tests;
