use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, PoisonError},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::{async_runtime, AppHandle};

use crate::dto::{
    AgentId, LimitsStatus, PendingResetSpendDto, ProviderLimitsDto, ResetCreditOutcome,
};
use crate::monitor::{self, wait_for_wake_or_deadline};
use crate::notifications::Sound;
use crate::read_revision::{announce, Source};
use crate::reset_alert::{
    self, AlertMemory, Due, Notice, Plan, ResetSpend, SpendShare, Waiting, WaitingSpend,
};

const DISABLED_WAKE_MINUTES: u16 = 60;
const MONITOR_STATE_SCHEMA_VERSION: u8 = 2;

pub struct LimitsMonitor;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MonitorState {
    schema_version: u8,
    providers: HashMap<AgentId, ProviderObservation>,
    #[serde(default)]
    reset_alerts: AlertMemory,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            schema_version: MONITOR_STATE_SCHEMA_VERSION,
            providers: HashMap::new(),
            reset_alerts: AlertMemory::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProviderObservation {
    account_id: String,
    windows: HashMap<String, WindowObservation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WindowObservation {
    used_percent: f64,
    resets_at: Option<String>,
    observed_at: String,
    exhausted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LimitEventKind {
    Reset,
    Exhausted,
}

#[derive(Debug, PartialEq)]
struct LimitEvent {
    kind: LimitEventKind,
    provider: AgentId,
    account_label: Option<String>,
    window_label: String,
    previous_used_percent: f64,
    used_percent: f64,
}

pub fn setup(app: &mut tauri::App) {
    monitor::spawn::<LimitsMonitor, _, _>(app, run);
}

pub fn wake(app: &AppHandle) {
    monitor::wake::<LimitsMonitor>(app);
}

#[derive(Default)]
struct Queue(Mutex<Waiting>);

static WAITING: LazyLock<Queue> = LazyLock::new(Queue::default);

impl Queue {
    fn read<R>(&self, look: impl FnOnce(&Waiting) -> R) -> R {
        look(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn change<R>(&self, change: impl FnOnce(&mut Waiting) -> R) -> R {
        let mut waiting = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let before = waiting.clone();
        let result = change(&mut waiting);
        let changed = *waiting != before;
        drop(waiting);
        if changed {
            announce(Source::ResetSpends);
        }
        result
    }

    fn spend_by_hand(
        &self,
        account_id: &str,
        spend: impl FnOnce(ResetSpend) -> Result<ResetCreditOutcome, String>,
    ) -> Result<ResetCreditOutcome, String> {
        let outcome = spend(ResetSpend::ByHand)?;
        self.change(|waiting| waiting.used_by_hand(account_id, outcome));
        Ok(outcome)
    }
}

pub fn pending_reset_spends() -> Vec<PendingResetSpendDto> {
    WAITING.read(Waiting::listed)
}

pub fn cancel_reset_spend(account_id: &str) -> bool {
    WAITING.change(|waiting| waiting.cancel(account_id))
}

pub fn spend_reset_by_hand(
    account_id: &str,
    idempotency_key: &str,
) -> Result<ResetCreditOutcome, String> {
    WAITING.spend_by_hand(account_id, |how| {
        crate::limits_refresh::consume_codex_reset_credit(account_id, idempotency_key, how)
    })
}

trait Io {
    async fn read(
        &self,
        providers: &[AgentId],
        fresh: Option<AgentId>,
    ) -> Result<Vec<ProviderLimitsDto>, String>;
    async fn save(&self, state: &MonitorState) -> Result<(), String>;
    async fn spend(
        &self,
        spend: &WaitingSpend,
        share: SpendShare,
    ) -> Result<ResetCreditOutcome, String>;
    fn notify(&self, notice: Notice);
    fn now(&self) -> DateTime<Utc>;
}

struct App {
    app: AppHandle,
    state_path: PathBuf,
}

impl Io for App {
    async fn read(
        &self,
        providers: &[AgentId],
        fresh: Option<AgentId>,
    ) -> Result<Vec<ProviderLimitsDto>, String> {
        let tasks: Vec<_> = providers
            .iter()
            .map(|&provider| {
                let force = fresh == Some(provider);
                async_runtime::spawn_blocking(move || {
                    crate::limits_refresh::read_limits(provider, force)
                })
            })
            .collect();
        let mut snapshots = Vec::new();
        for task in tasks {
            snapshots.extend(
                task.await
                    .map_err(|error| format!("provider worker failed: {error}"))?,
            );
        }
        Ok(snapshots)
    }

    async fn save(&self, state: &MonitorState) -> Result<(), String> {
        monitor::persist_state(&self.state_path, state).await
    }

    async fn spend(
        &self,
        spend: &WaitingSpend,
        share: SpendShare,
    ) -> Result<ResetCreditOutcome, String> {
        let account_id = spend.account_id().to_string();
        let idempotency_key = spend.idempotency_key().to_string();
        async_runtime::spawn_blocking(move || {
            crate::limits_refresh::consume_codex_reset_credit(
                &account_id,
                &idempotency_key,
                ResetSpend::Automatic(share),
            )
        })
        .await
        .unwrap_or_else(|error| Err(format!("The banked reset could not be used: {error}")))
    }

    fn notify(&self, (title, body): Notice) {
        monitor::notify(&self.app, "limits monitor", title, body, Sound::Default);
    }

    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

async fn run(app: AppHandle, mut wake_receiver: async_runtime::Receiver<()>) {
    let state_path = match crate::paths::limits_monitor_state_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!(
                "limits monitor could not resolve its state path: {}",
                error.message
            );
            return;
        }
    };
    let load_path = state_path.clone();
    let mut state = async_runtime::spawn_blocking(move || load_state(&load_path))
        .await
        .unwrap_or_default();
    let io = App { app, state_path };
    let waiting: &Queue = &WAITING;
    loop {
        let settings = async_runtime::spawn_blocking(crate::settings::load_settings)
            .await
            .unwrap_or_default();
        let watched = watched_providers(&settings);
        let polled_at = Utc::now();
        let poll_delay = if !watched.is_empty() {
            if let Err(error) =
                poll_once(&io, waiting, &mut state, &settings, watched, polled_at).await
            {
                eprintln!("limits monitor poll failed: {error}");
            }
            crate::limits_refresh::check_interval(minutes(settings.limits_poll_minutes))
        } else {
            rest(&io, waiting, &mut state).await;
            minutes(DISABLED_WAKE_MINUTES)
        };

        let delay = next_wake(
            poll_delay,
            waiting.read(Waiting::next_due),
            polled_at,
            Utc::now(),
        );
        wait_for_wake_or_deadline(&mut wake_receiver, delay).await;
    }
}

async fn rest(io: &impl Io, waiting: &Queue, state: &mut MonitorState) {
    waiting.change(Waiting::clear);
    if !state.providers.is_empty() || !state.reset_alerts.is_empty() {
        *state = MonitorState::default();
        if let Err(error) = io.save(state).await {
            eprintln!("limits monitor could not clear its state: {error}");
        }
    }
}

fn minutes(minutes: u16) -> Duration {
    Duration::from_secs(u64::from(minutes) * 60)
}

fn next_wake(
    poll_delay: Duration,
    next_due: Option<DateTime<Utc>>,
    polled_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Duration {
    next_due
        .filter(|due| *due > polled_at)
        .map(|due| (due - now).to_std().unwrap_or_default())
        .map_or(poll_delay, |until_due| until_due.min(poll_delay))
        .max(Duration::from_secs(1))
}

fn watched_providers(settings: &crate::settings::AppSettings) -> &'static [AgentId] {
    if settings.limit_notifications {
        &crate::provider_catalogue::WITH_LIMITS
    } else if !settings.reset_alerts.is_empty() {
        &[AgentId::Codex]
    } else {
        &[]
    }
}

async fn poll_once(
    io: &impl Io,
    waiting: &Queue,
    state: &mut MonitorState,
    settings: &crate::settings::AppSettings,
    watched: &[AgentId],
    polled_at: DateTime<Utc>,
) -> Result<(), String> {
    let fresh = waiting
        .read(|waiting| waiting.any_due(polled_at))
        .then_some(AgentId::Codex);
    let readings = io.read(watched, fresh).await?;
    let previous = state.clone();
    let mut notices = limit_notices(state, &readings, settings);
    let Plan { offered, due } = waiting.change(|waiting| {
        reset_alert::plan(
            &mut state.reset_alerts,
            waiting,
            &readings,
            &settings.reset_alerts,
            polled_at,
        )
    });
    let saved = io.save(state).await;
    if saved.is_err() {
        *state = previous;
    } else {
        notices.extend(waiting.change(|waiting| offered.commit(waiting, io.now())));
        for notice in notices {
            io.notify(notice);
        }
    }
    for due in due {
        let notice = match due {
            Due::Spend(spend, share) => spend.outcome_notice(&io.spend(&spend, share).await),
            Due::Kept(notice) => notice,
        };
        io.notify(notice);
    }
    saved.map_err(|error| format!("could not save state: {error}"))
}

fn limit_notices(
    state: &mut MonitorState,
    readings: &[ProviderLimitsDto],
    settings: &crate::settings::AppSettings,
) -> Vec<Notice> {
    if settings.limit_notifications {
        observe(state, readings)
            .iter()
            .map(notification_copy)
            .collect()
    } else {
        state.providers.clear();
        Vec::new()
    }
}

fn notification_copy(event: &LimitEvent) -> Notice {
    let account = event
        .account_label
        .as_deref()
        .map(|label| format!(" · {label}"))
        .unwrap_or_default();
    match event.kind {
        LimitEventKind::Reset => (
            format!("{} limit reset", event.provider.display_name()),
            format!(
                "{} reset from {:.0}% to {:.0}% used{}",
                event.window_label, event.previous_used_percent, event.used_percent, account
            ),
        ),
        LimitEventKind::Exhausted => (
            format!("{} limit reached", event.provider.display_name()),
            format!(
                "{} reached {:.0}% usage{}",
                event.window_label, event.used_percent, account
            ),
        ),
    }
}

fn observe(state: &mut MonitorState, snapshots: &[ProviderLimitsDto]) -> Vec<LimitEvent> {
    let mut events = Vec::new();
    for snapshot in snapshots {
        if !snapshot.current_account || snapshot.status != LimitsStatus::Ok {
            continue;
        }
        let Some(account) = snapshot.account.as_ref() else {
            continue;
        };
        let previous = state
            .providers
            .get(&snapshot.provider)
            .filter(|previous| previous.account_id == account.id);
        let mut windows = HashMap::new();
        for window in &snapshot.reading.windows {
            let before = previous.and_then(|previous| previous.windows.get(&window.id));
            let Some((observation, kind)) = observe_window(
                before,
                window.used_percent,
                window.resets_at.as_deref(),
                &window.observed_at,
            ) else {
                continue;
            };
            windows.insert(window.id.clone(), observation);
            if let (Some(before), Some(kind)) = (before, kind) {
                events.push(LimitEvent {
                    kind,
                    provider: snapshot.provider,
                    account_label: account.label.clone(),
                    window_label: window.label.clone(),
                    previous_used_percent: before.used_percent,
                    used_percent: window.used_percent,
                });
            }
        }
        state.providers.insert(
            snapshot.provider,
            ProviderObservation {
                account_id: account.id.clone(),
                windows,
            },
        );
    }
    events
}

fn observe_window(
    before: Option<&WindowObservation>,
    used_percent: f64,
    resets_at: Option<&str>,
    observed_at: &str,
) -> Option<(WindowObservation, Option<LimitEventKind>)> {
    let incoming_at = chrono::DateTime::parse_from_rfc3339(observed_at).ok()?;
    if let Some(before) = before {
        let is_stale = chrono::DateTime::parse_from_rfc3339(&before.observed_at)
            .is_ok_and(|previous_at| incoming_at <= previous_at);
        if is_stale {
            return Some((before.clone(), None));
        }
    }
    let reset =
        before.is_some_and(|before| reset_detected(before, used_percent, resets_at, incoming_at));
    let was_exhausted =
        before.is_some_and(|before| before.exhausted || before.used_percent >= 100.0);
    let kind = if before.is_none() {
        None
    } else if reset {
        Some(LimitEventKind::Reset)
    } else if !was_exhausted && used_percent >= 100.0 {
        Some(LimitEventKind::Exhausted)
    } else {
        None
    };
    let exhausted = if reset {
        used_percent >= 100.0
    } else {
        was_exhausted || used_percent >= 100.0
    };

    Some((
        WindowObservation {
            used_percent,
            resets_at: resets_at.map(str::to_string),
            observed_at: observed_at.to_string(),
            exhausted,
        },
        kind,
    ))
}

fn reset_detected(
    before: &WindowObservation,
    used_percent: f64,
    resets_at: Option<&str>,
    observed_at: chrono::DateTime<chrono::FixedOffset>,
) -> bool {
    let previous = before
        .resets_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok());
    let timestamp_advanced = previous
        .zip(resets_at.and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok()))
        .is_some_and(|(previous, current)| current > previous);
    let reset_passed = previous.is_some_and(|previous| previous <= observed_at);
    let drop = before.used_percent - used_percent;

    (timestamp_advanced || reset_passed) && drop > 0.5
}

fn load_state(path: &Path) -> MonitorState {
    monitor::load_state(path, |state: &MonitorState| {
        state.schema_version == MONITOR_STATE_SCHEMA_VERSION
    })
}

#[cfg(test)]
mod tests;
