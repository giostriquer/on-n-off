use super::*;

const STOPPED: &str = "Native notch stopped unexpectedly. Retrying…";

#[test]
fn delivery_is_dirty_driven_with_a_slow_heartbeat() {
    let app = app_with(&[(AgentId::Claude, 10.0)]);
    let mut rig = Rig::started(app);
    assert_eq!(rig.frames(), 2, "the opening frame, then the first reads");

    rig.tick(Duration::from_secs(2));
    rig.tick(Duration::from_secs(18));
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
