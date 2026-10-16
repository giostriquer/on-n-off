use super::*;
use crate::side_notch::host::{Inbound, Link};
use crate::side_notch::model::frame::RailData;
use crate::side_notch::model::{NotchSettings, ShowMode};
use std::sync::atomic::Ordering;

const MISSING: &str = "The native notch is missing from this app.";

static SETTINGS_REVISION: Mutex<()> = Mutex::new(());

#[derive(Clone, Default)]
struct FakeShell(Arc<Mutex<Seen>>);

#[derive(Default)]
struct Seen {
    opened: Vec<Page>,
    emitted: Vec<NotchSnapshot>,
    open_error: Option<String>,
}

impl Shell for FakeShell {
    fn open(&self, page: Page) -> Result<(), String> {
        let mut seen = self.0.lock().unwrap();
        seen.opened.push(page);
        seen.open_error.clone().map_or(Ok(()), Err)
    }

    fn emit(&self, status: &NotchSnapshot) {
        self.0.lock().unwrap().emitted.push(status.clone());
    }
}

fn production() -> (Production<FakeShell>, FakeShell) {
    let shell = FakeShell::default();
    let app = Production {
        shell: shell.clone(),
        limits_poll: Box::leak(Box::new(AtomicU64::new(0))),
        pull_requests_poll: Box::leak(Box::new(AtomicU64::new(0))),
    };
    (app, shell)
}

#[test]
fn the_notch_follows_the_revisions_of_the_shared_reads_themselves() {
    let (app, _) = production();
    for agent in [AgentId::Claude, AgentId::Codex, AgentId::Muse] {
        let before = app.limits_revision(agent);
        let replaced = crate::limits_refresh::bump_revision(agent);
        assert_ne!(replaced, before, "{agent:?}");
        assert_eq!(app.limits_revision(agent), replaced, "{agent:?}");
    }
    assert_eq!(app.limits_revision(AgentId::Cursor), 0);

    let before = app.pull_requests_revision();
    let replaced = crate::github::bump_revision();
    assert_ne!(replaced, before);
    assert_eq!(app.pull_requests_revision(), replaced);
}

#[test]
fn the_notch_checks_limits_at_the_saved_poll_and_at_least_every_five_minutes() {
    let (app, _) = production();
    assert_eq!(
        app.limits_interval(),
        Duration::from_secs(5 * 60),
        "a test build's default"
    );
    app.limits_poll.store(30 * 60, Ordering::Release);
    assert_eq!(app.limits_interval(), Duration::from_secs(5 * 60));
    app.limits_poll.store(2 * 60, Ordering::Release);
    assert_eq!(app.limits_interval(), Duration::from_secs(2 * 60));
}

#[test]
fn the_production_notch_shares_the_intervals_a_settings_save_replaces() {
    let app = Production::new(FakeShell::default());
    assert!(std::ptr::eq(
        app.limits_poll,
        &crate::limits_refresh::POLL_SECONDS
    ));
    assert!(std::ptr::eq(
        app.pull_requests_poll,
        &crate::github::POLL_SECONDS
    ));
}

#[test]
fn the_notch_takes_settings_at_the_revision_they_were_saved_at() {
    let _revision = SETTINGS_REVISION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (app, _) = production();
    assert_eq!(
        app.settings_revision(),
        crate::side_notch::config::revision()
    );
    let saved = crate::side_notch::config::bump_revision();
    assert_eq!(app.settings_revision(), saved);
    let saved_again = crate::side_notch::config::bump_revision();
    assert_eq!(saved_again, saved + 1);
    assert_eq!(app.settings_revision(), saved_again);
}

#[test]
fn the_notch_reads_pull_requests_at_the_saved_interval() {
    let (app, _) = production();
    assert_eq!(
        app.pull_requests_interval(),
        Duration::from_secs(60),
        "a test build's default"
    );
    app.pull_requests_poll.store(120, Ordering::Release);
    assert_eq!(app.pull_requests_interval(), Duration::from_secs(120));
    app.pull_requests_poll.store(30, Ordering::Release);
    assert_eq!(app.pull_requests_interval(), Duration::from_secs(30));
}

#[test]
fn opening_a_screen_and_announcing_the_status_go_through_the_shell() {
    let (app, shell) = production();
    assert_eq!(app.open(Page::Limits), Ok(()));
    shell.0.lock().unwrap().open_error = Some("Cannot open the window.".into());
    assert_eq!(
        app.open(Page::PullRequests),
        Err("Cannot open the window.".into())
    );
    let status = NotchSnapshot {
        revision: 3,
        supported: true,
        liquid_glass: false,
        settings: NotchSettings::default(),
        displays: Vec::new(),
        error: Some("boom".into()),
    };
    app.publish(&status);

    let seen = shell.0.lock().unwrap();
    assert_eq!(seen.opened, [Page::Limits, Page::PullRequests]);
    assert_eq!(seen.emitted, [status]);
}

struct Running;

impl Lifetime for Running {
    fn stopped(&self) -> bool {
        false
    }

    fn shutdown(&self) {}
}

struct Refused;

impl Transport for Refused {
    fn connect(&mut self, _: Instant) -> Result<Link, String> {
        Err(MISSING.into())
    }

    fn receive(&mut self, _: Instant, _: &mut Vec<Inbound>) -> Result<(), String> {
        Err(MISSING.into())
    }

    fn ready(&self) -> bool {
        false
    }

    fn send(&mut self, _: Instant, _: &NotchSnapshot, _: RailData) -> Result<(), String> {
        Err(MISSING.into())
    }

    fn close(&mut self) {}

    fn settings_changed(&mut self) {}
}

type Announced = Arc<Mutex<Vec<(Option<String>, ShowMode)>>>;

#[derive(Clone)]
struct Watching {
    controller: Arc<Controller>,
    seen: Announced,
}

impl Shell for Watching {
    fn open(&self, _: Page) -> Result<(), String> {
        Ok(())
    }

    fn emit(&self, status: &NotchSnapshot) {
        let error = self.controller.error.lock().unwrap().clone();
        self.seen
            .lock()
            .unwrap()
            .push((error, status.settings.show));
    }
}

#[test]
fn a_supervisor_step_records_the_error_before_it_announces_the_status() {
    let _revision = SETTINGS_REVISION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let controller = Arc::new(Controller {
        desired: Mutex::new(None),
        wake: Condvar::new(),
        error: Mutex::new(None),
        lifetime: Arc::new(Running),
    });
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Production {
        shell: Watching {
            controller: controller.clone(),
            seen: seen.clone(),
        },
        limits_poll: Box::leak(Box::new(AtomicU64::new(0))),
        pull_requests_poll: Box::leak(Box::new(AtomicU64::new(0))),
    };
    let snapshot = NotchSnapshot {
        revision: crate::side_notch::config::revision(),
        supported: true,
        liquid_glass: false,
        settings: NotchSettings {
            enabled: true,
            ..NotchSettings::default()
        },
        displays: Vec::new(),
        error: None,
    };
    let mut host = Host::new(app, Refused, snapshot.clone(), Instant::now());
    let mut hover = snapshot.clone();
    hover.settings.show = ShowMode::OnHover;
    *controller.desired.lock().unwrap() = Some(hover);

    let wait = step(&controller, &mut host, Instant::now());
    assert_eq!(wait, Some(Duration::from_millis(100)));
    assert!(controller.desired.lock().unwrap().is_none());
    assert_eq!(
        *seen.lock().unwrap(),
        [(Some(MISSING.to_string()), ShowMode::OnHover)],
        "the status goes out with the desired settings, after its error is recorded"
    );

    let mut off = snapshot;
    off.settings.enabled = false;
    *controller.desired.lock().unwrap() = Some(off);
    assert_eq!(step(&controller, &mut host, Instant::now()), None);
    assert_eq!(*controller.error.lock().unwrap(), None);
}
