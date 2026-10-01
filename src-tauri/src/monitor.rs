use std::future::Future;
use std::marker::PhantomData;
use std::path::Path;
use std::time::{Duration, SystemTime};
use std::{fs, io};

use serde::{de::DeserializeOwned, Serialize};
use tauri::{async_runtime, AppHandle, Manager};

use crate::notifications::Sound;
use crate::usage::cache_io::atomic_write;

const WAKE_HEARTBEAT: Duration = Duration::from_secs(30);
const WAKE_DRIFT_TOLERANCE: Duration = Duration::from_secs(5);

pub(crate) struct WakeHandle<M> {
    sender: async_runtime::Sender<()>,
    _monitor: PhantomData<fn() -> M>,
}

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

pub(crate) fn wake<M: 'static>(app: &AppHandle) {
    let Some(handle) = app.try_state::<WakeHandle<M>>() else {
        return;
    };
    let _ = handle.sender.try_send(());
}

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

pub(crate) fn backoff(base: Duration, consecutive_failures: u32, cap: Duration) -> Duration {
    base.saturating_mul(1 << consecutive_failures.min(4))
        .min(cap)
}

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

pub(crate) fn notify(app: &AppHandle, monitor: &str, title: String, body: String, sound: Sound) {
    if let Err(error) = crate::notifications::show(app, title, body, sound) {
        eprintln!("{monitor} could not show a notification: {}", error.message);
    }
}

#[cfg(test)]
mod tests;
