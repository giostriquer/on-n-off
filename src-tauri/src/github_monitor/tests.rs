//! Tests for `github_monitor`, kept beside it so the module itself stays readable.

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
    read_with_merged(mine, Vec::new())
}

fn read_with_merged(mine: Vec<GithubPrDto>, merged: Vec<GithubPrDto>) -> GithubPrsDto {
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
            merged: GithubPrListDto {
                total: merged.len() as u64,
                items: merged,
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
fn a_closed_pull_request_is_forgotten_silently() {
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
fn a_merged_pull_request_is_announced_once_with_the_merged_sound_then_forgotten() {
    let mut state = MonitorState::default();
    observe(
        &mut state,
        &read(vec![pr("a", CiState::Success), pr("b", CiState::Pending)]),
    );
    let merged = read_with_merged(
        vec![pr("b", CiState::Pending)],
        vec![pr("a", CiState::Success)],
    );
    let events = observe(&mut state, &merged);
    assert_eq!(kinds(&events), [EventKind::Merged]);
    assert_eq!(events[0].repo, "acme/app");
    assert_eq!(events[0].number, 41);
    assert_eq!(notification_copy(&events[0]).0, "Merged");
    assert_eq!(events[0].sound, Sound::Done);
    assert!(!state.seen.contains_key("a"));

    // The merged list keeps naming it on later polls; it was announced when it left `seen`.
    assert!(observe(&mut state, &merged).is_empty());
}

#[test]
fn a_merge_the_merged_search_reports_one_poll_late_is_still_announced_but_not_two() {
    for (polls_late, announced) in [(1, true), (2, false)] {
        let mut state = MonitorState::default();
        observe(&mut state, &read(vec![pr("a", CiState::Success)]));
        for _ in 0..polls_late {
            assert!(observe(&mut state, &read(vec![])).is_empty());
        }
        let late = read_with_merged(vec![], vec![pr("a", CiState::Success)]);
        let events = observe(&mut state, &late);
        assert_eq!(!events.is_empty(), announced, "{polls_late} polls late");
        assert!(state.vanished.is_empty());
        assert!(observe(&mut state, &late).is_empty());
    }
}

#[test]
fn a_pull_request_that_flickers_back_into_the_open_list_is_announced_merged_once() {
    let mut state = MonitorState::default();
    let open = pr("a", CiState::Success);
    observe(&mut state, &read(vec![open.clone()]));
    assert!(observe(&mut state, &read(vec![])).is_empty());
    // A stale open-list replica still lists it while the merged list already names it.
    let flicker = read_with_merged(vec![open.clone()], vec![open.clone()]);
    assert!(observe(&mut state, &flicker).is_empty());
    assert!(state.vanished.is_empty());
    let gone = read_with_merged(vec![], vec![open]);
    assert_eq!(kinds(&observe(&mut state, &gone)), [EventKind::Merged]);
    assert!(observe(&mut state, &gone).is_empty());
}

#[test]
fn a_closed_pull_request_is_remembered_for_one_poll_only() {
    let mut state = MonitorState::default();
    observe(&mut state, &read(vec![pr("a", CiState::Success)]));
    observe(&mut state, &read(vec![]));
    assert!(state.vanished.contains("a"));
    let raw = serde_json::to_string(&state).unwrap();
    assert!(raw.contains("\"vanished\":[\"a\"]"), "{raw}");
    observe(&mut state, &read(vec![]));
    assert!(state.vanished.is_empty());
    let raw = serde_json::to_string(&state).unwrap();
    assert!(!raw.contains("vanished"), "{raw}");
}

#[test]
fn a_pull_request_merged_before_it_was_ever_seen_open_is_not_announced() {
    let mut state = MonitorState::default();
    let first = read_with_merged(
        vec![pr("b", CiState::Pending)],
        vec![pr("z", CiState::Success)],
    );
    assert!(observe(&mut state, &first).is_empty());
    assert!(observe(&mut state, &first).is_empty());
}

#[test]
fn going_green_without_conflicts_gets_the_green_sound_and_everything_else_the_default() {
    let mut state = MonitorState::default();
    observe(&mut state, &read(vec![pr("a", CiState::Pending)]));
    let events = observe(&mut state, &read(vec![pr("a", CiState::Success)]));
    assert_eq!(kinds(&events), [EventKind::CiPassed]);
    assert_eq!(events[0].sound, Sound::Success);

    let mut state = MonitorState::default();
    let mut pending_and_dirty = with_merge("a", Mergeability::Conflicting, MergeState::Dirty);
    pending_and_dirty.ci = CiState::Pending;
    observe(&mut state, &read(vec![pending_and_dirty]));
    let green_but_dirty = with_merge("a", Mergeability::Conflicting, MergeState::Dirty);
    let events = observe(&mut state, &read(vec![green_but_dirty]));
    assert_eq!(kinds(&events), [EventKind::CiPassed]);
    assert_eq!(events[0].sound, Sound::Default);

    let mut state = MonitorState::default();
    observe(&mut state, &read(vec![pr("a", CiState::Success)]));
    let events = observe(&mut state, &read(vec![pr("a", CiState::Failure)]));
    assert_eq!(kinds(&events), [EventKind::CiFailed]);
    assert_eq!(events[0].sound, Sound::Default);
}

#[test]
fn conflicts_clearing_on_a_green_pull_request_sounds_green_on_a_pending_one_it_does_not() {
    for (ci, expected) in [
        (CiState::Success, Sound::Success),
        (CiState::Pending, Sound::Default),
    ] {
        let mut state = MonitorState::default();
        let mut dirty = with_merge("a", Mergeability::Conflicting, MergeState::Dirty);
        dirty.ci = ci;
        observe(&mut state, &read(vec![dirty]));
        let mut clean = with_merge("a", Mergeability::Mergeable, MergeState::Blocked);
        clean.ci = ci;
        let events = observe(&mut state, &read(vec![clean]));
        assert_eq!(kinds(&events), [EventKind::ConflictsResolved]);
        assert_eq!(events[0].sound, expected, "{ci:?}");
    }
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
fn ready_to_merge_sounds_green_and_approval_alone_does_not() {
    let mut state = MonitorState::default();
    let mut blocked = with_merge("a", Mergeability::Mergeable, MergeState::Blocked);
    blocked.ci = CiState::Success;
    observe(&mut state, &read(vec![blocked]));
    let mut ready = with_merge("a", Mergeability::Mergeable, MergeState::Clean);
    ready.ci = CiState::Success;
    ready.review_decision = Some(ReviewDecision::Approved);
    let events = observe(&mut state, &read(vec![ready]));
    assert_eq!(kinds(&events), [EventKind::ReadyToMerge]);
    assert_eq!(events[0].sound, Sound::Success);

    // A repository without checks is ready without ever being "green".
    let mut state = MonitorState::default();
    let mut blocked = with_merge("a", Mergeability::Mergeable, MergeState::Blocked);
    blocked.ci = CiState::None;
    observe(&mut state, &read(vec![blocked]));
    let mut ready = with_merge("a", Mergeability::Mergeable, MergeState::Clean);
    ready.ci = CiState::None;
    let events = observe(&mut state, &read(vec![ready]));
    assert_eq!(kinds(&events), [EventKind::ReadyToMerge]);
    assert_eq!(events[0].sound, Sound::Success);

    let mut state = MonitorState::default();
    observe(&mut state, &read(vec![with_review("a", None)]));
    let events = observe(
        &mut state,
        &read(vec![with_review("a", Some(ReviewDecision::Approved))]),
    );
    assert_eq!(kinds(&events), [EventKind::Approved]);
    assert_eq!(events[0].sound, Sound::Default);
}

#[test]
fn every_event_has_notification_copy() {
    let event = Event {
        kind: EventKind::Approved,
        repo: "acme/app".into(),
        number: 41,
        title: "Add the thing".into(),
        sound: Sound::Default,
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
        EventKind::Merged,
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
            "Ready to merge",
            "Merged"
        ]
    );
}
