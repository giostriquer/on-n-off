use super::*;

fn pr_cell(frame: &RailData) -> &PrCellData {
    frame
        .cells
        .iter()
        .find_map(|cell| match cell {
            CellData::PullRequests(cell) => Some(cell),
            CellData::Provider(_) => None,
        })
        .expect("a pull request cell")
}

fn drawn(app: FakeApp, snapshot: NotchSnapshot) -> RailData {
    let mut rig = Rig::new(app, snapshot);
    rig.tick(Duration::ZERO);
    rig.tick(Duration::from_millis(100));
    rig.last_frame()
}

#[test]
fn the_frame_lists_selected_providers_in_rail_order_with_their_sessions_and_prs_last() {
    let app = app_with(&[
        (AgentId::Claude, 10.0),
        (AgentId::Codex, 20.0),
        (AgentId::Muse, 30.0),
        (AgentId::Cursor, 40.0),
    ]);
    app.world()
        .sessions
        .insert(AgentId::Claude, vec![session("claude")]);
    app.world()
        .sessions
        .insert(AgentId::Codex, vec![session("codex")]);
    app.world()
        .sessions
        .insert(AgentId::Muse, vec![session("muse")]);
    let mut selected = snapshot();
    selected.settings.providers = vec![
        AgentId::Cursor,
        AgentId::Claude,
        AgentId::Muse,
        AgentId::Antigravity,
    ];
    selected.settings.pull_requests.enabled = false;
    let mut rig = Rig::new(app.clone(), selected.clone());
    rig.tick(Duration::ZERO);
    rig.tick(Duration::from_millis(100));

    let frame = rig.last_frame();
    assert_eq!(
        providers(&frame),
        [
            (AgentId::Claude, 10.0),
            (AgentId::Muse, 30.0),
            (AgentId::Cursor, 40.0)
        ],
        "Codex is deselected and Antigravity has not been read"
    );
    let sessions: Vec<Vec<LiveSession>> = frame
        .cells
        .iter()
        .filter_map(|cell| match cell {
            CellData::Provider(provider) => Some(provider.sessions.clone()),
            CellData::PullRequests(_) => None,
        })
        .collect();
    assert_eq!(
        sessions,
        [vec![session("claude")], vec![session("muse")], vec![]]
    );
    assert!(
        matches!(frame.cells.last(), Some(CellData::Provider(_))),
        "the PR cell is off"
    );

    selected.settings.pull_requests.enabled = true;
    rig.host.desire(selected);
    rig.tick(Duration::from_millis(100));
    rig.tick(Duration::from_millis(100));
    assert!(
        matches!(
            rig.last_frame().cells.last(),
            Some(CellData::PullRequests(_))
        ),
        "the PR cell closes the rail"
    );
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
    let app = FakeApp::default();
    app.world().pull_requests = Some(dto);

    let mut lists = snapshot();
    lists.settings.pull_requests.lists = vec![GithubList::Assigned, GithubList::Mine];
    let frame = drawn(app.clone(), lists);
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

    let mut review_only = snapshot();
    review_only.settings.pull_requests.lists = vec![GithubList::ReviewRequested];
    let frame = drawn(app, review_only);
    let row = &pr_cell(&frame).lists[0].items[..];
    assert_eq!(
        row,
        [PrRowData {
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
    let app = FakeApp::default();
    app.world().pull_requests = Some(prs(vec![
        row(1, true.into()),
        row(2, false.into()),
        row(3, serde_json::Value::Null),
    ]));
    let frame = drawn(app, snapshot());
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
fn only_the_signed_in_account_reaches_the_frame() {
    let app = FakeApp::default();
    let mut remembered = card(AgentId::Codex, 90.0);
    remembered[0].current_account = false;
    app.world().limits.insert(AgentId::Codex, remembered);
    let mut claude = card(AgentId::Claude, 80.0);
    claude[0].current_account = false;
    claude.extend(card(AgentId::Claude, 15.0));
    app.world().limits.insert(AgentId::Claude, claude);
    let frame = drawn(app, snapshot());
    assert_eq!(
        providers(&frame),
        [(AgentId::Claude, 15.0)],
        "a remembered reading is never the notch's"
    );
}

#[test]
fn a_frame_carries_the_settings_and_the_action_error() {
    let app = FakeApp::default();
    app.world().open_error = Some("boom".into());
    let mut rig = Rig::new(app, snapshot());
    rig.wire
        .wire()
        .inbound
        .push(Inbound::Action(Action::OpenLimits));
    rig.tick(Duration::ZERO);

    let frame = rig.last_frame();
    assert_eq!(frame.settings, snapshot().settings);
    assert_eq!(frame.action_error.as_deref(), Some("boom"));
    assert!(
        frame.cells.is_empty(),
        "nothing has been read yet, so nothing renders"
    );
}
