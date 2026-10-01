use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{async_runtime, AppHandle};

use crate::dto::{CiState, GithubPrDto, GithubPrsDto, GithubStatus, ReviewDecision};
use crate::github::merge;
use crate::monitor::{self, wait_for_wake_or_deadline};
use crate::notifications::Sound;

const DISABLED_WAKE: Duration = Duration::from_secs(60 * 60);
const FIRST_POLL_DELAY: Duration = Duration::from_secs(20);
const MAX_BACKOFF: Duration = Duration::from_secs(10 * 60);
const MONITOR_STATE_SCHEMA_VERSION: u8 = 2;

pub struct GithubMonitor;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MonitorState {
    schema_version: u8,
    seen: HashMap<String, Seen>,
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    vanished: HashSet<String>,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            schema_version: MONITOR_STATE_SCHEMA_VERSION,
            seen: HashMap::new(),
            vanished: HashSet::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Seen {
    ci: CiState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review: Option<ReviewDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    conflicts: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ready: Option<bool>,
}

impl Seen {
    fn of(pr: &GithubPrDto) -> Self {
        Self {
            ci: pr.ci,
            review: pr.review_decision,
            conflicts: merge::conflicts_known(pr),
            ready: merge::ready_known(pr),
        }
    }

    fn or_last_known(self, before: Option<&Seen>) -> Self {
        let Some(before) = before else {
            return self;
        };
        Self {
            conflicts: self.conflicts.or(before.conflicts),
            ready: self.ready.or(before.ready),
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EventKind {
    CiFailed,
    CiPassed,
    CiGreenAgain,
    Approved,
    ChangesRequested,
    Conflicts,
    ConflictsResolved,
    ReadyToMerge,
    Merged,
}

impl EventKind {
    fn is_subsumed_by_ready(self) -> bool {
        matches!(
            self,
            Self::CiPassed | Self::CiGreenAgain | Self::Approved | Self::ConflictsResolved
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Event {
    kind: EventKind,
    repo: String,
    number: u64,
    title: String,
    sound: Sound,
}

impl Event {
    fn of(kind: EventKind, pr: &GithubPrDto, sound: Sound) -> Self {
        Self {
            kind,
            repo: pr.repo.clone(),
            number: pr.number,
            title: pr.title.clone(),
            sound,
        }
    }
}

pub fn setup(app: &mut tauri::App) {
    monitor::spawn::<GithubMonitor, _, _>(app, run);
}

pub fn wake(app: &AppHandle) {
    monitor::wake::<GithubMonitor>(app);
}

async fn run(app: AppHandle, mut wake_receiver: async_runtime::Receiver<()>) {
    let state_path = match crate::paths::github_monitor_state_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!(
                "github monitor could not resolve its state path: {}",
                error.message
            );
            return;
        }
    };
    let load_path = state_path.clone();
    let mut state = async_runtime::spawn_blocking(move || load_state(&load_path))
        .await
        .unwrap_or_default();
    let mut consecutive_failures = 0_u32;

    wait_for_wake_or_deadline(&mut wake_receiver, FIRST_POLL_DELAY).await;
    loop {
        let settings = async_runtime::spawn_blocking(crate::settings::load_settings)
            .await
            .unwrap_or_default();
        let delay = if settings.github_notifications {
            let failed = match poll_once(&app, &state_path, &mut state).await {
                Ok(failed) => failed,
                Err(error) => {
                    eprintln!("github monitor poll failed: {error}");
                    true
                }
            };
            if failed {
                consecutive_failures = consecutive_failures.saturating_add(1);
            } else {
                consecutive_failures = 0;
            }
            poll_delay(settings.github_poll_seconds, consecutive_failures)
        } else {
            consecutive_failures = 0;
            if !state.seen.is_empty() || !state.vanished.is_empty() {
                state.seen.clear();
                state.vanished.clear();
                if let Err(error) = monitor::persist_state(&state_path, &state).await {
                    eprintln!("github monitor could not clear its state: {error}");
                }
            }
            DISABLED_WAKE
        };

        wait_for_wake_or_deadline(&mut wake_receiver, delay).await;
    }
}

async fn poll_once(
    app: &AppHandle,
    state_path: &Path,
    state: &mut MonitorState,
) -> Result<bool, String> {
    let prs = async_runtime::spawn_blocking(|| crate::github::read_prs(false))
        .await
        .map_err(|error| format!("github worker failed: {error}"))?;
    let failed = prs.status != GithubStatus::Ok;
    let current = state.clone();
    let path = state_path.to_path_buf();
    let (next, events) = async_runtime::spawn_blocking(move || {
        advance(&current, &prs, |next| save_state(&path, next))
    })
    .await
    .map_err(|error| format!("state worker failed: {error}"))??;
    *state = next;
    for event in events {
        let (title, body) = notification_copy(&event);
        monitor::notify(app, "github monitor", title, body, event.sound);
    }
    Ok(failed)
}

fn advance(
    state: &MonitorState,
    prs: &GithubPrsDto,
    persist: impl FnOnce(&MonitorState) -> Result<(), String>,
) -> Result<(MonitorState, Vec<Event>), String> {
    let mut next = state.clone();
    let events = observe(&mut next, prs);
    persist(&next).map_err(|error| format!("could not save state: {error}"))?;
    Ok((next, events))
}

fn notification_copy(event: &Event) -> (String, String) {
    let title = match event.kind {
        EventKind::CiFailed => "CI failed",
        EventKind::CiPassed => "CI passed",
        EventKind::CiGreenAgain => "CI green again",
        EventKind::Approved => "Approved",
        EventKind::ChangesRequested => "Changes requested",
        EventKind::Conflicts => "Merge conflicts",
        EventKind::ConflictsResolved => "Conflicts resolved",
        EventKind::ReadyToMerge => "Ready to merge",
        EventKind::Merged => "Merged",
    };
    (
        title.to_string(),
        format!("{}#{} · {}", event.repo, event.number, event.title),
    )
}

fn sound_for(kind: EventKind, after: Seen) -> Sound {
    let green = after.ci == CiState::Success && after.conflicts != Some(true);
    match kind {
        EventKind::Merged => Sound::Done,
        EventKind::ReadyToMerge => Sound::Success,
        EventKind::CiPassed | EventKind::CiGreenAgain | EventKind::ConflictsResolved if green => {
            Sound::Success
        }
        _ => Sound::Default,
    }
}

fn observe(state: &mut MonitorState, prs: &GithubPrsDto) -> Vec<Event> {
    if prs.status != GithubStatus::Ok || prs.stale {
        return Vec::new();
    }
    let mut events = Vec::new();
    let mut next = HashMap::with_capacity(prs.data.mine.items.len());
    for pr in &prs.data.mine.items {
        let before = state.seen.get(&pr.id);
        let after = Seen::of(pr).or_last_known(before);
        if let Some(before) = before {
            events.extend(
                transitions(*before, after)
                    .into_iter()
                    .map(|kind| Event::of(kind, pr, sound_for(kind, after))),
            );
        }
        next.insert(pr.id.clone(), after);
    }
    let mut vanished: HashSet<String> = state
        .seen
        .keys()
        .cloned()
        .chain(state.vanished.drain())
        .filter(|id| !next.contains_key(id))
        .collect();
    for pr in &prs.data.merged.items {
        if vanished.remove(&pr.id) {
            events.push(Event::of(
                EventKind::Merged,
                pr,
                sound_for(EventKind::Merged, Seen::of(pr)),
            ));
        }
    }
    vanished.retain(|id| state.seen.contains_key(id));
    state.vanished = vanished;
    state.seen = next;
    events
}

fn transitions(before: Seen, after: Seen) -> Vec<EventKind> {
    let mut kinds = Vec::new();
    kinds.extend(ci_transition(before.ci, after.ci));
    if after.review != before.review {
        match after.review {
            Some(ReviewDecision::Approved) => kinds.push(EventKind::Approved),
            Some(ReviewDecision::ChangesRequested) => kinds.push(EventKind::ChangesRequested),
            Some(ReviewDecision::ReviewRequired) | None => {}
        }
    }
    match (before.conflicts, after.conflicts) {
        (Some(false), Some(true)) => kinds.push(EventKind::Conflicts),
        (Some(true), Some(false)) => kinds.push(EventKind::ConflictsResolved),
        _ => {}
    }
    if (before.ready, after.ready) == (Some(false), Some(true)) {
        kinds.retain(|kind| !kind.is_subsumed_by_ready());
        kinds.push(EventKind::ReadyToMerge);
    }
    kinds
}

fn ci_transition(before: CiState, after: CiState) -> Option<EventKind> {
    use CiState::{Error, Failure, None as NoChecks, Pending, Success};
    match (before, after) {
        (Pending | NoChecks | Success, Failure | Error) => Some(EventKind::CiFailed),
        (Pending | NoChecks, Success) => Some(EventKind::CiPassed),
        (Failure | Error, Success) => Some(EventKind::CiGreenAgain),
        _ => None,
    }
}

fn poll_delay(base_seconds: u16, consecutive_failures: u32) -> Duration {
    monitor::backoff(
        Duration::from_secs(u64::from(base_seconds)),
        consecutive_failures,
        MAX_BACKOFF,
    )
}

fn load_state(path: &Path) -> MonitorState {
    monitor::load_state(path, |state: &MonitorState| {
        state.schema_version == MONITOR_STATE_SCHEMA_VERSION
    })
}

fn save_state(path: &Path, state: &MonitorState) -> Result<(), String> {
    monitor::save_state(path, state)
}

#[cfg(test)]
mod tests;
