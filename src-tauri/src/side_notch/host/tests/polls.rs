use super::*;

const READ_ALL: [Asked; 3] = [
    Asked::Limits(AgentId::Claude, false),
    Asked::Limits(AgentId::Codex, false),
    Asked::Limits(AgentId::Muse, false),
];

#[test]
fn limits_are_read_again_on_the_apps_interval_and_not_before() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::started(app.clone());
    assert_eq!(
        app.limits_asked(),
        READ_ALL,
        "only the selected providers, in rail order"
    );

    rig.tick(Duration::from_secs(299));
    assert!(app.limits_asked().is_empty(), "inside the interval");
    rig.tick(Duration::from_secs(1));
    assert_eq!(
        app.limits_asked(),
        READ_ALL,
        "automatic reads stay unforced"
    );
    rig.tick(Duration::from_secs(1));
    assert!(app.limits_asked().is_empty());
}

#[test]
fn the_interval_is_whatever_the_app_checks_limits_at() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    app.world().limits_interval = Some(Duration::from_secs(120));
    let mut rig = Rig::started(app.clone());
    app.take_asked();

    rig.tick(Duration::from_secs(119));
    assert!(app.limits_asked().is_empty());
    rig.tick(Duration::from_secs(1));
    assert_eq!(app.limits_asked(), READ_ALL);
}

#[test]
fn refresh_forces_one_read_of_everything_then_reads_go_back_to_unforced() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::started(app.clone());
    app.take_asked();

    rig.act(Inbound::Action(Action::Refresh));
    let asked = app.take_asked();
    for agent in [AgentId::Claude, AgentId::Codex, AgentId::Muse] {
        assert!(asked.contains(&Asked::Limits(agent, true)), "{agent:?}");
        assert!(asked.contains(&Asked::Sessions(agent)), "{agent:?}");
    }
    assert!(asked.contains(&Asked::PullRequests(true)));
    rig.tick(Duration::from_millis(100));
    assert!(app.limits_asked().is_empty(), "forced once, not again");

    rig.tick(Duration::from_secs(5 * 60));
    let asked = app.take_asked();
    assert!(asked.contains(&Asked::Limits(AgentId::Claude, false)));
    assert!(asked.contains(&Asked::PullRequests(false)));
}

#[test]
fn a_read_that_never_reports_back_releases_its_slot_after_a_minute() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    app.world().hold = true;
    let mut rig = Rig::new(app.clone(), snapshot());
    rig.tick(Duration::ZERO);
    let started = app.world().held.len();
    assert_eq!(started, 5, "three providers, the sessions and the PRs");

    rig.tick(Duration::from_secs(60));
    assert_eq!(app.world().held.len(), started, "one read in flight each");
    rig.tick(Duration::from_secs(1));
    assert_eq!(
        app.world().held.len(),
        2 * started,
        "past the deadline every slot is released and read again"
    );

    app.world().held.drain(..started);
    app.release();
    rig.tick(Duration::from_millis(100));
    assert_eq!(providers(&rig.last_frame()), [(AgentId::Claude, 10.0)]);
}

#[test]
fn sessions_are_read_every_ten_seconds_for_the_selected_providers() {
    let app = FakeApp::default();
    let mut rig = Rig::started(app.clone());
    let sessions = |asked: Vec<Asked>| {
        asked
            .into_iter()
            .filter(|asked| matches!(asked, Asked::Sessions(_)))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        sessions(app.take_asked()),
        [
            Asked::Sessions(AgentId::Claude),
            Asked::Sessions(AgentId::Codex),
            Asked::Sessions(AgentId::Muse)
        ]
    );

    rig.tick(Duration::from_millis(9_800));
    assert!(sessions(app.take_asked()).is_empty());
    rig.tick(Duration::from_millis(200));
    assert_eq!(sessions(app.take_asked()).len(), 3);
}

#[test]
fn pull_requests_are_read_on_the_apps_interval_only_while_the_cell_is_on() {
    let app = FakeApp::default();
    app.world().pull_requests_interval = Some(Duration::from_secs(120));
    let mut rig = Rig::started(app.clone());
    let pulls = |asked: Vec<Asked>| {
        asked
            .into_iter()
            .filter(|asked| matches!(asked, Asked::PullRequests(_)))
            .count()
    };
    assert_eq!(pulls(app.take_asked()), 1);

    rig.tick(Duration::from_secs(60));
    assert_eq!(
        pulls(app.take_asked()),
        0,
        "the app's interval, not the first minute"
    );
    rig.tick(Duration::from_secs(60));
    assert_eq!(pulls(app.take_asked()), 1);

    let mut off = snapshot();
    off.settings.pull_requests.enabled = false;
    rig.host.desire(off);
    rig.tick(Duration::from_secs(10 * 60));
    assert_eq!(pulls(app.take_asked()), 0);
}

#[test]
fn nothing_is_read_while_the_rail_has_nowhere_to_go() {
    for broken in [
        NotchSnapshot {
            displays: Vec::new(),
            ..snapshot()
        },
        NotchSnapshot {
            error: Some("Cannot read side-notch settings.".into()),
            ..snapshot()
        },
    ] {
        let app = app_with(&[(AgentId::Claude, 10.0)]);
        let mut rig = Rig::new(app.clone(), broken);
        rig.tick(Duration::ZERO);
        rig.tick(Duration::from_secs(10 * 60));
        assert!(app.take_asked().is_empty());
        assert!(rig.frames() > 0, "the surface still gets the settings");
    }
}

#[test]
fn a_limits_read_another_surface_replaced_reaches_the_notch_without_waiting() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    app.world().limits_revisions.insert(AgentId::Claude, 7);
    let mut rig = Rig::started(app.clone());
    app.take_asked();
    rig.tick(Duration::from_secs(1));
    assert!(
        app.limits_asked().is_empty(),
        "nothing new in the shared cache, so wait out the interval"
    );

    replace_limits(&app, AgentId::Claude, 55.0);
    rig.tick(Duration::from_millis(100));
    assert_eq!(
        app.limits_asked(),
        [Asked::Limits(AgentId::Claude, false)],
        "the Limits screen's refresh replaced the cached read: pick it up now, unforced"
    );
    rig.tick(Duration::from_millis(100));
    assert_eq!(providers(&rig.last_frame()), [(AgentId::Claude, 55.0)]);
    rig.tick(Duration::from_secs(1));
    assert!(
        app.limits_asked().is_empty(),
        "the read it made is at the current revision"
    );
}

#[test]
fn a_newer_revision_waits_for_the_read_in_flight() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::started(app.clone());
    app.take_asked();
    app.world().hold = true;

    replace_limits(&app, AgentId::Claude, 20.0);
    rig.tick(Duration::from_millis(100));
    assert_eq!(app.world().held.len(), 1);
    replace_limits(&app, AgentId::Claude, 30.0);
    rig.tick(Duration::from_millis(100));
    assert_eq!(app.world().held.len(), 1, "one read in flight");

    app.release();
    rig.tick(Duration::from_millis(100));
    rig.tick(Duration::from_millis(100));
    assert_eq!(
        app.world().held.len(),
        0,
        "that read saw the newest revision"
    );
    assert_eq!(providers(&rig.last_frame()), [(AgentId::Claude, 30.0)]);
}

#[test]
fn a_pull_request_read_another_surface_replaced_reaches_the_notch_without_waiting() {
    let app = FakeApp::default();
    let mut rig = Rig::started(app.clone());
    app.take_asked();

    replace_pull_requests(&app, vec![pr(1)]);
    rig.tick(Duration::from_millis(100));
    assert_eq!(
        app.take_asked()
            .into_iter()
            .filter(|asked| matches!(asked, Asked::PullRequests(_)))
            .collect::<Vec<_>>(),
        [Asked::PullRequests(false)],
        "the Pull requests screen's refresh reaches the notch in the next tick, unforced"
    );
    rig.tick(Duration::from_millis(100));
    let frame = rig.last_frame();
    let Some(CellData::PullRequests(cell)) = frame.cells.last() else {
        panic!("a pull request cell");
    };
    assert_eq!(cell.lists[0].items.len(), 1);
    rig.tick(Duration::from_secs(1));
    assert!(app.take_asked().is_empty());
}
