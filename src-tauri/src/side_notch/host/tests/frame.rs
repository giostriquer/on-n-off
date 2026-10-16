use super::*;

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
