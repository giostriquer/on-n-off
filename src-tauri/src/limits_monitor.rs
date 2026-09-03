use std::{collections::HashMap, path::Path, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::{async_runtime, AppHandle};

use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto};
use crate::monitor::{self, wait_for_wake_or_deadline};

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
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            schema_version: MONITOR_STATE_SCHEMA_VERSION,
            providers: HashMap::new(),
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
        let delay_minutes = if settings.limit_notifications {
            let failed = match poll_once(&app, &state_path, &mut state).await {
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
            poll_delay_minutes(settings.limits_poll_minutes, consecutive_failures)
        } else {
            consecutive_failures = 0;
            if !state.providers.is_empty() {
                state.providers.clear();
                if let Err(error) = monitor::persist_state(&state_path, &state).await {
                    eprintln!("limits monitor could not clear its state: {error}");
                }
            }
            DISABLED_WAKE_MINUTES
        };

        wait_for_wake_or_deadline(
            &mut wake_receiver,
            Duration::from_secs(u64::from(delay_minutes) * 60),
        )
        .await;
    }
}

async fn poll_once(
    app: &AppHandle,
    state_path: &Path,
    state: &mut MonitorState,
) -> Result<bool, String> {
    let snapshots = poll_providers().await?;
    let provider_failed = snapshots
        .iter()
        .any(|snapshot| snapshot.current_account && snapshot.status == LimitsStatus::Failed);
    let previous = state.clone();
    let events = observe(state, &snapshots);
    if let Err(error) = monitor::persist_state(state_path, state).await {
        *state = previous;
        return Err(format!("could not save state: {error}"));
    }
    for event in events {
        let (title, body) = notification_copy(&event);
        monitor::notify(app, "limits monitor", title, body);
    }
    Ok(provider_failed)
}

async fn poll_providers() -> Result<Vec<ProviderLimitsDto>, String> {
    let tasks = MONITORED_PROVIDERS.map(|provider| {
        async_runtime::spawn_blocking(move || crate::limits_refresh::read_limits(provider, false))
    });
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
        for window in &snapshot.windows {
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

#[cfg(test)]
mod tests;
