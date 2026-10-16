use super::*;
use crate::dto::{CiState, LimitWindowDto, LimitWindowKind, LimitsStatus, Reading};
use crate::side_notch::sessions::SessionStatus;

fn pr(number: u64) -> serde_json::Value {
    serde_json::json!({
        "id": format!("node-{number}"), "number": number, "title": format!("Fix #{number}"),
        "url": format!("https://github.com/acme/webapp/pull/{number}"), "repo": "acme/webapp",
        "author": "octocat", "isDraft": false, "ci": "success", "headRef": "fix",
        "baseRef": "main", "updatedAt": "2026-09-01T10:00:00Z"
    })
}

fn prs(mine: Vec<serde_json::Value>) -> GithubPrsDto {
    serde_json::from_value(serde_json::json!({
        "status": "ok", "stale": false, "scope": [],
        "mine": {"total": mine.len(), "items": mine},
        "reviewRequested": {"total": 0, "items": []},
        "assigned": {"total": 0, "items": []}
    }))
    .unwrap()
}

fn settings(lists: Vec<GithubList>) -> NotchSettings {
    let mut settings = NotchSettings::default();
    settings.pull_requests.lists = lists;
    settings
}

fn unread() -> Vec<(AgentId, Option<NotchProvider>, Vec<LiveSession>)> {
    Vec::new()
}

fn pr_cell(frame: &RailData) -> &PrReading {
    frame
        .cells
        .iter()
        .find_map(|cell| match cell {
            CellData::PullRequests(cell) => cell.reading.as_ref(),
            CellData::Provider(_) => None,
        })
        .expect("a pull request cell that was read")
}

#[test]
fn the_pr_cell_keeps_only_the_selected_lists_capped_and_on_github() {
    let mut mine: Vec<_> = (1..=30).map(pr).collect();
    mine[6]["url"] = "http://example.com/pull/7".into();
    let mut offsite = pr(98);
    offsite["url"] = "http://example.com/pull/98".into();
    let mut review = pr(99);
    review["reviewDecision"] = "APPROVED".into();
    review["mergeKind"] = "ready".into();
    let dto: GithubPrsDto = serde_json::from_value(serde_json::json!({
        "status": "ok", "hint": "Read a minute ago.", "stale": true, "scope": ["org:acme"],
        "mine": {"total": 30, "items": mine},
        "reviewRequested": {"total": 2, "items": [offsite, review]},
        "assigned": {"total": 0, "items": []}
    }))
    .unwrap();

    let frame = rail(
        &settings(vec![GithubList::Assigned, GithubList::Mine]),
        unread(),
        Some(&dto),
        None,
    );
    let cell = pr_cell(&frame);
    assert!(cell.readable);
    assert_eq!(
        cell.lists
            .iter()
            .map(|list| (list.id, list.title))
            .collect::<Vec<_>>(),
        [
            (GithubList::Mine, "Mine"),
            (GithubList::Assigned, "Assigned")
        ],
        "the lists in their own order, review requests not selected"
    );
    assert_eq!(cell.lists[0].total, 30);
    assert_eq!(
        cell.lists[0]
            .items
            .iter()
            .map(|row| row.number)
            .collect::<Vec<_>>(),
        (1..=26).filter(|number| *number != 7).collect::<Vec<_>>(),
        "rows that do not live on github.com never render, and a list stops at 25"
    );
    assert_eq!(MAX_PULL_REQUESTS, 25);
    assert!(cell.lists[1].items.is_empty());

    let frame = rail(
        &settings(vec![GithubList::ReviewRequested]),
        unread(),
        Some(&dto),
        None,
    );
    assert_eq!(pr_cell(&frame).lists[0].title, "Review requested");
    let row = &pr_cell(&frame).lists[0].items[..];
    assert_eq!(
        row,
        [PrRow {
            id: "node-99".into(),
            number: 99,
            title: "Fix #99".into(),
            url: "https://github.com/acme/webapp/pull/99".into(),
            repo: "acme/webapp".into(),
            author: "octocat".into(),
            ci: CiState::Success,
            updated_at: "2026-09-01T10:00:00Z".into(),
            badges: vec![
                Badge {
                    label: "Approved",
                    tone: Tone::Green
                },
                Badge {
                    label: "Ready to merge",
                    tone: Tone::Green
                },
            ],
            tag: None,
        }]
    );
}

#[test]
fn each_row_is_work_or_personal_and_an_unread_owner_neither() {
    let row = |number: u64, in_organization: serde_json::Value| {
        let mut row = pr(number);
        row["inOrganization"] = in_organization;
        row
    };
    let dto = prs(vec![
        row(1, true.into()),
        row(2, false.into()),
        row(3, serde_json::Value::Null),
    ]);
    let frame = rail(
        &settings(vec![GithubList::Mine]),
        unread(),
        Some(&dto),
        None,
    );
    let rows = &pr_cell(&frame).lists[0].items;
    assert_eq!(
        rows.iter().map(|row| row.tag).collect::<Vec<_>>(),
        [
            Some(Badge {
                label: "Work",
                tone: Tone::Work
            }),
            Some(Badge {
                label: "Personal",
                tone: Tone::Personal
            }),
            None
        ],
        "the tag both notches draw, in its context's ink"
    );
    assert!(
        rows.iter().all(|row| row.badges.is_empty()),
        "no review decision or merge state recorded on these rows"
    );
}

#[test]
fn the_pr_cell_is_there_only_while_it_is_on() {
    let dto = prs(vec![pr(1)]);
    let mut off = settings(vec![GithubList::Mine]);
    off.pull_requests.enabled = false;
    let without = rail(&off, unread(), Some(&dto), None);
    assert_eq!(without.cells.len(), off.cell_count());
    assert!(without
        .cells
        .iter()
        .all(|cell| matches!(cell, CellData::Provider(_))));
    let on = rail(
        &settings(vec![GithubList::Mine]),
        unread(),
        Some(&dto),
        Some("boom".into()),
    );
    assert_eq!(on.settings, settings(vec![GithubList::Mine]));
    assert_eq!(on.action_error.as_deref(), Some("boom"));
    assert!(matches!(on.cells.last(), Some(CellData::PullRequests(_))));
}

fn claude() -> NotchProvider {
    NotchProvider::current(vec![crate::limits::signed_in_card(
        AgentId::Claude,
        "acct",
        Reading {
            windows: vec![LimitWindowDto {
                id: "weekly".into(),
                label: "Weekly · all models".into(),
                kind: LimitWindowKind::Weekly,
                used_percent: 41.0,
                resets_at: None,
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
            }],
            ..Reading::default()
        },
    )])
    .expect("a signed-in account")
}

fn cells(frame: &RailData) -> Vec<(Option<AgentId>, bool)> {
    frame
        .cells
        .iter()
        .map(|cell| match cell {
            CellData::Provider(cell) => (Some(cell.provider), cell.reading.is_some()),
            CellData::PullRequests(cell) => (None, cell.reading.is_some()),
        })
        .collect()
}

#[test]
fn every_selected_provider_has_a_cell_in_rail_order_and_one_not_yet_read_is_checking() {
    let session = LiveSession {
        id: "claude-1".into(),
        name: "webapp-1a".into(),
        place: "Terminal".into(),
        project: "webapp".into(),
        status: SessionStatus::Working,
        last_active_at: "2026-09-01T10:00:00Z".into(),
    };
    let mut selected = settings(vec![GithubList::Mine]);
    selected.providers = vec![AgentId::Cursor, AgentId::Claude, AgentId::Muse];
    let frame = rail(
        &selected,
        [(AgentId::Claude, Some(claude()), vec![session.clone()])],
        None,
        None,
    );
    assert_eq!(
        cells(&frame),
        [
            (Some(AgentId::Claude), true),
            (Some(AgentId::Muse), false),
            (Some(AgentId::Cursor), false),
            (None, false),
        ],
        "a cell for every selected provider in rail order, then the pull requests; only Claude was read"
    );
    assert_eq!(frame.cells.len(), selected.cell_count());
    let CellData::Provider(first) = &frame.cells[0] else {
        panic!("Claude leads the rail");
    };
    assert_eq!(first.sessions, [session]);
}

fn read(status: &str, stale: bool, mine: Vec<serde_json::Value>) -> PrReading {
    let dto: GithubPrsDto = serde_json::from_value(serde_json::json!({
        "status": status, "stale": stale, "scope": [],
        "mine": {"total": mine.len(), "items": mine},
        "reviewRequested": {"total": 0, "items": []},
        "assigned": {"total": 0, "items": []}
    }))
    .unwrap();
    let frame = rail(
        &settings(vec![GithubList::Mine]),
        unread(),
        Some(&dto),
        None,
    );
    pr_cell(&frame).clone()
}

#[test]
fn the_ring_has_a_segment_per_row_up_to_24_once_the_read_is_readable() {
    let rows = |count: u64| (1..=count).map(pr).collect::<Vec<_>>();
    let ok = read("ok", false, rows(25));
    assert!(ok.readable);
    assert_eq!(ok.count, 25);
    assert_eq!(ok.segments.len(), MAX_RING_SEGMENTS);
    assert_eq!(MAX_RING_SEGMENTS, 24);

    let stale = read("rateLimited", true, rows(2));
    assert!(stale.readable, "a stale read still shows what it last saw");
    assert_eq!(stale.segments.len(), 2);

    let unreadable = read("ghNotLoggedIn", false, rows(2));
    assert!(!unreadable.readable);
    assert_eq!(unreadable.segments, [], "an unreadable read draws no ring");
}

#[test]
fn the_conflict_band_marks_only_passing_rows_with_merge_conflicts() {
    for ci in ["none", "pending", "success", "failure", "error"] {
        for merge_kind in [
            serde_json::Value::Null,
            "conflicts".into(),
            "ready".into(),
            "behind".into(),
            "blocked".into(),
        ] {
            let mut row = pr(1);
            row["ci"] = ci.into();
            row["mergeKind"] = merge_kind.clone();
            let segments = read("ok", false, vec![row]).segments;
            assert_eq!(
                segments,
                [PrSegment {
                    ci: serde_json::from_value(ci.into()).unwrap(),
                    passing_with_conflicts: ci == "success" && merge_kind == "conflicts",
                }],
                "{ci} with {merge_kind}"
            );
        }
    }
}

#[test]
fn a_pull_request_in_two_lists_is_one_segment_of_the_ring() {
    let mut failing = pr(2);
    failing["ci"] = "failure".into();
    let dto: GithubPrsDto = serde_json::from_value(serde_json::json!({
        "status": "ok", "stale": false, "scope": [],
        "mine": {"total": 2, "items": [pr(1), failing]},
        "reviewRequested": {"total": 0, "items": []},
        "assigned": {"total": 1, "items": [pr(1)]}
    }))
    .unwrap();
    let frame = rail(
        &settings(vec![GithubList::Mine, GithubList::Assigned]),
        unread(),
        Some(&dto),
        None,
    );
    let reading = pr_cell(&frame);
    assert_eq!(reading.lists[1].items.len(), 1, "both lists still show it");
    assert_eq!(reading.count, 2);
    assert_eq!(
        reading.segments,
        [
            PrSegment {
                ci: CiState::Success,
                passing_with_conflicts: false
            },
            PrSegment {
                ci: CiState::Failure,
                passing_with_conflicts: false
            },
        ],
        "one segment per pull request, as the count is"
    );
}

#[test]
fn each_row_carries_its_badges_in_order_with_their_tones() {
    let badges = |draft: bool, review: Option<&str>, merge: Option<&str>| {
        let mut row = pr(1);
        row["isDraft"] = draft.into();
        row["reviewDecision"] = serde_json::to_value(review).unwrap();
        row["mergeKind"] = serde_json::to_value(merge).unwrap();
        read("ok", false, vec![row]).lists[0].items[0]
            .badges
            .iter()
            .map(|badge| (badge.label, badge.tone))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        badges(true, Some("APPROVED"), Some("conflicts")),
        [
            ("Draft", Tone::Muted),
            ("Approved", Tone::Green),
            ("Conflicts", Tone::Red)
        ]
    );
    assert_eq!(
        badges(false, Some("CHANGES_REQUESTED"), Some("queued")),
        [("Changes requested", Tone::Red), ("Queued", Tone::Green)]
    );
    assert_eq!(
        badges(false, Some("REVIEW_REQUIRED"), Some("autoMerge")),
        [("Auto-merge", Tone::Muted)],
        "a review still required is no badge"
    );
    assert_eq!(
        badges(false, None, Some("ready")),
        [("Ready to merge", Tone::Green)]
    );
    assert_eq!(
        badges(false, None, Some("behind")),
        [("Behind base", Tone::Amber)]
    );
    assert_eq!(
        badges(false, None, Some("blocked")),
        [("Blocked", Tone::Amber)]
    );
    assert_eq!(badges(false, None, None), []);
}

fn provider_cell_of(reading: Option<NotchProvider>) -> ProviderCell {
    let mut only = settings(vec![GithubList::Mine]);
    only.providers = vec![AgentId::Claude];
    only.pull_requests.enabled = false;
    match rail(&only, [(AgentId::Claude, reading, Vec::new())], None, None)
        .cells
        .pop()
    {
        Some(CellData::Provider(cell)) => cell,
        other => panic!("a provider cell, not {other:?}"),
    }
}

fn reported(status: LimitsStatus, message: Option<&str>, reading: Reading) -> NotchProvider {
    let mut card = crate::limits::signed_in_card(AgentId::Claude, "acct", reading);
    card.status = status;
    card.message = message.map(str::to_owned);
    NotchProvider::current(vec![card]).expect("a signed-in account")
}

type Said<'a> = Option<(&'a str, Tone)>;

fn said(notice: &Option<Notice>) -> Said<'_> {
    notice
        .as_ref()
        .map(|notice| (notice.text.as_str(), notice.tone))
}

fn notices(cell: &ProviderCell) -> (Said<'_>, Said<'_>) {
    (said(&cell.notice), said(&cell.paused))
}

#[test]
fn a_provider_says_it_is_checking_until_read_and_when_its_refresh_paused() {
    let windows = claude().windows;
    let paused = Some(("Refresh paused. Last observed values below.", Tone::Amber));
    assert_eq!(
        notices(&provider_cell_of(None)),
        (Some(("Checking usage…", Tone::Muted)), None)
    );
    assert_eq!(
        notices(&provider_cell_of(Some(claude()))),
        (None, None),
        "a reading that is fine says nothing above its windows"
    );
    let failed = provider_cell_of(Some(reported(
        LimitsStatus::Failed,
        Some("Claude Code reported no usage."),
        Reading {
            windows: windows.clone(),
            ..Reading::default()
        },
    )));
    assert_eq!(
        notices(&failed),
        (
            Some(("Claude Code reported no usage.", Tone::Muted)),
            paused
        )
    );
    assert_eq!(
        notices(&provider_cell_of(Some(reported(
            LimitsStatus::SignedOut,
            None,
            Reading::default()
        )))),
        (Some(("Usage unavailable.", Tone::Muted)), None),
        "nothing observed, so nothing is last observed"
    );
    let share_only = reported(
        LimitsStatus::Failed,
        Some("Refresh failed."),
        Reading {
            workspace_credits: Some(crate::dto::LimitsWorkspaceCreditsDto {
                limit: "25000".into(),
                used: "8000".into(),
                used_percent: 32.0,
                resets_at: None,
                reached: false,
            }),
            ..Reading::default()
        },
    );
    assert_eq!(notices(&provider_cell_of(Some(share_only))).1, paused);
}

fn pr_cell_of(dto: Option<&GithubPrsDto>) -> PrCell {
    let mut only = settings(vec![GithubList::Mine]);
    only.providers = Vec::new();
    match rail(&only, unread(), dto, None).cells.pop() {
        Some(CellData::PullRequests(cell)) => cell,
        other => panic!("a pull request cell, not {other:?}"),
    }
}

fn reported_pulls(status: &str, hint: Option<&str>, stale: bool) -> GithubPrsDto {
    serde_json::from_value(serde_json::json!({
        "status": status, "hint": hint, "stale": stale, "scope": [],
        "mine": {"total": 1, "items": [pr(1)]},
        "reviewRequested": {"total": 0, "items": []},
        "assigned": {"total": 0, "items": []}
    }))
    .unwrap()
}

#[test]
fn the_pull_request_cell_says_it_is_checking_until_read_then_shows_a_failed_reads_hint() {
    let checking = pr_cell_of(None);
    assert_eq!(checking.reading, None);
    assert_eq!(checking.label, "—", "no count before the first read");
    assert_eq!(
        checking.notice,
        Some(Notice {
            text: "Checking pull requests…".into(),
            tone: Tone::Muted
        })
    );

    let stale = pr_cell_of(Some(&reported_pulls(
        "rateLimited",
        Some("Showing the pull requests read at 10:00."),
        true,
    )));
    assert_eq!(
        stale.notice,
        Some(Notice {
            text: "Showing the pull requests read at 10:00.".into(),
            tone: Tone::Amber
        }),
        "a stale read warns over the rows it still shows"
    );
    assert_eq!(
        stale.label, "1",
        "the ring's figure is the count of what it still shows"
    );
    assert_eq!(stale.reading.expect("read").lists[0].items.len(), 1);

    let unreadable = pr_cell_of(Some(&reported_pulls(
        "ghNotLoggedIn",
        Some("Sign in with the GitHub CLI."),
        false,
    )));
    assert_eq!(
        unreadable.notice,
        Some(Notice {
            text: "Sign in with the GitHub CLI.".into(),
            tone: Tone::Muted
        })
    );
    assert_eq!(unreadable.label, "—");
    let reading = unreadable.reading.expect("read");
    assert!(!reading.readable);
    assert!(reading.lists.is_empty(), "an unreadable read shows no rows");
    assert_eq!(reading.count, 0);

    assert_eq!(
        pr_cell_of(Some(&reported_pulls(
            "ok",
            Some("Read a minute ago."),
            false
        )))
        .notice,
        None,
        "a read that is fine keeps its hint to itself"
    );
}

#[test]
fn a_provider_that_was_read_is_not_checking_even_without_windows() {
    let empty = reported(LimitsStatus::Ok, None, Reading::default());
    assert_eq!(
        notices(&provider_cell_of(Some(empty))),
        (None, None),
        "the read answered; it just reported no windows"
    );
}

#[test]
fn what_needs_attention_is_counted_once_per_pull_request() {
    let mut conflicted = pr(1);
    conflicted["mergeKind"] = "conflicts".into();
    let mut failing = pr(2);
    failing["ci"] = "error".into();
    failing["reviewDecision"] = "CHANGES_REQUESTED".into();
    let mut ready = pr(3);
    ready["mergeKind"] = "ready".into();
    ready["ci"] = "failure".into();
    let dto: GithubPrsDto = serde_json::from_value(serde_json::json!({
        "status": "ok", "stale": false, "scope": [],
        "mine": {"total": 3, "items": [conflicted.clone(), failing, ready]},
        "reviewRequested": {"total": 1, "items": [conflicted]},
        "assigned": {"total": 0, "items": []}
    }))
    .unwrap();
    let frame = rail(
        &settings(vec![GithubList::Mine, GithubList::ReviewRequested]),
        unread(),
        Some(&dto),
        None,
    );
    assert_eq!(
        pr_cell(&frame).attention,
        PrAttention {
            conflicts: 1,
            failing: 2,
            changes_requested: 1,
            ready: 1,
        }
    );
}
