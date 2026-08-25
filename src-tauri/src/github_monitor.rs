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
mod tests {
    use super::*;
    use crate::dto::{
        GithubPrListDto, GithubPrsData, GithubPrsDto, GithubStatus, MergeState, Mergeability,
    };
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
            mergeable: Mergeability::default(),
            merge_state: MergeState::default(),
            merge_queue: None,
            auto_merge: false,
            merge_kind: None,
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
        assert_eq!(ci_of(&state, "a"), Some(CiState::Failure));
    }

    #[test]
    fn ci_transitions_that_matter_raise_one_event_each() {
        let cases: [(CiState, CiState, Option<EventKind>); 12] = [
            (
                CiState::Pending,
                CiState::Failure,
                Some(EventKind::CiFailed),
            ),
            (CiState::Pending, CiState::Error, Some(EventKind::CiFailed)),
            (CiState::None, CiState::Failure, Some(EventKind::CiFailed)),
            (
                CiState::Success,
                CiState::Failure,
                Some(EventKind::CiFailed),
            ),
            (
                CiState::Pending,
                CiState::Success,
                Some(EventKind::CiPassed),
            ),
            (CiState::None, CiState::Success, Some(EventKind::CiPassed)),
            (
                CiState::Failure,
                CiState::Success,
                Some(EventKind::CiGreenAgain),
            ),
            (
                CiState::Error,
                CiState::Success,
                Some(EventKind::CiGreenAgain),
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
            assert_eq!(ci_of(&state, "a"), Some(after));
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
        let passed = Event {
            kind: EventKind::CiPassed,
            ..event.clone()
        };
        assert_eq!(notification_copy(&passed).0, "CI passed");
        let green = Event {
            kind: EventKind::CiGreenAgain,
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
        assert_eq!(state.seen.len(), 1);
        assert!(!state.seen.contains_key("a"));
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
        assert!(state.seen.is_empty());
    }

    #[test]
    fn a_failed_or_stale_read_leaves_the_baseline_alone() {
        let mut state = MonitorState::default();
        observe(&mut state, &read(vec![pr("a", CiState::Pending)]));
        let mut stale = read(vec![pr("a", CiState::Failure)]);
        stale.status = GithubStatus::Network;
        stale.stale = true;
        assert!(observe(&mut state, &stale).is_empty());
        assert_eq!(ci_of(&state, "a"), Some(CiState::Pending));
        let mut empty_failure = read(Vec::new());
        empty_failure.status = GithubStatus::GhNotLoggedIn;
        assert!(observe(&mut state, &empty_failure).is_empty());
        assert_eq!(state.seen.len(), 1, "a failure must not drop the baseline");
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
                ci_of(persisted, "a"),
                Some(CiState::Failure),
                "the persisted state already carries the transition"
            );
            Ok(())
        })
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(ci_of(&next, "a"), Some(CiState::Failure));
        assert_eq!(
            ci_of(&state, "a"),
            Some(CiState::Pending),
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
        assert!(load_state(&path).seen.is_empty());
        fs::write(&path, r#"{"schema_version":99,"seen":{}}"#).unwrap();
        assert!(load_state(&path).seen.is_empty());
        // The v0.2.0 file kept CI rollups only; it is dropped rather than migrated, so the poll
        // after an upgrade is a baseline and announces nothing.
        fs::write(&path, r#"{"schema_version":1,"ci":{"a":"failure"}}"#).unwrap();
        let mut upgraded = load_state(&path);
        assert!(upgraded.seen.is_empty());
        assert!(observe(&mut upgraded, &read(vec![pr("a", CiState::Success)])).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    fn ci_of(state: &MonitorState, id: &str) -> Option<CiState> {
        state.seen.get(id).map(|seen| seen.ci)
    }

    fn with_review(id: &str, decision: Option<ReviewDecision>) -> GithubPrDto {
        GithubPrDto {
            review_decision: decision,
            ..pr(id, CiState::Success)
        }
    }

    fn with_merge(id: &str, mergeable: Mergeability, state: MergeState) -> GithubPrDto {
        GithubPrDto {
            mergeable,
            merge_state: state,
            ..pr(id, CiState::Success)
        }
    }

    fn kinds(events: &[Event]) -> Vec<EventKind> {
        events.iter().map(|event| event.kind).collect()
    }

    #[test]
    fn a_review_decision_landing_raises_one_event() {
        use ReviewDecision::{Approved, ChangesRequested, ReviewRequired};
        let cases: [(
            Option<ReviewDecision>,
            Option<ReviewDecision>,
            Vec<EventKind>,
        ); 7] = [
            (
                Some(ReviewRequired),
                Some(Approved),
                vec![EventKind::Approved],
            ),
            (None, Some(Approved), vec![EventKind::Approved]),
            (
                Some(ChangesRequested),
                Some(Approved),
                vec![EventKind::Approved],
            ),
            (
                Some(ReviewRequired),
                Some(ChangesRequested),
                vec![EventKind::ChangesRequested],
            ),
            (
                Some(Approved),
                Some(ChangesRequested),
                vec![EventKind::ChangesRequested],
            ),
            (Some(Approved), Some(Approved), vec![]),
            (Some(Approved), Some(ReviewRequired), vec![]),
        ];
        for (before, after, expected) in cases {
            let mut state = MonitorState::default();
            observe(&mut state, &read(vec![with_review("a", before)]));
            let events = observe(&mut state, &read(vec![with_review("a", after)]));
            assert_eq!(kinds(&events), expected, "{before:?} -> {after:?}");
        }
    }

    #[test]
    fn conflicts_are_announced_when_they_appear_and_when_they_clear_never_from_unknown() {
        use MergeState::{Blocked, Dirty, Draft, Unknown as StateUnknown};
        use Mergeability::{Conflicting, Mergeable, Unknown};
        type Case = (
            (Mergeability, MergeState),
            (Mergeability, MergeState),
            Vec<EventKind>,
        );
        let cases: [Case; 7] = [
            (
                (Mergeable, Blocked),
                (Conflicting, Dirty),
                vec![EventKind::Conflicts],
            ),
            // A draft with conflicts: GitHub says DRAFT for the state, CONFLICTING for the merge.
            (
                (Mergeable, Draft),
                (Conflicting, Draft),
                vec![EventKind::Conflicts],
            ),
            // Dirty alone is conflicts too, whatever `mergeable` says.
            (
                (Mergeable, Blocked),
                (Mergeable, Dirty),
                vec![EventKind::Conflicts],
            ),
            (
                (Conflicting, Dirty),
                (Mergeable, Blocked),
                vec![EventKind::ConflictsResolved],
            ),
            ((Unknown, StateUnknown), (Conflicting, Dirty), vec![]),
            ((Conflicting, Dirty), (Unknown, StateUnknown), vec![]),
            ((Conflicting, Dirty), (Conflicting, Dirty), vec![]),
        ];
        for ((before_m, before_s), (after_m, after_s), expected) in cases {
            let mut state = MonitorState::default();
            observe(&mut state, &read(vec![with_merge("a", before_m, before_s)]));
            let events = observe(&mut state, &read(vec![with_merge("a", after_m, after_s)]));
            assert_eq!(
                kinds(&events),
                expected,
                "{before_m:?}/{before_s:?} -> {after_m:?}/{after_s:?}"
            );
        }
    }

    #[test]
    fn becoming_ready_to_merge_is_announced_once_and_speaks_for_the_good_news_with_it() {
        let mut state = MonitorState::default();
        let mut before = pr("a", CiState::Pending);
        before.review_decision = Some(ReviewDecision::ReviewRequired);
        before.merge_state = MergeState::Blocked;
        observe(&mut state, &read(vec![before]));

        let mut after = pr("a", CiState::Success);
        after.review_decision = Some(ReviewDecision::Approved);
        after.merge_state = MergeState::Clean;
        let events = observe(&mut state, &read(vec![after.clone()]));
        assert_eq!(kinds(&events), vec![EventKind::ReadyToMerge]);
        assert_eq!(
            notification_copy(&events[0]),
            (
                "Ready to merge".to_string(),
                "acme/app#41 · Add the thing".to_string()
            )
        );

        // Still ready on the next read: nothing new to say.
        assert!(observe(&mut state, &read(vec![after.clone()])).is_empty());

        // Clean but already owned by the merge queue or auto-merge is not "ready" for the user.
        let mut blocked = after.clone();
        blocked.merge_state = MergeState::Blocked;
        let mut queued = after.clone();
        queued.merge_queue = Some(crate::dto::GithubMergeQueueDto { position: Some(1) });
        let mut state = MonitorState::default();
        observe(&mut state, &read(vec![blocked.clone()]));
        assert!(observe(&mut state, &read(vec![queued])).is_empty());
        let mut auto = after;
        auto.auto_merge = true;
        let mut state = MonitorState::default();
        observe(&mut state, &read(vec![blocked]));
        assert!(observe(&mut state, &read(vec![auto])).is_empty());
    }

    #[test]
    fn bad_news_is_never_hidden_behind_ready_to_merge() {
        // Changes requested on a repository that does not require reviews: CLEAN and red at once.
        let mut state = MonitorState::default();
        observe(
            &mut state,
            &read(vec![with_merge(
                "a",
                Mergeability::Mergeable,
                MergeState::Blocked,
            )]),
        );
        let mut after = pr("a", CiState::Success);
        after.review_decision = Some(ReviewDecision::ChangesRequested);
        after.merge_state = MergeState::Clean;
        assert_eq!(
            kinds(&observe(&mut state, &read(vec![after]))),
            vec![EventKind::ChangesRequested, EventKind::ReadyToMerge]
        );
    }

    #[test]
    fn conflicts_that_appear_across_a_poll_that_saw_unknown_are_still_announced() {
        use MergeState::{Blocked, Dirty, Unknown as StateUnknown};
        use Mergeability::{Conflicting, Mergeable, Unknown};
        let mut state = MonitorState::default();
        observe(&mut state, &read(vec![with_merge("a", Mergeable, Blocked)]));
        // GitHub recomputes after a push; this poll lands before it has an answer.
        assert!(observe(
            &mut state,
            &read(vec![with_merge("a", Unknown, StateUnknown)])
        )
        .is_empty());
        let events = observe(&mut state, &read(vec![with_merge("a", Conflicting, Dirty)]));
        assert_eq!(kinds(&events), vec![EventKind::Conflicts]);
        // And the other way round: resolved across an unknown poll is still resolved.
        assert!(observe(
            &mut state,
            &read(vec![with_merge("a", Unknown, StateUnknown)])
        )
        .is_empty());
        let events = observe(&mut state, &read(vec![with_merge("a", Mergeable, Blocked)]));
        assert_eq!(kinds(&events), vec![EventKind::ConflictsResolved]);
    }

    #[test]
    fn a_poll_that_saw_unknown_does_not_re_announce_ready_to_merge() {
        let mut state = MonitorState::default();
        observe(
            &mut state,
            &read(vec![with_merge(
                "a",
                Mergeability::Mergeable,
                MergeState::Blocked,
            )]),
        );
        let clean = with_merge("a", Mergeability::Mergeable, MergeState::Clean);
        assert_eq!(
            kinds(&observe(&mut state, &read(vec![clean.clone()]))),
            vec![EventKind::ReadyToMerge]
        );
        // A base-branch push triggers a recompute; nothing else changed.
        assert!(observe(
            &mut state,
            &read(vec![with_merge(
                "a",
                Mergeability::Unknown,
                MergeState::Unknown
            )])
        )
        .is_empty());
        assert!(observe(&mut state, &read(vec![clean])).is_empty());
    }

    #[test]
    fn ready_to_merge_is_not_announced_when_the_baseline_never_knew() {
        let mut state = MonitorState::default();
        // Notifications switched on for a pull request GitHub has not computed yet.
        observe(
            &mut state,
            &read(vec![with_merge(
                "a",
                Mergeability::Unknown,
                MergeState::Unknown,
            )]),
        );
        let clean = with_merge("a", Mergeability::Mergeable, MergeState::Clean);
        assert!(observe(&mut state, &read(vec![clean.clone()])).is_empty());
        assert!(observe(&mut state, &read(vec![clean])).is_empty());
    }

    #[test]
    fn a_draft_reporting_clean_is_not_ready() {
        let mut state = MonitorState::default();
        let mut draft = with_merge("a", Mergeability::Mergeable, MergeState::Blocked);
        draft.is_draft = true;
        observe(&mut state, &read(vec![draft.clone()]));
        draft.merge_state = MergeState::Clean;
        assert!(observe(&mut state, &read(vec![draft])).is_empty());
    }

    #[test]
    fn several_facts_changing_at_once_are_all_announced_in_order() {
        let mut state = MonitorState::default();
        let mut before = with_merge("a", Mergeability::Mergeable, MergeState::Blocked);
        before.ci = CiState::Pending;
        before.review_decision = Some(ReviewDecision::ReviewRequired);
        observe(&mut state, &read(vec![before]));
        let mut after = with_merge("a", Mergeability::Conflicting, MergeState::Dirty);
        after.ci = CiState::Failure;
        after.review_decision = Some(ReviewDecision::ChangesRequested);
        assert_eq!(
            kinds(&observe(&mut state, &read(vec![after]))),
            vec![
                EventKind::CiFailed,
                EventKind::ChangesRequested,
                EventKind::Conflicts
            ]
        );
    }

    #[test]
    fn a_dismissed_and_renewed_approval_is_announced_again() {
        use ReviewDecision::{Approved, ReviewRequired};
        let mut state = MonitorState::default();
        observe(
            &mut state,
            &read(vec![with_review("a", Some(ReviewRequired))]),
        );
        assert_eq!(
            kinds(&observe(
                &mut state,
                &read(vec![with_review("a", Some(Approved))])
            )),
            vec![EventKind::Approved]
        );
        assert!(observe(
            &mut state,
            &read(vec![with_review("a", Some(ReviewRequired))])
        )
        .is_empty());
        assert_eq!(
            kinds(&observe(
                &mut state,
                &read(vec![with_review("a", Some(Approved))])
            )),
            vec![EventKind::Approved]
        );
    }

    #[test]
    fn a_persisted_record_with_every_fact_round_trips() {
        let root = scratch_dir("github-monitor-full-round-trip");
        let path = root.join("monitor.json");
        let mut state = MonitorState::default();
        let mut seen = with_merge("a", Mergeability::Conflicting, MergeState::Dirty);
        seen.review_decision = Some(ReviewDecision::Approved);
        observe(&mut state, &read(vec![seen.clone()]));
        save_state(&path, &state).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        for needle in [
            r#""review":"APPROVED""#,
            r#""conflicts":true"#,
            r#""ready":false"#,
        ] {
            assert!(raw.contains(needle), "{needle} missing from {raw}");
        }

        let mut reloaded = load_state(&path);
        assert!(observe(&mut reloaded, &read(vec![seen])).is_empty());
        let mut resolved = with_merge("a", Mergeability::Mergeable, MergeState::Blocked);
        resolved.review_decision = Some(ReviewDecision::Approved);
        assert_eq!(
            kinds(&observe(&mut reloaded, &read(vec![resolved]))),
            vec![EventKind::ConflictsResolved]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn every_event_has_notification_copy() {
        let event = Event {
            kind: EventKind::Approved,
            repo: "acme/app".into(),
            number: 41,
            title: "Add the thing".into(),
        };
        let titles: Vec<String> = [
            EventKind::CiFailed,
            EventKind::CiPassed,
            EventKind::CiGreenAgain,
            EventKind::Approved,
            EventKind::ChangesRequested,
            EventKind::Conflicts,
            EventKind::ConflictsResolved,
            EventKind::ReadyToMerge,
        ]
        .into_iter()
        .map(|kind| {
            notification_copy(&Event {
                kind,
                ..event.clone()
            })
            .0
        })
        .collect();
        assert_eq!(
            titles,
            [
                "CI failed",
                "CI passed",
                "CI green again",
                "Approved",
                "Changes requested",
                "Merge conflicts",
                "Conflicts resolved",
                "Ready to merge"
            ]
        );
    }
}
