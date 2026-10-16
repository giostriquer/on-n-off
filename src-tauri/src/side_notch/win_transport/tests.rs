use super::*;
use crate::side_notch::host::Lifetime as _;
use crate::side_notch::model::{NotchSettings, ShowMode};

struct Screen {
    messages: mpsc::Receiver<WindowMsg>,
    actions: mpsc::Sender<WinAction>,
}

fn overlay() -> (Overlay, Arc<Lifetime>, Arc<Mutex<Vec<String>>>) {
    let opened = Arc::new(Mutex::new(Vec::new()));
    let seen = opened.clone();
    let (overlay, lifetime) = Overlay::with(
        Box::new(move |url| {
            seen.lock().unwrap().push(url.to_string());
            Ok(())
        }),
        |title, _| Err(format!("Cannot copy {title}.")),
    );
    (overlay, lifetime, opened)
}

fn show(overlay: &mut Overlay) -> Screen {
    let (messages_tx, messages) = mpsc::channel();
    let (actions, actions_rx) = mpsc::channel();
    overlay.lifetime.state().window = Some(messages_tx.clone());
    overlay.window = Some(Window {
        messages: messages_tx,
        actions: actions_rx,
    });
    Screen { messages, actions }
}

fn snapshot() -> NotchSnapshot {
    NotchSnapshot {
        revision: 0,
        supported: true,
        liquid_glass: false,
        settings: NotchSettings::default(),
        displays: Vec::new(),
        error: None,
    }
}

fn frame() -> RailData {
    RailData {
        settings: NotchSettings::default(),
        cells: Vec::new(),
        action_error: None,
    }
}

#[test]
fn the_windows_clicks_reach_the_host_and_its_own_actions_are_answered() {
    let (mut overlay, _lifetime, opened) = overlay();
    let screen = show(&mut overlay);
    for action in [
        WinAction::Refresh,
        WinAction::OpenLimits,
        WinAction::OpenPullRequests,
        WinAction::SetShow(ShowMode::OnHover),
        WinAction::OpenUrl("https://github.com/acme/webapp/pull/1".into()),
        WinAction::OpenUrl("file:///Users/me/notes.txt".into()),
        WinAction::CopyReviewRequest {
            title: "Fix #1".into(),
            url: "https://github.com/acme/webapp/pull/1".into(),
        },
    ] {
        screen.actions.send(action).unwrap();
    }
    let mut inbound = Vec::new();
    assert_eq!(overlay.receive(Instant::now(), &mut inbound), Ok(()));

    assert_eq!(
        inbound,
        [
            Inbound::Action(Action::Refresh),
            Inbound::Action(Action::OpenLimits),
            Inbound::Action(Action::OpenPullRequests),
            Inbound::Action(Action::SetShow(ShowMode::OnHover)),
            Inbound::Handled(Ok(())),
            Inbound::Handled(Err("refusing to open file:///Users/me/notes.txt".into())),
            Inbound::Handled(Err("Cannot copy Fix #1.".into())),
        ]
    );
    assert_eq!(
        *opened.lock().unwrap(),
        ["https://github.com/acme/webapp/pull/1"],
        "only a github.com link reaches the browser"
    );
    assert!(matches!(screen.messages.try_recv(), Ok(WindowMsg::Ping)));
}

#[test]
fn a_window_that_went_away_is_reported_and_forgotten() {
    let (mut overlay, lifetime, _) = overlay();
    drop(show(&mut overlay));
    let mut inbound = Vec::new();
    assert_eq!(
        overlay.receive(Instant::now(), &mut inbound),
        Err(STOPPED.into())
    );
    assert!(overlay.window.is_none());
    assert!(lifetime.state().window.is_none());

    drop(show(&mut overlay));
    assert_eq!(
        overlay.send(Instant::now(), &snapshot(), frame()),
        Err(STOPPED.into())
    );
    assert!(overlay.window.is_none());
    assert_eq!(
        overlay.receive(Instant::now(), &mut inbound),
        Err(STOPPED.into()),
        "nothing to receive from without a window"
    );
}

#[test]
fn a_frame_goes_to_the_window() {
    let (mut overlay, _lifetime, _) = overlay();
    let screen = show(&mut overlay);
    assert!(overlay.ready());
    assert_eq!(overlay.send(Instant::now(), &snapshot(), frame()), Ok(()));
    assert!(matches!(screen.messages.try_recv(), Ok(WindowMsg::Data(data)) if data == frame()));
    assert_eq!(overlay.connect(Instant::now()), Ok(Link::Open));
}

#[test]
fn turning_the_notch_off_shuts_the_window() {
    let (mut overlay, lifetime, _) = overlay();
    let screen = show(&mut overlay);
    overlay.close();
    assert!(matches!(
        screen.messages.try_recv(),
        Ok(WindowMsg::Shutdown)
    ));
    assert!(overlay.window.is_none());
    assert!(lifetime.state().window.is_none());
}

#[test]
fn quitting_shuts_the_window_and_no_other_opens() {
    let (mut overlay, lifetime, _) = overlay();
    let screen = show(&mut overlay);
    lifetime.shutdown();
    assert!(lifetime.stopped());
    assert!(matches!(
        screen.messages.try_recv(),
        Ok(WindowMsg::Shutdown)
    ));

    overlay.window = None;
    assert_eq!(
        overlay.connect(Instant::now()),
        Err("Native notch is shutting down.".into())
    );
    assert!(lifetime.state().window.is_none());
}

#[test]
fn a_frame_the_window_took_is_confirmed_once() {
    let (mut overlay, _lifetime, _) = overlay();
    let screen = show(&mut overlay);
    let mut inbound = Vec::new();
    assert_eq!(overlay.receive(Instant::now(), &mut inbound), Ok(()));
    assert!(inbound.is_empty(), "nothing was sent yet");

    overlay.send(Instant::now(), &snapshot(), frame()).unwrap();
    screen.actions.send(WinAction::Refresh).unwrap();
    assert_eq!(overlay.receive(Instant::now(), &mut inbound), Ok(()));
    assert_eq!(
        inbound,
        [Inbound::Confirmed, Inbound::Action(Action::Refresh)]
    );

    inbound.clear();
    assert_eq!(overlay.receive(Instant::now(), &mut inbound), Ok(()));
    assert!(inbound.is_empty());
}
