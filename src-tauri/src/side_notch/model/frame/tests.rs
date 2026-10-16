use super::*;
use crate::dto::{CiState, LimitWindowDto, LimitWindowKind, MergeKind, Reading, ReviewDecision};
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

fn unread(_: AgentId) -> (Option<NotchProvider>, Vec<LiveSession>) {
    (None, Vec::new())
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
        unread,
        Some(&dto),
        None,
    );
    let cell = pr_cell(&frame);
    assert_eq!(cell.status, GithubStatus::Ok);
    assert_eq!(cell.hint.as_deref(), Some("Read a minute ago."));
    assert!(cell.stale);
    assert_eq!(
        cell.lists.iter().map(|list| list.id).collect::<Vec<_>>(),
        [GithubList::Mine, GithubList::Assigned],
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
        unread,
        Some(&dto),
        None,
    );
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
            is_draft: false,
            review_decision: Some(ReviewDecision::Approved),
            ci: CiState::Success,
            merge_kind: Some(MergeKind::Ready),
            updated_at: "2026-09-01T10:00:00Z".into(),
            context: None,
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
    let frame = rail(&settings(vec![GithubList::Mine]), unread, Some(&dto), None);
    let rows = &pr_cell(&frame).lists[0].items;
    assert_eq!(
        rows.iter().map(|row| row.context).collect::<Vec<_>>(),
        [Some(PrContext::Work), Some(PrContext::Personal), None]
    );
    assert!(
        rows.iter().all(|row| row.review_decision.is_none()),
        "no review decision recorded on these rows"
    );
}

#[test]
fn the_pr_cell_is_there_only_while_it_is_on() {
    let dto = prs(vec![pr(1)]);
    let mut off = settings(vec![GithubList::Mine]);
    off.pull_requests.enabled = false;
    let without = rail(&off, unread, Some(&dto), None);
    assert_eq!(without.cells.len(), off.cell_count());
    assert!(without
        .cells
        .iter()
        .all(|cell| matches!(cell, CellData::Provider(_))));
    let on = rail(
        &settings(vec![GithubList::Mine]),
        unread,
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
        |agent| match agent {
            AgentId::Claude => (Some(claude()), vec![session.clone()]),
            _ => (None, Vec::new()),
        },
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
