use super::*;
use crate::side_notch::host::Lifetime as _;
use crate::side_notch::model::ShowMode;

#[test]
fn shutdown_reaps_an_unresponsive_child_without_the_supervisor() {
    let child = Arc::new(Mutex::new(
        Command::new("/bin/sleep").arg("60").spawn().unwrap(),
    ));
    let lifetime = Lifetime::default();
    lifetime.0.lock().unwrap().child = Some(child.clone());
    lifetime.shutdown();
    let exited = child.lock().unwrap().try_wait().unwrap().is_some();
    if !exited {
        let mut child = child.lock().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
    }
    assert!(
        exited,
        "shutdown returned while the unresponsive helper was alive"
    );
    assert!(
        lifetime.connect().is_err(),
        "quit must prevent a later spawn"
    );
}

const SHUTTING_DOWN: &str = "Native notch is shutting down.";

struct Helper {
    writes: Receiver<Vec<u8>>,
    events: SyncSender<Result<Action, String>>,
}

fn attach(pipe: &mut Pipe, now: Instant) -> Helper {
    let child = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let (writer, writes) = mpsc::sync_channel(1);
    let (events, received) = mpsc::sync_channel(32);
    pipe.connection = Some(Connection {
        child: Arc::new(Mutex::new(child)),
        writer: Some(writer),
        events: received,
    });
    pipe.connected_at = now;
    Helper { writes, events }
}

fn stopped_pipe() -> Pipe {
    let (pipe, lifetime) = Pipe::new();
    lifetime.shutdown();
    pipe
}

fn snapshot() -> NotchSnapshot {
    NotchSnapshot {
        revision: 0,
        supported: true,
        liquid_glass: false,
        settings: crate::side_notch::model::NotchSettings::default(),
        displays: Vec::new(),
        error: None,
    }
}

fn frame() -> RailData {
    RailData {
        settings: snapshot().settings,
        cells: Vec::new(),
        action_error: None,
    }
}

fn received(pipe: &mut Pipe, now: Instant) -> (Result<(), String>, Vec<Inbound>) {
    let mut inbound = Vec::new();
    let result = pipe.receive(now, &mut inbound);
    (result, inbound)
}

fn sequence_of(bytes: &[u8]) -> u64 {
    assert_eq!(bytes.last(), Some(&b'\n'), "one line per frame");
    let message: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    message["sequence"].as_u64().unwrap()
}

fn waits(pipe: &mut Pipe, at: Instant, seconds: u64) {
    let until = at + Duration::from_secs(seconds);
    assert_eq!(
        pipe.connect(until - Duration::from_millis(1)),
        Ok(Link::Closed),
        "still waiting {seconds} s"
    );
    assert_eq!(
        pipe.connect(until),
        Err(SHUTTING_DOWN.into()),
        "tries at {seconds} s"
    );
}

#[test]
fn a_failed_start_waits_one_two_four_seconds_and_so_on_up_to_thirty() {
    let mut pipe = stopped_pipe();
    let mut at = Instant::now();
    assert_eq!(pipe.connect(at), Err(SHUTTING_DOWN.into()));
    for seconds in [1, 2, 4, 8, 16, 30, 30] {
        waits(&mut pipe, at, seconds);
        at += Duration::from_secs(seconds);
    }
}

#[test]
fn new_settings_let_the_next_start_try_at_once() {
    let mut pipe = stopped_pipe();
    let at = Instant::now();
    assert_eq!(pipe.connect(at), Err(SHUTTING_DOWN.into()));
    let soon = at + Duration::from_millis(500);
    assert_eq!(pipe.connect(soon), Ok(Link::Closed));

    pipe.settings_changed();
    assert_eq!(pipe.connect(soon), Err(SHUTTING_DOWN.into()));
    waits(&mut pipe, soon, 2);
}

#[test]
fn turning_the_notch_off_sets_the_wait_back_to_one_second() {
    let mut pipe = stopped_pipe();
    let mut at = Instant::now();
    assert_eq!(pipe.connect(at), Err(SHUTTING_DOWN.into()));
    for seconds in [1, 2] {
        at += Duration::from_secs(seconds);
        assert_eq!(pipe.connect(at), Err(SHUTTING_DOWN.into()));
    }

    pipe.close();
    at += Duration::from_secs(4);
    assert_eq!(pipe.connect(at), Err(SHUTTING_DOWN.into()));
    waits(&mut pipe, at, 1);
}

#[test]
fn only_the_awaited_ack_confirms_the_frame_and_lets_the_next_one_go() {
    let mut pipe = stopped_pipe();
    let at = Instant::now();
    let helper = attach(&mut pipe, at);
    assert_eq!(pipe.connect(at), Ok(Link::Open));
    assert!(pipe.ready());

    pipe.send(at, &snapshot(), frame()).unwrap();
    assert_eq!(sequence_of(&helper.writes.recv().unwrap()), 1);
    assert!(!pipe.ready(), "one frame waits for its ack");

    helper.events.send(Ok(Action::Ready)).unwrap();
    helper.events.send(Ok(Action::Ack { sequence: 7 })).unwrap();
    let (result, inbound) = received(&mut pipe, at + Duration::from_secs(1));
    assert_eq!(result, Ok(()));
    assert!(inbound.is_empty(), "another frame's ack confirms nothing");
    assert!(!pipe.ready());

    helper.events.send(Ok(Action::Ack { sequence: 1 })).unwrap();
    let (result, inbound) = received(&mut pipe, at + Duration::from_secs(2));
    assert_eq!(result, Ok(()));
    assert_eq!(inbound, [Inbound::Confirmed]);
    assert!(pipe.ready());
}

#[test]
fn the_helpers_actions_reach_the_host_in_order() {
    let mut pipe = stopped_pipe();
    let at = Instant::now();
    let helper = attach(&mut pipe, at);
    for action in [
        Action::ScreensChanged,
        Action::Refresh,
        Action::OpenLimits,
        Action::OpenPullRequests,
        Action::SetShow {
            show: ShowMode::OnHover,
        },
    ] {
        helper.events.send(Ok(action)).unwrap();
    }
    let (result, inbound) = received(&mut pipe, at);
    assert_eq!(result, Ok(()));
    assert_eq!(
        inbound,
        [
            Inbound::Action(host::Action::ScreensChanged),
            Inbound::Action(host::Action::Refresh),
            Inbound::Action(host::Action::OpenLimits),
            Inbound::Action(host::Action::OpenPullRequests),
            Inbound::Action(host::Action::SetShow(ShowMode::OnHover)),
        ]
    );
}

#[test]
fn a_frame_unanswered_for_ten_seconds_restarts_the_helper() {
    let mut pipe = stopped_pipe();
    let at = Instant::now();
    let helper = attach(&mut pipe, at);
    pipe.send(at, &snapshot(), frame()).unwrap();
    helper.writes.recv().unwrap();

    let deadline = at + Duration::from_secs(10);
    assert_eq!(received(&mut pipe, deadline).0, Ok(()));
    let late = deadline + Duration::from_millis(1);
    assert_eq!(
        received(&mut pipe, late).0,
        Err("Native notch stopped responding. Retrying…".into())
    );
    assert!(pipe.ready(), "the unanswered frame is forgotten");
    assert_eq!(
        pipe.connect(late),
        Ok(Link::Closed),
        "it waits before a restart"
    );
}

#[test]
fn a_helper_that_breaks_or_goes_away_is_restarted_after_a_wait() {
    let mut pipe = stopped_pipe();
    let at = Instant::now();
    let helper = attach(&mut pipe, at);
    helper
        .events
        .send(Err("Invalid native notch message.".into()))
        .unwrap();
    assert_eq!(
        received(&mut pipe, at).0,
        Err("Invalid native notch message.".into())
    );
    assert_eq!(pipe.connect(at), Ok(Link::Closed));

    let later = at + Duration::from_secs(1);
    let helper = attach(&mut pipe, later);
    drop(helper.events);
    assert_eq!(received(&mut pipe, later).0, Err(STOPPED.into()));
    assert_eq!(
        pipe.connect(later + Duration::from_millis(1_999)),
        Ok(Link::Closed)
    );
}

#[test]
fn a_helper_that_is_not_reading_is_restarted() {
    let mut pipe = stopped_pipe();
    let at = Instant::now();
    let helper = attach(&mut pipe, at);
    pipe.send(at, &snapshot(), frame()).unwrap();
    helper.events.send(Ok(Action::Ack { sequence: 1 })).unwrap();
    assert_eq!(received(&mut pipe, at).1, [Inbound::Confirmed]);

    assert_eq!(
        pipe.send(at, &snapshot(), frame()),
        Err("Native notch is not reading updates.".into())
    );
    assert_eq!(pipe.connect(at), Ok(Link::Closed));
}

#[test]
fn an_ack_thirty_seconds_after_the_start_sets_the_wait_back_to_one_second() {
    for (up, wait) in [(30, 1), (29, 4)] {
        let mut pipe = stopped_pipe();
        let mut at = Instant::now();
        assert_eq!(pipe.connect(at), Err(SHUTTING_DOWN.into()));
        at += Duration::from_secs(1);
        assert_eq!(pipe.connect(at), Err(SHUTTING_DOWN.into()));

        let helper = attach(&mut pipe, at);
        pipe.send(at, &snapshot(), frame()).unwrap();
        at += Duration::from_secs(up);
        helper.events.send(Ok(Action::Ack { sequence: 1 })).unwrap();
        assert_eq!(received(&mut pipe, at).1, [Inbound::Confirmed]);

        drop(helper.events);
        assert!(received(&mut pipe, at).0.is_err());
        waits(&mut pipe, at, wait);
    }
}

#[test]
fn frames_go_out_numbered_one_two_three() {
    let mut pipe = stopped_pipe();
    let at = Instant::now();
    let helper = attach(&mut pipe, at);
    for sequence in 1..=3 {
        pipe.send(at, &snapshot(), frame()).unwrap();
        assert_eq!(sequence_of(&helper.writes.recv().unwrap()), sequence);
        helper.events.send(Ok(Action::Ack { sequence })).unwrap();
        assert_eq!(received(&mut pipe, at).1, [Inbound::Confirmed]);
    }
}

#[test]
fn a_pipe_without_a_helper_neither_sends_nor_receives() {
    let mut pipe = stopped_pipe();
    let at = Instant::now();
    assert_eq!(received(&mut pipe, at).0, Err(STOPPED.into()));
    assert_eq!(pipe.send(at, &snapshot(), frame()), Err(STOPPED.into()));
    assert_eq!(pipe.sequence, 0, "nothing was numbered");
}
