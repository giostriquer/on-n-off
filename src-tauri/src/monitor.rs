//! Plumbing shared by the background monitors (subscription limits, GitHub CI): one wake
//! channel per monitor, a sleep that ends on a wake message or a system-sleep gap, exponential
//! backoff, versioned JSON state files, and notification delivery.

use std::future::Future;
use std::marker::PhantomData;
use std::path::Path;
use std::time::{Duration, SystemTime};
use std::{fs, io};

use serde::{de::DeserializeOwned, Serialize};
use tauri::{async_runtime, AppHandle, Manager};

use crate::usage::cache_io::atomic_write;

const WAKE_HEARTBEAT: Duration = Duration::from_secs(30);
const WAKE_DRIFT_TOLERANCE: Duration = Duration::from_secs(5);

/// The wake channel of one monitor, keyed by a marker type so each monitor is managed and woken
/// on its own.
pub(crate) struct WakeHandle<M> {
    sender: async_runtime::Sender<()>,
    _monitor: PhantomData<fn() -> M>,
}

/// Register the wake handle for monitor `M` and start its loop on the async runtime.
pub(crate) fn spawn<M, F, Fut>(app: &mut tauri::App, run: F)
where
    M: 'static,
    F: FnOnce(AppHandle, async_runtime::Receiver<()>) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (sender, receiver) = async_runtime::channel(1);
    app.manage(WakeHandle::<M> {
        sender,
        _monitor: PhantomData,
    });
    let app_handle = app.handle().clone();
    async_runtime::spawn(run(app_handle, receiver));
}

/// Wake monitor `M`'s loop after a settings change. The capacity-one channel coalesces repeated
/// changes, and a monitor that was never set up is silently skipped.
pub(crate) fn wake<M: 'static>(app: &AppHandle) {
    let Some(handle) = app.try_state::<WakeHandle<M>>() else {
        return;
    };
    let _ = handle.sender.try_send(());
}

/// Sleep until `delay` passes or a wake message arrives. Desktop Tauri does not emit
/// `RunEvent::Resumed`, so the wall clock is compared against a cheap heartbeat instead: a
/// system-sleep gap ends the wait within one heartbeat of wake.
pub(crate) async fn wait_for_wake_or_deadline(
    wake_receiver: &mut async_runtime::Receiver<()>,
    delay: Duration,
) {
    let deadline = SystemTime::now().checked_add(delay);
    loop {
        let now = SystemTime::now();
        let Some(remaining) = deadline.and_then(|deadline| deadline.duration_since(now).ok())
        else {
            return;
        };
        let heartbeat = remaining.min(WAKE_HEARTBEAT);
        let started = SystemTime::now();
        if tokio::time::timeout(heartbeat, wake_receiver.recv())
            .await
            .is_ok()
        {
            return;
        }
        if wake_gap_detected(started, SystemTime::now(), heartbeat) {
            return;
        }
    }
}

fn wake_gap_detected(started: SystemTime, finished: SystemTime, expected: Duration) -> bool {
    finished
        .duration_since(started)
        .map_or(true, |elapsed| elapsed > expected + WAKE_DRIFT_TOLERANCE)
}

/// `base` doubled per consecutive failure (at most sixteenfold), never above `cap`.
pub(crate) fn backoff(base: Duration, consecutive_failures: u32, cap: Duration) -> Duration {
    base.saturating_mul(1 << consecutive_failures.min(4))
        .min(cap)
}

/// A state file's content, or the default when it is absent, unreadable, or not `is_current`
/// (an old schema is discarded rather than migrated).
pub(crate) fn load_state<T: DeserializeOwned + Default>(
    path: &Path,
    is_current: impl Fn(&T) -> bool,
) -> T {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .filter(is_current)
        .unwrap_or_default()
}

pub(crate) fn save_state<T: Serialize>(path: &Path, state: &T) -> Result<(), String> {
    let json = serde_json::to_string(state).map_err(|error| error.to_string())?;
    atomic_write(path, &json).map_err(|error: io::Error| error.to_string())
}

/// `save_state` off the async runtime's threads.
pub(crate) async fn persist_state<T: Serialize + Clone + Send + 'static>(
    path: &Path,
    state: &T,
) -> Result<(), String> {
    let path = path.to_path_buf();
    let state = state.clone();
    async_runtime::spawn_blocking(move || save_state(&path, &state))
        .await
        .map_err(|error| format!("state worker failed: {error}"))?
}

/// Show a notification; a delivery failure is logged with the monitor's name, never raised.
pub(crate) fn notify(app: &AppHandle, monitor: &str, title: String, body: String) {
    if let Err(error) = crate::notifications::show(app, title, body) {
        eprintln!("{monitor} could not show a notification: {}", error.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wall_clock_gap_larger_than_the_heartbeat_detects_system_wake() {
        let started = std::time::UNIX_EPOCH + Duration::from_secs(1_000);

        assert!(!wake_gap_detected(
            started,
            started + Duration::from_secs(30),
            Duration::from_secs(30),
        ));
        assert!(wake_gap_detected(
            started,
            started + Duration::from_secs(40),
            Duration::from_secs(30),
        ));
        assert!(wake_gap_detected(
            started,
            started - Duration::from_secs(1),
            Duration::from_secs(30),
        ));
    }

    #[test]
    fn backoff_doubles_per_failure_and_stops_at_the_cap() {
        let minute = Duration::from_secs(60);
        assert_eq!(backoff(minute, 0, Duration::from_secs(600)), minute);
        assert_eq!(backoff(minute, 1, Duration::from_secs(600)), minute * 2);
        assert_eq!(
            backoff(minute, 4, Duration::from_secs(600)),
            Duration::from_secs(600)
        );
        assert_eq!(
            backoff(minute, 40, Duration::from_secs(600)),
            Duration::from_secs(600)
        );
    }
}
