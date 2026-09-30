use std::{collections::HashMap, path::Path, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::{async_runtime, AppHandle};

use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto};
use crate::monitor::{self, wait_for_wake_or_deadline};
use crate::notifications::Sound;

const DISABLED_WAKE_MINUTES: u16 = 60;
const MONITOR_STATE_SCHEMA_VERSION: u8 = 2;
const MONITORED_PROVIDERS: [AgentId; 2] = [AgentId::Claude, AgentId::Codex];
const MAX_BACKOFF_MINUTES: u16 = 60;

/// Marker for this monitor's wake channel.
pub struct LimitsMonitor;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MonitorState {
    schema_version: u8,
    providers: HashMap<AgentId, ProviderObservation>,
    /// Banked reset alerts, by the card's account id (`reset_alerts`).
    #[serde(default)]
    reset_alerts: HashMap<String, reset_alerts::AlertState>,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            schema_version: MONITOR_STATE_SCHEMA_VERSION,
            providers: HashMap::new(),
            reset_alerts: HashMap::new(),
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

/// Wake the poll loop after a settings change.
pub fn wake(app: &AppHandle) {
    monitor::wake::<LimitsMonitor>(app);
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
    let mut consecutive_failures = 0_u32;

    loop {
        let settings = async_runtime::spawn_blocking(crate::settings::load_settings)
            .await
            .unwrap_or_default();
        let watched = watched_providers(&settings);
        let polled_at = chrono::Utc::now();
        let poll_delay = if !watched.is_empty() {
            let failed =
                match poll_once(&app, &state_path, &mut state, &settings, watched, polled_at).await
                {
                    Ok(failed) => failed,
                    Err(error) => {
                        eprintln!("limits monitor poll failed: {error}");
                        true
                    }
                };
            if failed {
                consecutive_failures = consecutive_failures.saturating_add(1);
            } else {
                consecutive_failures = 0;
            }
            minutes(poll_delay_minutes(
                settings.limits_poll_minutes,
                consecutive_failures,
            ))
        } else {
            consecutive_failures = 0;
            // With nothing watched no alert is left, so nothing waits to be spent either.
            auto_spend::clear();
            if !state.providers.is_empty() || !state.reset_alerts.is_empty() {
                // A poll of nothing forgets every observation.
                observe_poll(&mut state, &[], &settings, chrono::Utc::now());
                if let Err(error) = monitor::persist_state(&state_path, &state).await {
                    eprintln!("limits monitor could not clear its state: {error}");
                }
            }
            minutes(DISABLED_WAKE_MINUTES)
        };

        let delay = next_wake(
            poll_delay,
            auto_spend::next_due(),
            polled_at,
            chrono::Utc::now(),
        );
        wait_for_wake_or_deadline(&mut wake_receiver, delay).await;
    }
}

fn minutes(minutes: u16) -> Duration {
    Duration::from_secs(u64::from(minutes) * 60)
}

/// How long the monitor sleeps: until its next poll, or until the soonest reset waiting to be
/// spent falls due, whichever comes first, and never less than a second. Only a spend falling due
/// after the poll made at `polled_at` wakes it early: one that poll saw and could not decide waits
/// for the next poll, which after a failure is the backoff, rather than being asked every second.
fn next_wake(
    poll_delay: Duration,
    next_due: Option<chrono::DateTime<chrono::Utc>>,
    polled_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> Duration {
    next_due
        .filter(|due| *due > polled_at)
        .map(|due| (due - now).to_std().unwrap_or_default())
        .map_or(poll_delay, |until_due| until_due.min(poll_delay))
        .max(Duration::from_secs(1))
}

/// The providers the monitor reads under `settings`: both while limit notifications are on, Codex
/// alone while only banked reset alerts are, and none otherwise.
fn watched_providers(settings: &crate::settings::AppSettings) -> &'static [AgentId] {
    if settings.limit_notifications {
        &MONITORED_PROVIDERS
    } else if !settings.reset_alerts.is_empty() {
        &[AgentId::Codex]
    } else {
        &[]
    }
}

/// One poll of `watched`: limit notifications when they are on, a banked reset offered to every
/// opted-in account that needs one, and every automatic spend whose time has come.
///
/// A spend whose time has come reads Codex afresh, so it is decided by what Codex says now. It is
/// taken out of the waiting ones before anything else, which is what stops it being handed out
/// twice, and is decided whether or not the state saves. An offer is saved before it is shown or
/// scheduled, so one that failed to save is not shown again and again, nor spent.
async fn poll_once(
    app: &AppHandle,
    state_path: &Path,
    state: &mut MonitorState,
    settings: &crate::settings::AppSettings,
    watched: &[AgentId],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<bool, String> {
    let snapshots = poll_providers(watched, auto_spend::any_due(now)).await?;
    let provider_failed = snapshots
        .iter()
        .any(|snapshot| snapshot.current_account && snapshot.status == LimitsStatus::Failed);
    let due = auto_spend::take_due(&snapshots, &settings.reset_alerts, now);
    let previous = state.clone();
    let outcome = observe_poll(state, &snapshots, settings, now);
    let saved = monitor::persist_state(state_path, state).await;
    if saved.is_err() {
        *state = previous;
    } else {
        // Counted from when the user is told, not from before the read, so the wait is the whole
        // ten minutes.
        let scheduled = auto_spend::schedule_all(&outcome.automatic_offers, chrono::Utc::now());
        for (title, body) in outcome.notices.into_iter().chain(scheduled) {
            monitor::notify(app, "limits monitor", title, body, Sound::Default);
        }
    }
    for due in due {
        let (title, body) = match due {
            auto_spend::Due::Spend(spend) => {
                let (account_id, key) = (spend.account_id.clone(), spend.idempotency_key.clone());
                let spent = async_runtime::spawn_blocking(move || {
                    crate::limits_refresh::consume_codex_reset_credit(&account_id, &key)
                })
                .await
                .unwrap_or_else(|error| {
                    Err(format!("The banked reset could not be used: {error}"))
                });
                auto_spend::outcome_copy(&spend, &spent)
            }
            auto_spend::Due::Kept(spend, why) => auto_spend::kept_copy(&spend, why),
        };
        monitor::notify(app, "limits monitor", title, body, Sound::Default);
    }
    saved.map_err(|error| format!("could not save state: {error}"))?;
    Ok(provider_failed)
}

/// What one poll's readings call for.
struct PollOutcome {
    notices: Vec<(String, String)>,
    /// Offers whose alerts use the reset by themselves, to be scheduled once they are saved.
    automatic_offers: Vec<reset_alerts::Offer>,
}

/// What one poll's `snapshots` call for under `settings` at `now`, with `state` brought up to
/// date: limit changes only while limit notifications are on, whose baselines are forgotten while
/// they are off, and every banked reset an opted-in account is offered, which an automatic alert
/// schedules instead of only saying so.
fn observe_poll(
    state: &mut MonitorState,
    snapshots: &[ProviderLimitsDto],
    settings: &crate::settings::AppSettings,
    now: chrono::DateTime<chrono::Utc>,
) -> PollOutcome {
    let events = if settings.limit_notifications {
        observe(state, snapshots)
    } else {
        state.providers.clear();
        Vec::new()
    };
    let mut notices: Vec<(String, String)> = events.iter().map(notification_copy).collect();
    let (automatic_offers, offers): (Vec<_>, Vec<_>) = reset_alerts::observe(
        &mut state.reset_alerts,
        snapshots,
        &settings.reset_alerts,
        now,
    )
    .into_iter()
    .partition(|offer| offer.automatic);
    notices.extend(
        offers
            .iter()
            .map(|offer| reset_alerts::notification_copy(offer, now)),
    );
    PollOutcome {
        notices,
        automatic_offers,
    }
}

/// Reads `watched`, Codex afresh when `fresh_codex`: a spend whose time has come is decided by what
/// Codex says now, not by a cached read, which can be the one that made the offer.
async fn poll_providers(
    watched: &[AgentId],
    fresh_codex: bool,
) -> Result<Vec<ProviderLimitsDto>, String> {
    let tasks: Vec<_> = watched
        .iter()
        .map(|&provider| {
            let force = fresh_codex && provider == AgentId::Codex;
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

fn notification_copy(event: &LimitEvent) -> (String, String) {
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
    let reset = before.is_some_and(|before| reset_detected(before, used_percent, resets_at));
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

fn reset_detected(before: &WindowObservation, used_percent: f64, resets_at: Option<&str>) -> bool {
    // A lower percentage alone can be a correction. Only a newer provider reset instant proves a
    // new cycle and can rearm an exhausted notification.
    let timestamp_advanced = before
        .resets_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .zip(resets_at.and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok()))
        .is_some_and(|(previous, current)| current > previous);
    let drop = before.used_percent - used_percent;

    timestamp_advanced && drop > 0.5
}

fn poll_delay_minutes(base_minutes: u16, consecutive_failures: u32) -> u16 {
    let delay = monitor::backoff(
        Duration::from_secs(u64::from(base_minutes) * 60),
        consecutive_failures,
        Duration::from_secs(u64::from(MAX_BACKOFF_MINUTES) * 60),
    );
    u16::try_from(delay.as_secs() / 60).unwrap_or(MAX_BACKOFF_MINUTES)
}

fn load_state(path: &Path) -> MonitorState {
    monitor::load_state(path, |state: &MonitorState| {
        state.schema_version == MONITOR_STATE_SCHEMA_VERSION
    })
}

pub mod auto_spend;
mod reset_alerts;

#[cfg(test)]
mod tests;
