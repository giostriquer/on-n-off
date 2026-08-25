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
        async_runtime::spawn_blocking(move || crate::limits::read_limits(provider, false))
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
mod tests {
    use super::*;
    use crate::dto::{
        AgentId, LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto,
    };
    use crate::paths::scratch_dir;
    use std::fs;

    fn snapshot(
        provider: AgentId,
        account_id: &str,
        account_label: &str,
        used_percent: f64,
        resets_at: Option<&str>,
    ) -> ProviderLimitsDto {
        ProviderLimitsDto {
            provider,
            status: LimitsStatus::Ok,
            message: None,
            account: Some(LimitsAccountDto {
                id: account_id.into(),
                label: Some(account_label.into()),
            }),
            current_account: true,
            plan: Some("pro".into()),
            windows: vec![LimitWindowDto {
                id: "weekly".into(),
                label: "Weekly · all models".into(),
                kind: LimitWindowKind::Weekly,
                used_percent,
                resets_at: resets_at.map(str::to_string),
                window_seconds: Some(7 * 24 * 60 * 60),
                observed_at: "2026-08-19T12:00:00Z".into(),
            }],
            credits: None,
        }
    }

    fn observed_at(mut snapshot: ProviderLimitsDto, value: &str) -> ProviderLimitsDto {
        snapshot.windows[0].observed_at = value.to_string();
        snapshot
    }

    #[test]
    fn first_successful_read_is_only_a_baseline() {
        let mut state = MonitorState::default();

        let events = observe(
            &mut state,
            &[snapshot(
                AgentId::Claude,
                "account-a",
                "me@example.com",
                82.0,
                Some("2026-08-24T12:00:00Z"),
            )],
        );

        assert!(events.is_empty());
        assert_eq!(state.providers.len(), 1);
    }

    #[test]
    fn a_model_limit_crossing_one_hundred_percent_notifies_once() {
        let mut state = MonitorState::default();
        let mut before = snapshot(
            AgentId::Claude,
            "account-a",
            "me@example.com",
            99.0,
            Some("2026-08-24T12:00:00Z"),
        );
        before.windows[0].id = "weekly_fable".into();
        before.windows[0].label = "Weekly · Fable".into();
        before.windows[0].kind = LimitWindowKind::Model;
        let mut exhausted = before.clone();
        exhausted.windows[0].used_percent = 100.0;
        exhausted.windows[0].observed_at = "2026-08-19T13:00:00Z".into();
        assert!(observe(&mut state, &[before]).is_empty());

        let events = observe(&mut state, std::slice::from_ref(&exhausted));

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, LimitEventKind::Exhausted);
        assert_eq!(events[0].provider, AgentId::Claude);
        assert_eq!(events[0].window_label, "Weekly · Fable");
        assert_eq!(events[0].previous_used_percent, 99.0);
        assert_eq!(events[0].used_percent, 100.0);
        assert!(observe(&mut state, &[exhausted]).is_empty());
    }

    #[test]
    fn a_usage_correction_does_not_rearm_an_exhausted_limit() {
        let mut state = MonitorState::default();
        let below_limit = snapshot(
            AgentId::Claude,
            "account-a",
            "me@example.com",
            99.0,
            Some("2026-08-24T12:00:00Z"),
        );
        let at_limit = observed_at(
            snapshot(
                AgentId::Claude,
                "account-a",
                "me@example.com",
                100.0,
                Some("2026-08-24T12:00:00Z"),
            ),
            "2026-08-19T13:00:00Z",
        );
        assert!(observe(&mut state, std::slice::from_ref(&below_limit)).is_empty());
        assert_eq!(
            observe(&mut state, std::slice::from_ref(&at_limit)).len(),
            1
        );

        assert!(observe(&mut state, &[below_limit]).is_empty());
        assert!(observe(&mut state, &[at_limit]).is_empty());
    }

    #[test]
    fn an_exhausted_limit_notification_is_not_described_as_a_reset() {
        let event = LimitEvent {
            kind: LimitEventKind::Exhausted,
            provider: AgentId::Claude,
            account_label: Some("me@example.com".into()),
            window_label: "Weekly · Fable".into(),
            previous_used_percent: 99.0,
            used_percent: 100.0,
        };

        let (title, body) = notification_copy(&event);

        assert!(title.contains("reached"));
        assert!(!title.contains("reset"));
        assert!(body.contains("Weekly · Fable"));
        assert!(body.contains("100%"));
        assert!(body.contains("me@example.com"));
    }

    #[test]
    fn an_exhausted_first_observation_is_only_a_baseline() {
        let mut state = MonitorState::default();

        let events = observe(
            &mut state,
            &[snapshot(
                AgentId::Codex,
                "account-a",
                "me@example.com",
                100.0,
                Some("2026-08-24T12:00:00Z"),
            )],
        );

        assert!(events.is_empty());
    }

    #[test]
    fn a_reset_rearms_the_exhausted_notification() {
        let mut state = MonitorState::default();
        let at_limit = snapshot(
            AgentId::Codex,
            "account-a",
            "me@example.com",
            100.0,
            Some("2026-08-24T12:00:00Z"),
        );
        let reset = observed_at(
            snapshot(
                AgentId::Codex,
                "account-a",
                "me@example.com",
                0.0,
                Some("2026-08-31T12:00:00Z"),
            ),
            "2026-08-19T13:00:00Z",
        );
        let exhausted_again = observed_at(
            snapshot(
                AgentId::Codex,
                "account-a",
                "me@example.com",
                100.0,
                Some("2026-08-31T12:00:00Z"),
            ),
            "2026-08-19T14:00:00Z",
        );
        assert!(observe(&mut state, std::slice::from_ref(&at_limit)).is_empty());
        assert_eq!(observe(&mut state, &[reset]).len(), 1);

        let events = observe(&mut state, &[exhausted_again]);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].previous_used_percent, 0.0);
        assert_eq!(events[0].used_percent, 100.0);
    }

    #[test]
    fn advancing_the_reset_timestamp_after_usage_notifies_once() {
        let mut state = MonitorState::default();
        let before = snapshot(
            AgentId::Claude,
            "account-a",
            "me@example.com",
            82.0,
            Some("2026-08-24T12:00:00Z"),
        );
        let after = observed_at(
            snapshot(
                AgentId::Claude,
                "account-a",
                "me@example.com",
                0.0,
                Some("2026-08-31T12:00:00Z"),
            ),
            "2026-08-19T13:00:00Z",
        );
        assert!(observe(&mut state, &[before]).is_empty());

        let events = observe(&mut state, std::slice::from_ref(&after));

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, LimitEventKind::Reset);
        assert_eq!(events[0].provider, AgentId::Claude);
        assert_eq!(events[0].account_label.as_deref(), Some("me@example.com"));
        assert_eq!(events[0].window_label, "Weekly · all models");
        assert_eq!(events[0].previous_used_percent, 82.0);
        assert_eq!(events[0].used_percent, 0.0);
        assert!(observe(&mut state, &[after]).is_empty());
    }

    #[test]
    fn a_large_drop_without_a_new_reset_instant_is_not_a_reset() {
        let mut state = MonitorState::default();
        let reset_at = Some("2026-08-24T12:00:00Z");
        assert!(observe(
            &mut state,
            &[snapshot(
                AgentId::Codex,
                "account-a",
                "me@example.com",
                80.0,
                reset_at,
            )],
        )
        .is_empty());

        let events = observe(
            &mut state,
            &[observed_at(
                snapshot(AgentId::Codex, "account-a", "me@example.com", 8.0, reset_at),
                "2026-08-19T13:00:00Z",
            )],
        );

        assert!(events.is_empty());
    }

    #[test]
    fn an_older_observation_never_notifies_or_replaces_the_baseline() {
        let mut state = MonitorState::default();
        let baseline = observed_at(
            snapshot(
                AgentId::Codex,
                "account-a",
                "me@example.com",
                80.0,
                Some("2026-08-24T12:00:00Z"),
            ),
            "2026-08-19T13:00:00Z",
        );
        let stale = observed_at(
            snapshot(
                AgentId::Codex,
                "account-a",
                "me@example.com",
                100.0,
                Some("2026-08-24T12:00:00Z"),
            ),
            "2026-08-19T12:00:00Z",
        );
        assert!(observe(&mut state, &[baseline]).is_empty());

        assert!(observe(&mut state, &[stale]).is_empty());
        assert_eq!(
            state.providers[&AgentId::Codex].windows["weekly"].used_percent,
            80.0
        );
    }

    #[test]
    fn small_utilization_corrections_do_not_notify() {
        let mut state = MonitorState::default();
        let reset_at = Some("2026-08-24T12:00:00Z");
        assert!(observe(
            &mut state,
            &[snapshot(
                AgentId::Codex,
                "account-a",
                "me@example.com",
                50.0,
                reset_at,
            )],
        )
        .is_empty());

        assert!(observe(
            &mut state,
            &[snapshot(
                AgentId::Codex,
                "account-a",
                "me@example.com",
                48.0,
                reset_at,
            )],
        )
        .is_empty());
    }

    #[test]
    fn switching_accounts_establishes_a_new_baseline() {
        let mut state = MonitorState::default();
        assert!(observe(
            &mut state,
            &[snapshot(
                AgentId::Claude,
                "account-a",
                "first@example.com",
                82.0,
                Some("2026-08-24T12:00:00Z"),
            )],
        )
        .is_empty());

        assert!(observe(
            &mut state,
            &[snapshot(
                AgentId::Claude,
                "account-b",
                "second@example.com",
                70.0,
                Some("2026-08-25T12:00:00Z"),
            )],
        )
        .is_empty());

        let events = observe(
            &mut state,
            &[observed_at(
                snapshot(
                    AgentId::Claude,
                    "account-b",
                    "second@example.com",
                    0.0,
                    Some("2026-09-01T12:00:00Z"),
                ),
                "2026-08-19T13:00:00Z",
            )],
        );
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].account_label.as_deref(),
            Some("second@example.com")
        );
    }

    #[test]
    fn failed_and_remembered_snapshots_do_not_replace_the_notification_baseline() {
        let mut state = MonitorState::default();
        let before = snapshot(
            AgentId::Claude,
            "account-a",
            "me@example.com",
            82.0,
            Some("2026-08-24T12:00:00Z"),
        );
        assert!(observe(&mut state, std::slice::from_ref(&before)).is_empty());

        let mut failed = before.clone();
        failed.status = LimitsStatus::Failed;
        failed.windows.clear();
        let mut remembered = before;
        remembered.current_account = false;
        remembered.windows[0].used_percent = 0.0;
        assert!(observe(&mut state, &[failed, remembered]).is_empty());

        let events = observe(
            &mut state,
            &[observed_at(
                snapshot(
                    AgentId::Claude,
                    "account-a",
                    "me@example.com",
                    0.0,
                    Some("2026-08-31T12:00:00Z"),
                ),
                "2026-08-19T13:00:00Z",
            )],
        );
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn failure_backoff_doubles_and_caps_at_sixty_minutes() {
        assert_eq!(poll_delay_minutes(10, 0), 10);
        assert_eq!(poll_delay_minutes(10, 1), 20);
        assert_eq!(poll_delay_minutes(10, 2), 40);
        assert_eq!(poll_delay_minutes(10, 3), 60);
        assert_eq!(poll_delay_minutes(10, 8), 60);
    }

    #[test]
    fn persisted_observations_prevent_duplicate_notifications_after_restart() {
        let root = scratch_dir("limits-monitor-round-trip");
        let path = root.join("monitor.json");
        let mut state = MonitorState::default();
        let before = snapshot(
            AgentId::Claude,
            "account-a",
            "me@example.com",
            82.0,
            Some("2026-08-24T12:00:00Z"),
        );
        let after = observed_at(
            snapshot(
                AgentId::Claude,
                "account-a",
                "me@example.com",
                0.0,
                Some("2026-08-31T12:00:00Z"),
            ),
            "2026-08-19T13:00:00Z",
        );
        assert!(observe(&mut state, &[before]).is_empty());
        assert_eq!(observe(&mut state, std::slice::from_ref(&after)).len(), 1);
        monitor::save_state(&path, &state).unwrap();

        let mut reloaded = load_state(&path);

        assert!(observe(&mut reloaded, &[after]).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_monitor_state_falls_back_to_an_empty_baseline() {
        let root = scratch_dir("limits-monitor-malformed");
        let path = root.join("monitor.json");
        fs::write(&path, "{nope").unwrap();

        let state = load_state(&path);

        assert!(state.providers.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn monitor_state_without_observation_times_is_discarded_instead_of_migrated() {
        let root = scratch_dir("limits-monitor-legacy");
        let path = root.join("monitor.json");
        fs::write(
            &path,
            r#"{"providers":{"claude":{"account_id":"account-a","windows":{"weekly":{"used_percent":99,"resets_at":"2026-08-24T12:00:00Z","exhausted":false}}}}}"#,
        )
        .unwrap();

        let state = load_state(&path);

        assert!(state.providers.is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
