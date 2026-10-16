use std::cell::{Cell, RefCell};

use super::*;
use crate::settings::AppSettings;

#[derive(Debug, Clone, PartialEq)]
enum Effect {
    Read(Vec<AgentId>, Option<AgentId>),
    Saved,
    Spent(String, SpendShare),
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
    async fn read(
        &self,
        providers: &[AgentId],
        fresh: Option<AgentId>,
    ) -> Result<Vec<ProviderLimitsDto>, String> {
        self.log
            .borrow_mut()
            .push(Effect::Read(providers.to_vec(), fresh));
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
        spend: &WaitingSpend,
        share: SpendShare,
    ) -> Result<ResetCreditOutcome, String> {
        self.log
            .borrow_mut()
            .push(Effect::Spent(spend.account_id().to_string(), share));
        self.keys
            .borrow_mut()
            .push(spend.idempotency_key().to_string());
        self.outcome.borrow().clone()
    }

    fn notify(&self, (title, body): Notice) {
        self.log.borrow_mut().push(Effect::Told(title, body));
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.get()
    }
}

struct Monitor {
    io: Fake,
    waiting: Queue,
    state: MonitorState,
    settings: AppSettings,
}

impl Monitor {
    fn new(settings: AppSettings) -> Self {
        Self {
            io: Fake::new(),
            waiting: Queue::default(),
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
        assert_eq!(
            self.waiting.read(Waiting::next_due),
            Some(at("2026-08-19T13:10:30Z"))
        );
        self.io.effects();
    }

    fn share(&self) -> SpendShare {
        self.settings.reset_alerts["acct-codex"].spend_share()
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
        [Effect::Read(vec![AgentId::Codex], None), Effect::Saved]
    );

    monitor.io.save_fails.set(true);
    let unsaved = monitor.poll(
        "2026-08-19T13:00:00Z",
        vec![codex_with_a_reset(96.0, "2026-08-19T12:10:00Z")],
    );

    assert_eq!(unsaved, Err("could not save state: disk full".to_string()));
    assert_eq!(
        monitor.io.effects(),
        [Effect::Read(vec![AgentId::Codex], None)]
    );
    assert!(listed(&monitor).is_empty());

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
            Effect::Read(vec![AgentId::Codex], None),
            Effect::Saved,
            told(
                "Codex: using a banked reset in 10 minutes",
                "you@example.com has 3% of its limit left. Cancel on the account's card in on-n-off to keep the reset."
            ),
        ]
    );
    assert_eq!(
        listed(&monitor),
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
            Effect::Read(vec![AgentId::Claude, AgentId::Codex, AgentId::Muse], None),
            Effect::Saved,
            told("Codex limit reached", "Weekly · all models reached 100% usage · you@example.com"),
            told(
                "Codex: a banked reset is available",
                "you@example.com has 0% of its limit left and renews in 4d 23h. Open on-n-off to use a banked reset."
            ),
        ]
    );
    assert!(listed(&monitor).is_empty());
}

#[test]
fn a_spend_scheduled_is_told_after_the_limits() {
    let mut monitor = Monitor::new(AppSettings {
        limit_notifications: true,
        ..automatic_only()
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
        monitor.io.effects()[1..],
        [
            Effect::Saved,
            told("Codex limit reached", "Weekly · all models reached 100% usage · you@example.com"),
            told(
                "Codex: using a banked reset in 10 minutes",
                "you@example.com has 0% of its limit left. Cancel on the account's card in on-n-off to keep the reset."
            ),
        ]
    );
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
        Effect::Read(vec![AgentId::Claude, AgentId::Codex, AgentId::Muse], None)
    );
    assert_eq!(
        monitor.io.effects()[0],
        Effect::Read(
            vec![AgentId::Claude, AgentId::Codex, AgentId::Muse],
            Some(AgentId::Codex)
        )
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
            Effect::Read(vec![AgentId::Codex], Some(AgentId::Codex)),
            Effect::Spent("acct-codex".to_string(), monitor.share()),
            told(
                "Codex: banked reset used",
                "you@example.com's usage is back to 0%."
            ),
        ]
    );
    assert!(listed(&monitor).is_empty());
    let keys = monitor.io.keys.borrow();
    assert_eq!(keys.len(), 1);
    assert!(
        uuid::Uuid::parse_str(&keys[0]).is_ok(),
        "spent under {:?}",
        keys[0]
    );
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
            Effect::Read(vec![AgentId::Codex], Some(AgentId::Codex)),
            Effect::Saved,
            told(
                "Codex: banked reset not used",
                "you@example.com no longer needs it, or is no longer signed in, so it was kept."
            ),
        ]
    );
    assert!(listed(&monitor).is_empty());
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
        [Effect::Read(vec![AgentId::Codex], Some(AgentId::Codex))]
    );
    assert_eq!(listed(&monitor).len(), 1);
}

#[test]
fn with_nothing_watched_the_waiting_spends_and_the_state_are_forgotten_once() {
    let mut monitor = Monitor::new(automatic_only());
    monitor.schedule_a_spend();

    for _ in 0..2 {
        tauri::async_runtime::block_on(rest(&monitor.io, &monitor.waiting, &mut monitor.state));
    }

    assert_eq!(monitor.io.effects(), [Effect::Saved]);
    assert!(monitor.state.providers.is_empty() && monitor.state.reset_alerts.is_empty());
    assert!(listed(&monitor).is_empty());
}

fn listed(monitor: &Monitor) -> Vec<crate::dto::PendingResetSpendDto> {
    monitor.waiting.read(Waiting::listed)
}

#[test]
fn an_alert_offers_its_reset_while_limit_notifications_stay_quiet() {
    let mut monitor = Monitor::new(alerts_only());

    monitor
        .poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(95.0, "2026-08-19T12:00:00Z")],
        )
        .unwrap();
    monitor
        .poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(100.0, "2026-08-19T12:10:00Z")],
        )
        .unwrap();

    let titles: Vec<String> = monitor
        .io
        .effects()
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Told(title, _) => Some(title),
            _ => None,
        })
        .collect();
    assert_eq!(titles, ["Codex: a banked reset is available"]);
    assert!(
        monitor.state.providers.is_empty(),
        "limit baselines kept while off"
    );
}

#[test]
fn an_offer_is_not_made_again_after_a_restart() {
    let root = scratch_dir("limits-monitor-offer-restart");
    let path = root.join("monitor.json");
    let mut monitor = Monitor::new(alerts_only());
    monitor
        .poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(95.0, "2026-08-19T12:00:00Z")],
        )
        .unwrap();
    monitor
        .poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(96.0, "2026-08-19T12:10:00Z")],
        )
        .unwrap();
    assert!(monitor
        .io
        .effects()
        .iter()
        .any(|effect| matches!(effect, Effect::Told(..))));
    monitor::save_state(&path, &monitor.state).unwrap();

    let mut restarted = Monitor::new(alerts_only());
    restarted.state = load_state(&path);
    restarted
        .poll(
            "2026-08-19T13:00:00Z",
            vec![codex_with_a_reset(97.0, "2026-08-19T12:20:00Z")],
        )
        .unwrap();

    assert_eq!(
        restarted.io.effects(),
        [Effect::Read(vec![AgentId::Codex], None), Effect::Saved]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn every_change_to_the_waiting_spends_is_told_and_nothing_else_is() {
    use crate::read_revision::{take_announced, Source};
    let mut monitor = Monitor::new(automatic_only());
    let _ = take_announced();

    monitor.schedule_a_spend();
    assert_eq!(take_announced(), [Source::ResetSpends], "scheduled");
    monitor
        .poll(
            "2026-08-19T13:05:00Z",
            vec![codex_with_a_reset(97.0, "2026-08-19T13:05:00Z")],
        )
        .unwrap();
    assert!(take_announced().is_empty(), "announced deciding nothing");

    assert!(monitor
        .waiting
        .change(|waiting| waiting.cancel("acct-codex")));
    assert_eq!(take_announced(), [Source::ResetSpends], "cancelled");
    assert!(!monitor
        .waiting
        .change(|waiting| waiting.cancel("acct-codex")));
    assert!(take_announced().is_empty(), "announced a cancel of nothing");

    monitor.state = MonitorState::default();
    monitor.schedule_a_spend();
    let _ = take_announced();
    monitor
        .poll(
            "2026-08-19T13:11:00Z",
            vec![codex_with_a_reset(98.0, "2026-08-19T13:11:00Z")],
        )
        .unwrap();
    assert_eq!(take_announced(), [Source::ResetSpends], "spent");

    monitor.state = MonitorState::default();
    monitor.schedule_a_spend();
    let _ = take_announced();
    tauri::async_runtime::block_on(rest(&monitor.io, &monitor.waiting, &mut monitor.state));
    assert_eq!(take_announced(), [Source::ResetSpends], "cleared");
    tauri::async_runtime::block_on(rest(&monitor.io, &monitor.waiting, &mut monitor.state));
    assert!(take_announced().is_empty(), "announced clearing nothing");
}

#[test]
fn a_reset_used_by_hand_is_spent_by_hand_and_ends_the_waiting_spend_only_once_it_reset() {
    use crate::read_revision::{take_announced, Source};
    let mut monitor = Monitor::new(automatic_only());
    monitor.schedule_a_spend();
    let _ = take_announced();
    let asked = RefCell::new(Vec::new());
    let by_hand = |outcome: Result<ResetCreditOutcome, String>| {
        let asked = &asked;
        move |how| {
            asked.borrow_mut().push(how);
            outcome
        }
    };
    let waiting = &monitor.waiting;

    assert_eq!(
        waiting.spend_by_hand("acct-codex", by_hand(Err("Codex is not signed in.".into()))),
        Err("Codex is not signed in.".to_string())
    );
    assert_eq!(
        waiting.spend_by_hand(
            "acct-codex",
            by_hand(Ok(ResetCreditOutcome::NothingToReset))
        ),
        Ok(ResetCreditOutcome::NothingToReset)
    );
    assert_eq!(listed(&monitor).len(), 1);
    assert!(
        take_announced().is_empty(),
        "announced a spend that reset nothing"
    );
    assert_eq!(
        waiting.spend_by_hand("other", by_hand(Ok(ResetCreditOutcome::Reset))),
        Ok(ResetCreditOutcome::Reset)
    );
    assert_eq!(
        listed(&monitor).len(),
        1,
        "another account's reset ended this one's"
    );

    assert_eq!(
        waiting.spend_by_hand("acct-codex", by_hand(Ok(ResetCreditOutcome::Reset))),
        Ok(ResetCreditOutcome::Reset)
    );
    assert!(listed(&monitor).is_empty());
    assert_eq!(take_announced(), [Source::ResetSpends]);
    assert_eq!(asked.into_inner(), [ResetSpend::ByHand; 4]);
}

#[test]
fn the_commands_list_and_cancel_the_spends_the_running_monitor_schedules() {
    let waiting: &Queue = &WAITING;
    waiting.change(Waiting::clear);
    let io = Fake::new();
    let mut state = MonitorState::default();
    let settings = automatic_only();
    for (used, observed_at) in [
        (95.0, "2026-08-19T12:00:00Z"),
        (96.0, "2026-08-19T12:10:00Z"),
    ] {
        io.reads(vec![codex_with_a_reset(used, observed_at)]);
        tauri::async_runtime::block_on(poll_once(
            &io,
            waiting,
            &mut state,
            &settings,
            watched_providers(&settings),
            at("2026-08-19T13:00:00Z"),
        ))
        .unwrap();
    }

    assert_eq!(
        pending_reset_spends(),
        [crate::dto::PendingResetSpendDto {
            account_id: "acct-codex".into(),
            due_at: "2026-08-19T13:10:30Z".into(),
        }]
    );
    assert!(cancel_reset_spend("acct-codex"));
    assert!(!cancel_reset_spend("acct-codex"));
    assert!(pending_reset_spends().is_empty());
}
