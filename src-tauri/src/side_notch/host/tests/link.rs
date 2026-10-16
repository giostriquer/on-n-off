use super::*;

const STOPPED: &str = "Native notch stopped unexpectedly. Retrying…";

#[test]
fn delivery_is_dirty_driven_with_a_slow_heartbeat() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::started(app.clone());
    app.take_asked();
    assert_eq!(rig.frames(), 2, "the opening frame, then the first reads");

    rig.tick(Duration::from_secs(2));
    rig.tick(Duration::from_secs(18));
    rig.tick(Duration::from_millis(100));
    assert!(app.take_asked().contains(&Asked::Sessions(AgentId::Claude)));
    assert_eq!(rig.frames(), 2, "sessions read again and unchanged");
    rig.act(Inbound::Action(Action::OpenLimits));
    assert_eq!(rig.frames(), 3, "an action redraws at once");

    rig.tick(Duration::from_millis(29_900));
    assert_eq!(rig.frames(), 3);
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.frames(), 4, "the heartbeat");
    assert!(HEARTBEAT_INTERVAL >= Duration::from_secs(30));
}

#[test]
fn a_sessions_read_that_found_a_new_session_redraws_with_it() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::started(app.clone());
    let sent = rig.frames();
    app.world()
        .sessions
        .insert(AgentId::Claude, vec![session("claude")]);

    rig.tick(Duration::from_secs(10));
    assert_eq!(rig.frames(), sent);
    rig.tick(Duration::from_millis(100));
    assert_eq!(
        rig.frames(),
        sent + 1,
        "the next sessions read changed them"
    );
    let Some(CellData::Provider(claude)) = rig.last_frame().cells.first().cloned() else {
        panic!("a provider cell");
    };
    assert_eq!(claude.sessions, [session("claude")]);
}

#[test]
fn a_pull_request_read_that_changed_nothing_redraws_nothing() {
    let app = FakeApp::default();
    app.world().pull_requests = Some(prs(vec![pr(1)]));
    let mut rig = Rig::started(app.clone());
    app.take_asked();

    rig.tick(Duration::from_secs(60));
    assert!(app.take_asked().contains(&Asked::PullRequests(false)));
    let sent = rig.frames();
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.frames(), sent, "the same pull requests again");
}

#[test]
fn opening_a_screen_reports_its_error_on_the_next_frame() {
    let app = FakeApp::default();
    let mut rig = Rig::started(app.clone());
    app.take_asked();
    app.world().open_error = Some("Cannot open the Limits window.".into());

    rig.act(Inbound::Action(Action::OpenLimits));
    assert_eq!(
        rig.last_frame().action_error.as_deref(),
        Some("Cannot open the Limits window.")
    );
    app.world().open_error = None;
    rig.act(Inbound::Action(Action::OpenPullRequests));
    assert_eq!(rig.last_frame().action_error, None);
    assert_eq!(
        app.take_asked()
            .into_iter()
            .filter(|asked| matches!(asked, Asked::Open(_)))
            .collect::<Vec<_>>(),
        [Asked::Open(Page::Limits), Asked::Open(Page::PullRequests)]
    );
}

#[test]
fn set_show_saves_the_settings_and_draws_them() {
    let app = FakeApp::default();
    let mut rig = Rig::started(app.clone());

    rig.act(Inbound::Action(Action::SetShow(ShowMode::OnHover)));
    assert_eq!(app.world().saved.len(), 1);
    assert_eq!(app.world().saved[0].show, ShowMode::OnHover);
    assert_eq!(rig.last_frame().settings.show, ShowMode::OnHover);
    let status = rig.published().pop().unwrap();
    assert_eq!(
        (status.revision, status.settings.show),
        (1, ShowMode::OnHover)
    );

    app.world().save_error = Some("Cannot save side-notch settings.".into());
    rig.act(Inbound::Action(Action::SetShow(ShowMode::Always)));
    let frame = rig.last_frame();
    assert_eq!(frame.settings.show, ShowMode::OnHover, "nothing was saved");
    assert_eq!(
        frame.action_error.as_deref(),
        Some("Cannot save side-notch settings.")
    );
    app.world().save_error = None;
    rig.act(Inbound::Action(Action::SetShow(ShowMode::Always)));
    assert_eq!(
        rig.last_frame().action_error,
        None,
        "a save that works clears it"
    );
}

#[test]
fn an_action_the_transport_carried_out_reports_its_result() {
    let mut rig = Rig::started(FakeApp::default());
    rig.act(Inbound::Handled(Err(
        "refusing to open http://example.com".into()
    )));
    assert_eq!(
        rig.last_frame().action_error.as_deref(),
        Some("refusing to open http://example.com")
    );
    rig.act(Inbound::Handled(Ok(())));
    assert_eq!(rig.last_frame().action_error, None);
}

#[test]
fn a_broken_link_is_reported_then_opened_again_with_a_fresh_frame() {
    let app = FakeApp::default();
    app.world().open_error = Some("boom".into());
    let mut rig = Rig::started(app.clone());
    rig.act(Inbound::Action(Action::OpenLimits));
    let sent = rig.frames();

    rig.wire.wire().fail_receive = Some(STOPPED.into());
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.host.runtime_error().as_deref(), Some(STOPPED));
    assert_eq!(
        rig.published().pop().unwrap().error.as_deref(),
        Some(STOPPED)
    );
    assert_eq!(rig.frames(), sent);

    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.wire.wire().opened, 2);
    assert_eq!(
        rig.frames(),
        sent + 1,
        "a reopened surface is drawn at once"
    );
    assert_eq!(rig.last_frame().action_error, None);

    rig.wire.wire().fail_send = Some(STOPPED.into());
    rig.act(Inbound::Action(Action::OpenLimits));
    assert_eq!(rig.frames(), sent + 1);
    assert!(!rig.wire.wire().up);
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.wire.wire().opened, 3);
    assert_eq!(rig.frames(), sent + 2);
}

#[test]
fn a_surface_that_cannot_open_says_why_and_nothing_is_read() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::new(app.clone(), snapshot());
    rig.wire.wire().refuse = Some("The native notch is missing from this app.".into());
    rig.tick(Duration::ZERO);
    rig.tick(Duration::from_secs(10 * 60));

    assert_eq!(rig.frames(), 0);
    assert_eq!(
        rig.published().pop().unwrap().error.as_deref(),
        Some("The native notch is missing from this app.")
    );
    assert!(app.take_asked().is_empty());
}

#[test]
fn turning_the_notch_off_closes_the_surface_and_clears_the_error() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::started(app.clone());
    assert!(rig.host.enabled());
    rig.wire.wire().fail_receive = Some(STOPPED.into());
    rig.tick(Duration::from_millis(100));
    rig.tick(Duration::from_millis(100));
    app.take_asked();

    let mut off = snapshot();
    off.settings.enabled = false;
    rig.host.desire(off);
    rig.tick(Duration::from_millis(100));
    rig.tick(Duration::from_secs(10 * 60));
    assert!(!rig.host.enabled());
    assert_eq!(rig.wire.wire().closed, 1);
    assert_eq!(rig.host.runtime_error(), None);
    assert_eq!(rig.published().pop().unwrap().error, None);
    assert!(app.take_asked().is_empty());
}

#[test]
fn settings_from_a_revision_that_has_moved_on_are_ignored() {
    let app = FakeApp::default();
    let mut rig = Rig::started(app.clone());
    app.world().settings_revision = 5;
    let sent = rig.frames();

    let mut stale = snapshot();
    stale.revision = 3;
    stale.settings.show = ShowMode::OnHover;
    rig.host.desire(stale);
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.frames(), sent);

    let mut current = snapshot();
    current.revision = 5;
    current.settings.show = ShowMode::OnHover;
    rig.host.desire(current);
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.frames(), sent + 1);
    assert_eq!(rig.last_frame().settings.show, ShowMode::OnHover);
}

#[test]
fn the_status_is_published_once_per_change_and_never_for_old_settings() {
    let app = FakeApp::default();
    let mut rig = Rig::started(app.clone());
    rig.tick(Duration::from_secs(5));
    assert_eq!(rig.published(), [snapshot()]);

    app.world().settings_revision = 1;
    rig.wire.wire().fail_receive = Some(STOPPED.into());
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.published().len(), 1, "settings saved since then");
}

#[test]
fn a_limits_read_that_changed_nothing_redraws_nothing() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::started(app.clone());
    let sent = rig.frames();

    app.world().limits_revisions.insert(AgentId::Claude, 1);
    rig.tick(Duration::from_millis(100));
    rig.tick(Duration::from_millis(100));
    assert!(app
        .limits_asked()
        .contains(&Asked::Limits(AgentId::Claude, false)));
    assert_eq!(rig.frames(), sent, "the same reading again");

    replace_limits(&app, AgentId::Claude, 20.0);
    rig.tick(Duration::from_millis(100));
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.frames(), sent + 1, "a new reading redraws");
    assert_eq!(providers(&rig.last_frame()), [(AgentId::Claude, 20.0)]);
}

#[test]
fn a_screen_change_rereads_the_settings_and_reads_limits_again() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::started(app.clone());
    app.take_asked();
    let mut moved = snapshot();
    moved.displays[0].x = 1512.0;
    app.world().reread = Some(moved.clone());

    rig.act(Inbound::Action(Action::ScreensChanged));
    assert_eq!(rig.wire.wire().snapshots.last(), Some(&moved));
    assert_eq!(app.limits_asked(), READ_ALL);

    let sent = rig.frames();
    app.world().reread = Some(NotchSnapshot {
        revision: 9,
        ..snapshot()
    });
    rig.tick(Duration::from_millis(100));
    rig.act(Inbound::Action(Action::ScreensChanged));
    assert_eq!(
        rig.frames(),
        sent,
        "settings saved since then are not drawn"
    );
    assert_eq!(
        app.limits_asked(),
        READ_ALL,
        "the limits are read for the new screens all the same"
    );
}

#[test]
fn the_error_stays_until_the_surface_confirms_a_frame() {
    let mut rig = Rig::started(FakeApp::default());
    rig.wire.wire().fail_receive = Some(STOPPED.into());
    rig.tick(Duration::from_millis(100));
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.wire.wire().opened, 2);
    assert_eq!(rig.host.runtime_error().as_deref(), Some(STOPPED));

    rig.act(Inbound::Confirmed);
    assert_eq!(rig.host.runtime_error(), None);
    assert_eq!(rig.published().pop().unwrap().error, None);
}

#[test]
fn while_the_surface_waits_to_retry_nothing_is_read_or_sent() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::new(app.clone(), snapshot());
    rig.wire.wire().waiting = true;
    rig.tick(Duration::ZERO);
    rig.tick(Duration::from_secs(10 * 60));
    assert_eq!(rig.frames(), 0);
    assert!(app.take_asked().is_empty());

    rig.wire.wire().waiting = false;
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.frames(), 1);
}

#[test]
fn a_surface_that_is_not_ready_gets_the_frame_once_it_is_and_reads_go_on() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::started(app.clone());
    app.take_asked();
    let sent = rig.frames();

    rig.wire.wire().unready = true;
    rig.act(Inbound::Action(Action::OpenLimits));
    rig.tick(Duration::from_secs(5 * 60));
    assert_eq!(rig.frames(), sent);
    assert_eq!(app.limits_asked(), READ_ALL);

    rig.wire.wire().unready = false;
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.frames(), sent + 1);
}

#[test]
fn only_settings_that_are_taken_tell_the_surface() {
    let mut rig = Rig::started(FakeApp::default());
    rig.host.desire(NotchSnapshot {
        revision: 4,
        ..snapshot()
    });
    assert_eq!(rig.wire.wire().settings_changed, 0);
    rig.host.desire(snapshot());
    assert_eq!(rig.wire.wire().settings_changed, 1);
}

#[test]
fn what_the_surface_reported_before_its_link_failed_is_still_handled() {
    let app = FakeApp::default();
    let mut rig = Rig::started(app.clone());
    {
        let mut wire = rig.wire.wire();
        wire.inbound
            .push(Inbound::Action(Action::SetShow(ShowMode::OnHover)));
        wire.fail_receive = Some(STOPPED.into());
    }
    rig.tick(Duration::from_millis(100));
    assert_eq!(app.world().saved.len(), 1);
    assert_eq!(rig.host.runtime_error().as_deref(), Some(STOPPED));

    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.last_frame().settings.show, ShowMode::OnHover);
}

#[test]
fn the_status_shows_the_settings_error_before_the_surfaces() {
    let broken = NotchSnapshot {
        error: Some("Cannot read side-notch settings.".into()),
        ..snapshot()
    };
    let mut rig = Rig::new(FakeApp::default(), broken);
    rig.wire.wire().refuse = Some("The native notch is missing from this app.".into());
    rig.tick(Duration::ZERO);

    assert_eq!(
        rig.host.runtime_error().as_deref(),
        Some("The native notch is missing from this app.")
    );
    assert_eq!(
        rig.published().pop().unwrap().error.as_deref(),
        Some("Cannot read side-notch settings.")
    );
}
