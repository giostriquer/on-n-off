//! Background watcher for CI on the user's own pull requests: when the head commit's rollup goes
//! red or green between two reads, a tray notification says so. Opt-in (`github_notifications`),
//! polls on the same interval as the screen and shares its in-memory result, and remembers the
//! last-seen state on disk so a restart never re-notifies.

use std::{collections::HashMap, path::Path, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::{async_runtime, AppHandle};

use crate::dto::{CiState, GithubPrsDto, GithubStatus};
use crate::monitor::{self, wait_for_wake_or_deadline};

const DISABLED_WAKE: Duration = Duration::from_secs(60 * 60);
/// The Limits monitor, the Keychain probe and the first provider load all run at startup; the
/// first GitHub poll waits so it does not join that burst. A settings change wakes it sooner.
const FIRST_POLL_DELAY: Duration = Duration::from_secs(20);
const MAX_BACKOFF: Duration = Duration::from_secs(10 * 60);
const MONITOR_STATE_SCHEMA_VERSION: u8 = 1;

/// Marker for this monitor's wake channel.
pub struct GithubMonitor;

/// Last-seen CI rollup per own pull request, keyed by GitHub node id.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct MonitorState {
    schema_version: u8,
    ci: HashMap<String, CiState>,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            schema_version: MONITOR_STATE_SCHEMA_VERSION,
            ci: HashMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CiEventKind {
    /// Checks went red (from pending, no checks, or green).
    Failed,
    /// Checks went green without having been red since this pull request was last seen.
    Passed,
    /// Checks went green after being red.
    GreenAgain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CiEvent {
    kind: CiEventKind,
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
            if !state.ci.is_empty() {
                state.ci.clear();
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
) -> Result<(MonitorState, Vec<CiEvent>), String> {
    let mut next = state.clone();
    let events = observe(&mut next, prs);
    persist(&next).map_err(|error| format!("could not save state: {error}"))?;
    Ok((next, events))
}

fn notification_copy(event: &CiEvent) -> (String, String) {
    let title = match event.kind {
        CiEventKind::Failed => "CI failed",
        CiEventKind::Passed => "CI passed",
        CiEventKind::GreenAgain => "CI green again",
    };
    (
        title.to_string(),
        format!("{}#{} · {}", event.repo, event.number, event.title),
    )
}

/// Compare a read against the last-seen rollups of the user's own pull requests. Only a fresh,
/// successful read moves the baseline: a failed or stale one changes nothing, and a pull request
/// that is no longer open is simply forgotten.
fn observe(state: &mut MonitorState, prs: &GithubPrsDto) -> Vec<CiEvent> {
    if prs.status != GithubStatus::Ok || prs.stale {
        return Vec::new();
    }
    let mut events = Vec::new();
    let mut next = HashMap::with_capacity(prs.data.mine.items.len());
    for pr in &prs.data.mine.items {
        if let Some(kind) = state
            .ci
            .get(&pr.id)
            .and_then(|before| transition(*before, pr.ci))
        {
            events.push(CiEvent {
                kind,
                repo: pr.repo.clone(),
                number: pr.number,
                title: pr.title.clone(),
            });
        }
        next.insert(pr.id.clone(), pr.ci);
    }
    state.ci = next;
    events
}

fn transition(before: CiState, after: CiState) -> Option<CiEventKind> {
    use CiState::{Error, Failure, None as NoChecks, Pending, Success};
    match (before, after) {
        (Pending | NoChecks | Success, Failure | Error) => Some(CiEventKind::Failed),
        (Pending | NoChecks, Success) => Some(CiEventKind::Passed),
        (Failure | Error, Success) => Some(CiEventKind::GreenAgain),
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
mod tests {
    use super::*;
    use crate::dto::{GithubPrDto, GithubPrListDto, GithubPrsData, GithubPrsDto, GithubStatus};
    use crate::paths::scratch_dir;
    use std::fs;

    fn pr(id: &str, ci: CiState) -> GithubPrDto {
        GithubPrDto {
            id: id.into(),
            number: 41,
            title: "Add the thing".into(),
            url: format!("https://github.com/acme/app/pull/{id}"),
            repo: "acme/app".into(),
            author: "octocat".into(),
            is_draft: false,
            review_decision: None,
            ci,
            head_ref: "feat/thing".into(),
            base_ref: "main".into(),
            updated_at: "2026-08-24T20:00:00Z".into(),
            review_request: None,
        }
    }

    fn read(mine: Vec<GithubPrDto>) -> GithubPrsDto {
        GithubPrsDto {
            status: GithubStatus::Ok,
            hint: None,
            stale: false,
            data: GithubPrsData {
                viewer: Some("octocat".into()),
                fetched_at: Some("2026-08-24T20:00:00Z".into()),
                mine: GithubPrListDto {
                    total: mine.len() as u64,
                    items: mine,
                },
                ..GithubPrsData::default()
            },
            warnings: Vec::new(),
        }
    }

    #[test]
    fn the_first_observation_is_only_a_baseline() {
        let mut state = MonitorState::default();
        let events = observe(&mut state, &read(vec![pr("a", CiState::Failure)]));
        assert!(events.is_empty());
        assert_eq!(state.ci.get("a"), Some(&CiState::Failure));
    }

    #[test]
    fn ci_transitions_that_matter_raise_one_event_each() {
        let cases: [(CiState, CiState, Option<CiEventKind>); 12] = [
            (
                CiState::Pending,
                CiState::Failure,
                Some(CiEventKind::Failed),
            ),
            (CiState::Pending, CiState::Error, Some(CiEventKind::Failed)),
            (CiState::None, CiState::Failure, Some(CiEventKind::Failed)),
            (
                CiState::Success,
                CiState::Failure,
                Some(CiEventKind::Failed),
            ),
            (
                CiState::Pending,
                CiState::Success,
                Some(CiEventKind::Passed),
            ),
            (CiState::None, CiState::Success, Some(CiEventKind::Passed)),
            (
                CiState::Failure,
                CiState::Success,
                Some(CiEventKind::GreenAgain),
            ),
            (
                CiState::Error,
                CiState::Success,
                Some(CiEventKind::GreenAgain),
            ),
            (CiState::Success, CiState::Success, None),
            (CiState::Failure, CiState::Error, None),
            (CiState::Success, CiState::Pending, None),
            (CiState::Failure, CiState::None, None),
        ];
        for (before, after, expected) in cases {
            let mut state = MonitorState::default();
            observe(&mut state, &read(vec![pr("a", before)]));
            let events = observe(&mut state, &read(vec![pr("a", after)]));
            assert_eq!(
                events.iter().map(|event| event.kind).collect::<Vec<_>>(),
                expected.into_iter().collect::<Vec<_>>(),
                "{before:?} -> {after:?}"
            );
            assert_eq!(state.ci.get("a"), Some(&after));
        }
    }

    #[test]
    fn events_carry_the_pull_request_and_read_as_notifications() {
        let mut state = MonitorState::default();
        observe(&mut state, &read(vec![pr("a", CiState::Pending)]));
        let events = observe(&mut state, &read(vec![pr("a", CiState::Failure)]));
        let event = &events[0];
        assert_eq!(event.repo, "acme/app");
        assert_eq!(event.number, 41);
        assert_eq!(event.title, "Add the thing");
        assert_eq!(
            notification_copy(event),
            (
                "CI failed".to_string(),
                "acme/app#41 · Add the thing".to_string()
            )
        );
        let passed = CiEvent {
            kind: CiEventKind::Passed,
            ..event.clone()
        };
        assert_eq!(notification_copy(&passed).0, "CI passed");
        let green = CiEvent {
            kind: CiEventKind::GreenAgain,
            ..event.clone()
        };
        assert_eq!(notification_copy(&green).0, "CI green again");
    }

    #[test]
    fn a_merged_or_closed_pull_request_is_forgotten_silently() {
        let mut state = MonitorState::default();
        observe(
            &mut state,
            &read(vec![pr("a", CiState::Failure), pr("b", CiState::Pending)]),
        );
        let events = observe(&mut state, &read(vec![pr("b", CiState::Pending)]));
        assert!(events.is_empty());
        assert_eq!(state.ci.len(), 1);
        assert!(!state.ci.contains_key("a"));
    }

    #[test]
    fn only_the_users_own_pull_requests_are_watched() {
        let mut state = MonitorState::default();
        let mut first = read(Vec::new());
        first.data.review_requested.items = vec![pr("r", CiState::Pending)];
        first.data.assigned.items = vec![pr("s", CiState::Pending)];
        observe(&mut state, &first);
        let mut second = read(Vec::new());
        second.data.review_requested.items = vec![pr("r", CiState::Failure)];
        second.data.assigned.items = vec![pr("s", CiState::Failure)];
        assert!(observe(&mut state, &second).is_empty());
        assert!(state.ci.is_empty());
    }

    #[test]
    fn a_failed_or_stale_read_leaves_the_baseline_alone() {
        let mut state = MonitorState::default();
        observe(&mut state, &read(vec![pr("a", CiState::Pending)]));
        let mut stale = read(vec![pr("a", CiState::Failure)]);
        stale.status = GithubStatus::Network;
        stale.stale = true;
        assert!(observe(&mut state, &stale).is_empty());
        assert_eq!(state.ci.get("a"), Some(&CiState::Pending));
        let mut empty_failure = read(Vec::new());
        empty_failure.status = GithubStatus::GhNotLoggedIn;
        assert!(observe(&mut state, &empty_failure).is_empty());
        assert_eq!(state.ci.len(), 1, "a failure must not drop the baseline");
        let events = observe(&mut state, &read(vec![pr("a", CiState::Failure)]));
        assert_eq!(
            events.len(),
            1,
            "the transition is still reported once the read works"
        );
    }

    #[test]
    fn draft_pull_requests_count_too() {
        let mut state = MonitorState::default();
        let mut draft = pr("a", CiState::Pending);
        draft.is_draft = true;
        observe(&mut state, &read(vec![draft.clone()]));
        draft.ci = CiState::Success;
        assert_eq!(observe(&mut state, &read(vec![draft])).len(), 1);
    }

    #[test]
    fn the_baseline_moves_only_once_the_new_state_is_persisted() {
        let mut state = MonitorState::default();
        observe(&mut state, &read(vec![pr("a", CiState::Pending)]));
        let red = read(vec![pr("a", CiState::Failure)]);

        let (next, events) = advance(&state, &red, |persisted| {
            assert_eq!(
                persisted.ci.get("a"),
                Some(&CiState::Failure),
                "the persisted state already carries the transition"
            );
            Ok(())
        })
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(next.ci.get("a"), Some(&CiState::Failure));
        assert_eq!(
            state.ci.get("a"),
            Some(&CiState::Pending),
            "the caller commits `next`"
        );

        let error = advance(&state, &red, |_| Err("disk full".to_string())).unwrap_err();
        assert!(error.contains("disk full"), "{error}");
    }

    #[test]
    fn failure_backoff_doubles_and_caps_at_ten_minutes() {
        assert_eq!(poll_delay(60, 0), Duration::from_secs(60));
        assert_eq!(poll_delay(60, 1), Duration::from_secs(120));
        assert_eq!(poll_delay(60, 3), Duration::from_secs(480));
        assert_eq!(poll_delay(60, 4), Duration::from_secs(600));
        assert_eq!(poll_delay(300, 9), Duration::from_secs(600));
    }

    #[test]
    fn persisted_observations_prevent_duplicate_notifications_after_restart() {
        let root = scratch_dir("github-monitor-round-trip");
        let path = root.join("monitor.json");
        let mut state = MonitorState::default();
        observe(&mut state, &read(vec![pr("a", CiState::Pending)]));
        let after = read(vec![pr("a", CiState::Failure)]);
        assert_eq!(observe(&mut state, &after).len(), 1);
        save_state(&path, &state).unwrap();

        let mut reloaded = load_state(&path);

        assert!(observe(&mut reloaded, &after).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_or_foreign_monitor_state_falls_back_to_an_empty_baseline() {
        let root = scratch_dir("github-monitor-malformed");
        let path = root.join("monitor.json");
        fs::write(&path, "{nope").unwrap();
        assert!(load_state(&path).ci.is_empty());
        fs::write(&path, r#"{"schema_version":99,"ci":{"a":"failure"}}"#).unwrap();
        assert!(load_state(&path).ci.is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
