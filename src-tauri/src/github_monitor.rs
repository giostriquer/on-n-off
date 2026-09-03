//! Background watcher for the user's own pull requests: when, between two reads, the head
//! commit's CI goes red or green, the review decision lands, conflicts appear or clear, or the
//! pull request becomes ready to merge, a tray notification says so. Opt-in
//! (`github_notifications`), polls on the same interval as the screen and shares its in-memory
//! result, and remembers the last-seen state on disk so a restart never re-notifies. The merge
//! fields are read through `github::merge`, the same classification the screen shows.

use std::{collections::HashMap, path::Path, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::{async_runtime, AppHandle};

use crate::dto::{CiState, GithubPrDto, GithubPrsDto, GithubStatus, ReviewDecision};
use crate::github::merge;
use crate::monitor::{self, wait_for_wake_or_deadline};

const DISABLED_WAKE: Duration = Duration::from_secs(60 * 60);
/// The Limits monitor, the Keychain probe and the first provider load all run at startup; the
/// first GitHub poll waits so it does not join that burst. A settings change wakes it sooner.
const FIRST_POLL_DELAY: Duration = Duration::from_secs(20);
const MAX_BACKOFF: Duration = Duration::from_secs(10 * 60);
/// Version 1 kept CI rollups only; an older file is dropped, so the first poll after an upgrade
/// is a baseline and announces nothing.
const MONITOR_STATE_SCHEMA_VERSION: u8 = 2;

/// Marker for this monitor's wake channel.
pub struct GithubMonitor;

/// What was last seen of each own pull request, keyed by GitHub node id.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct MonitorState {
    schema_version: u8,
    seen: HashMap<String, Seen>,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            schema_version: MONITOR_STATE_SCHEMA_VERSION,
            seen: HashMap::new(),
        }
    }
}

/// The facts about one pull request that the monitor compares between reads. The two merge
/// facts are `None` while GitHub has not computed them, and a poll that sees `None` keeps the
/// last computed answer (`or_last_known`), so the baseline only ever moves on facts.
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
    /// The same reading of the merge fields the screen shows (`github::merge`).
    fn of(pr: &GithubPrDto) -> Self {
        Self {
            ci: pr.ci,
            review: pr.review_decision,
            conflicts: merge::conflicts_known(pr),
            ready: merge::ready_known(pr),
        }
    }

    /// GitHub recomputes mergeability after every push and answers "unknown" until it is done;
    /// carrying the previous answer through such a poll keeps "conflicts appeared" and "ready to
    /// merge" from being lost or repeated around it.
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
    /// Checks went red (from pending, no checks, or green).
    CiFailed,
    /// Checks went green without having been red since this pull request was last seen.
    CiPassed,
    /// Checks went green after being red.
    CiGreenAgain,
    /// The review decision became "approved".
    Approved,
    /// The review decision became "changes requested".
    ChangesRequested,
    /// The head stopped merging cleanly into the base.
    Conflicts,
    /// The head merges cleanly again.
    ConflictsResolved,
    /// Everything the base branch asks for is in place; only the merge button is left.
    ReadyToMerge,
}

impl EventKind {
    /// Good news that "ready to merge" already implies; announced alone when it arrives together.
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
}

pub fn setup(app: &mut tauri::App) {
    monitor::spawn::<GithubMonitor, _, _>(app, run);
}

/// Wake the poll loop after a settings change.
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
            if !state.seen.is_empty() {
                state.seen.clear();
                if let Err(error) = monitor::persist_state(&state_path, &state).await {
                    eprintln!("github monitor could not clear its state: {error}");
                }
            }
            DISABLED_WAKE
        };

        wait_for_wake_or_deadline(&mut wake_receiver, delay).await;
    }
}

/// One poll; `Ok(true)` when the read did not come back `ok` (drives the backoff). State is
/// persisted before any notification is shown, so nothing is announced that is not recorded.
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
        monitor::notify(app, "github monitor", title, body);
    }
    Ok(failed)
}

/// One step of the monitor: the baseline that `prs` implies, persisted through `persist`
/// before the events it produced are handed back. On a persist failure nothing is returned, so
/// the caller keeps its old state and never announces a transition it has not recorded.
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
    };
    (
        title.to_string(),
        format!("{}#{} · {}", event.repo, event.number, event.title),
    )
}

/// Compare a read against what was last seen of the user's own pull requests. Only a fresh,
/// successful read moves the baseline: a failed or stale one changes nothing, and a pull request
/// that is no longer open is simply forgotten.
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
            events.extend(transitions(*before, after).into_iter().map(|kind| Event {
                kind,
                repo: pr.repo.clone(),
                number: pr.number,
                title: pr.title.clone(),
            }));
        }
        next.insert(pr.id.clone(), after);
    }
    state.seen = next;
    events
}

/// Every transition worth a notification between two sightings of one pull request. A merge
/// fact that is unknown on either side says nothing, and "ready to merge" speaks for the good
/// news that arrived with it.
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
