use std::cell::{Cell, RefCell};

use super::*;
use crate::settings::AppSettings;

#[derive(Debug, Clone, PartialEq)]
enum Effect {
    Read(Vec<(AgentId, bool)>),
    Saved,
    Spent(String, ResetSpend),
    Told(String, String),
}

struct Fake {
    readings: RefCell<Vec<ProviderLimitsDto>>,
    read_fails: Cell<bool>,
    save_fails: Cell<bool>,
    outcome: RefCell<Result<ResetCreditOutcome, String>>,
    clock: Cell<DateTime<Utc>>,
    log: RefCell<Vec<Effect>>,
    keys: RefCell<Vec<String>>,
}

impl Fake {
    fn new() -> Self {
        Self {
            readings: RefCell::new(Vec::new()),
            read_fails: Cell::new(false),
            save_fails: Cell::new(false),
            outcome: RefCell::new(Ok(ResetCreditOutcome::Reset)),
            clock: Cell::new(at("2026-08-19T13:00:30Z")),
            log: RefCell::new(Vec::new()),
            keys: RefCell::new(Vec::new()),
        }
    }

    fn reads(&self, readings: Vec<ProviderLimitsDto>) {
        *self.readings.borrow_mut() = readings;
    }

    fn effects(&self) -> Vec<Effect> {
        std::mem::take(&mut self.log.borrow_mut())
    }
}

impl Io for Fake {
    async fn read(&self, reads: Vec<(AgentId, bool)>) -> Result<Vec<ProviderLimitsDto>, String> {
        self.log.borrow_mut().push(Effect::Read(reads));
        if self.read_fails.get() {
            return Err("provider worker failed: cancelled".to_string());
        }
        Ok(self.readings.borrow().clone())
    }

    async fn save(&self, _state: &MonitorState) -> Result<(), String> {
        if self.save_fails.get() {
            return Err("disk full".to_string());
        }
        self.log.borrow_mut().push(Effect::Saved);
        Ok(())
    }

    async fn spend(
        &self,
        account_id: String,
        idempotency_key: String,
        how: ResetSpend,
    ) -> Result<ResetCreditOutcome, String> {
        self.log.borrow_mut().push(Effect::Spent(account_id, how));
        self.keys.borrow_mut().push(idempotency_key);
        self.outcome.borrow().clone()
    }

    fn notify(&self, title: String, body: String) {
        self.log.borrow_mut().push(Effect::Told(title, body));
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.get()
    }
}

struct Monitor {
    io: Fake,
    waiting: auto_spend::Queue,
    state: MonitorState,
    settings: AppSettings,
}

impl Monitor {
    fn new(settings: AppSettings) -> Self {
        Self {
            io: Fake::new(),
            waiting: auto_spend::Queue::default(),
            state: MonitorState::default(),
            settings,
        }
    }

    fn poll(&mut self, now: &str, readings: Vec<ProviderLimitsDto>) -> Result<(), String> {
        self.io.reads(readings);
        tauri::async_runtime::block_on(poll_once(
            &self.io,
            &self.waiting,
            &mut self.state,
            &self.settings,
            watched_providers(&self.settings),
            at(now),
        ))
    }

    fn schedule_a_spend(&mut self) {
        self.poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(95.0, "2026-08-19T12:00:00Z")],
        )
        .unwrap();
        self.poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(96.0, "2026-08-19T12:10:00Z")],
        )
        .unwrap();
        assert_eq!(self.waiting.next_due(), Some(at("2026-08-19T13:10:30Z")));
        self.io.effects();
    }

    fn share(&self) -> ResetSpend {
        ResetSpend::Automatic {
            max_left_percent: self.settings.reset_alerts["acct-codex"].spend_limit(),
        }
    }
}

fn told(title: &str, body: &str) -> Effect {
    Effect::Told(title.to_string(), body.to_string())
}

#[test]
fn an_automatic_offer_is_told_and_waits_only_once_the_state_is_saved() {
    let mut monitor = Monitor::new(automatic_only());
    monitor
        .poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(95.0, "2026-08-19T12:00:00Z")],
        )
        .unwrap();
    assert_eq!(
        monitor.io.effects(),
        [Effect::Read(vec![(AgentId::Codex, false)]), Effect::Saved]
    );

    monitor.io.save_fails.set(true);
    let unsaved = monitor.poll(
        "2026-08-19T13:00:00Z",
        vec![codex_with_a_reset(96.0, "2026-08-19T12:10:00Z")],
    );

    assert_eq!(unsaved, Err("could not save state: disk full".to_string()));
    assert_eq!(
        monitor.io.effects(),
        [Effect::Read(vec![(AgentId::Codex, false)])]
    );
    assert!(auto_spend_listed(&monitor).is_empty());

    monitor.io.save_fails.set(false);
    monitor
        .poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(97.0, "2026-08-19T12:20:00Z")],
        )
        .unwrap();

    assert_eq!(
        monitor.io.effects(),
        [
            Effect::Read(vec![(AgentId::Codex, false)]),
            Effect::Saved,
            told(
                "Codex: using a banked reset in 10 minutes",
                "you@example.com has 3% of its limit left. Cancel on the account's card in on-n-off to keep the reset."
            ),
        ]
    );
    assert_eq!(
        auto_spend_listed(&monitor),
        [crate::dto::PendingResetSpendDto {
            account_id: "acct-codex".into(),
            due_at: "2026-08-19T13:10:30Z".into(),
        }],
        "due ten minutes after it was told, not after the poll began"
    );
}

#[test]
fn a_notify_only_offer_is_told_after_the_limits_and_nothing_waits() {
    let mut monitor = Monitor::new(AppSettings {
        limit_notifications: true,
        ..alerts_only()
    });
    monitor
        .poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(95.0, "2026-08-19T12:00:00Z")],
        )
        .unwrap();
    monitor.io.effects();

    monitor
        .poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(100.0, "2026-08-19T12:10:00Z")],
        )
        .unwrap();

    assert_eq!(
        monitor.io.effects(),
        [
            Effect::Read(vec![
                (AgentId::Claude, false),
                (AgentId::Codex, false),
                (AgentId::Muse, false)
            ]),
            Effect::Saved,
            told("Codex limit reached", "Weekly · all models reached 100% usage · you@example.com"),
            told(
                "Codex: a banked reset is available",
                "you@example.com has 0% of its limit left and renews in 4d 23h. Open on-n-off to use a banked reset."
            ),
        ]
    );
    assert!(auto_spend_listed(&monitor).is_empty());
}

#[test]
fn a_spend_falling_due_reads_codex_afresh_and_nothing_else() {
    let mut monitor = Monitor::new(AppSettings {
        limit_notifications: true,
        ..automatic_only()
    });
    monitor.schedule_a_spend();

    monitor
        .poll(
            "2026-08-19T13:10:00Z",
            vec![codex_with_a_reset(97.0, "2026-08-19T13:10:00Z")],
        )
        .unwrap();
    let before_due = monitor.io.effects();
    monitor
        .poll(
            "2026-08-19T13:10:30Z",
            vec![codex_with_a_reset(97.0, "2026-08-19T13:10:30Z")],
        )
        .unwrap();

    assert_eq!(
        before_due[0],
        Effect::Read(vec![
            (AgentId::Claude, false),
            (AgentId::Codex, false),
            (AgentId::Muse, false)
        ])
    );
    assert_eq!(
        monitor.io.effects()[0],
        Effect::Read(vec![
            (AgentId::Claude, false),
            (AgentId::Codex, true),
            (AgentId::Muse, false)
        ])
    );
}

#[test]
fn a_spend_still_needed_is_made_at_its_alerts_share_and_told_even_when_the_save_fails() {
    let mut monitor = Monitor::new(automatic_only());
    monitor.schedule_a_spend();
    monitor.io.save_fails.set(true);

    let unsaved = monitor.poll(
        "2026-08-19T13:11:00Z",
        vec![codex_with_a_reset(98.0, "2026-08-19T13:11:00Z")],
    );

    assert_eq!(unsaved, Err("could not save state: disk full".to_string()));
    assert_eq!(
        monitor.io.effects(),
        [
            Effect::Read(vec![(AgentId::Codex, true)]),
            Effect::Spent("acct-codex".to_string(), monitor.share()),
            told(
                "Codex: banked reset used",
                "you@example.com's usage is back to 0%."
            ),
        ]
    );
    assert!(auto_spend_listed(&monitor).is_empty());
    assert!(!monitor.io.keys.borrow()[0].is_empty());
}

#[test]
fn a_spend_that_fails_is_told_and_not_tried_again() {
    let mut monitor = Monitor::new(automatic_only());
    monitor.schedule_a_spend();
    *monitor.io.outcome.borrow_mut() = Err("Codex is not signed in.".to_string());

    monitor
        .poll(
            "2026-08-19T13:11:00Z",
            vec![codex_with_a_reset(98.0, "2026-08-19T13:11:00Z")],
        )
        .unwrap();
    monitor
        .poll(
            "2026-08-19T13:16:00Z",
            vec![codex_with_a_reset(98.0, "2026-08-19T13:16:00Z")],
        )
        .unwrap();

    let effects = monitor.io.effects();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::Spent(..) | Effect::Told(..)))
            .collect::<Vec<_>>(),
        [
            &Effect::Spent("acct-codex".to_string(), monitor.share()),
            &told("Codex: banked reset not used", "Codex is not signed in."),
        ]
    );
}

#[test]
fn a_spend_no_longer_needed_is_kept_and_told() {
    let mut monitor = Monitor::new(automatic_only());
    monitor.schedule_a_spend();

    monitor
        .poll(
            "2026-08-19T13:11:00Z",
            vec![codex_with_a_reset(50.0, "2026-08-19T13:11:00Z")],
        )
        .unwrap();

    assert_eq!(
        monitor.io.effects(),
        [
            Effect::Read(vec![(AgentId::Codex, true)]),
            Effect::Saved,
            told(
                "Codex: banked reset not used",
                "you@example.com no longer needs it, or is no longer signed in, so it was kept."
            ),
        ]
    );
    assert!(auto_spend_listed(&monitor).is_empty());
}

#[test]
fn a_read_that_fails_decides_nothing_and_the_spend_keeps_waiting() {
    let mut monitor = Monitor::new(automatic_only());
    monitor.schedule_a_spend();
    monitor.io.read_fails.set(true);

    let failed = monitor.poll("2026-08-19T13:11:00Z", Vec::new());

    assert_eq!(failed, Err("provider worker failed: cancelled".to_string()));
    assert_eq!(
        monitor.io.effects(),
        [Effect::Read(vec![(AgentId::Codex, true)])]
    );
    assert_eq!(auto_spend_listed(&monitor).len(), 1);
}

#[test]
fn with_nothing_watched_the_waiting_spends_and_the_state_are_forgotten_once() {
    let mut monitor = Monitor::new(automatic_only());
    monitor.schedule_a_spend();
    let off = AppSettings::default();

    tauri::async_runtime::block_on(rest(
        &monitor.io,
        &monitor.waiting,
        &mut monitor.state,
        &off,
    ));
    tauri::async_runtime::block_on(rest(
        &monitor.io,
        &monitor.waiting,
        &mut monitor.state,
        &off,
    ));

    assert_eq!(monitor.io.effects(), [Effect::Saved]);
    assert!(monitor.state.providers.is_empty() && monitor.state.reset_alerts.is_empty());
    assert!(auto_spend_listed(&monitor).is_empty());
}

fn auto_spend_listed(monitor: &Monitor) -> Vec<crate::dto::PendingResetSpendDto> {
    monitor.waiting.listed()
}
