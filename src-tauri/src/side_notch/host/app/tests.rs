use super::*;
use crate::side_notch::model::NotchSettings;
use std::sync::atomic::Ordering;

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
    let (app, _) = production();
    let saved = crate::side_notch::config::bump_revision();
    assert_ne!(saved, 0);
    assert_eq!(app.settings_revision(), saved);
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
