use super::{
    model::{layout, NotchSnapshot},
    protocol::Action,
    transport::{Connection, Lifetime},
};
use crate::dto::{AgentId, LimitWindowDto, LimitsStatus, ProviderLimitsDto};
use serde::Serialize;
use std::{
    sync::{mpsc, Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
struct Controller {
    desired: Mutex<Option<NotchSnapshot>>,
    wake: Condvar,
    lifetime: Lifetime,
    error: Mutex<Option<String>>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Changed {
    snapshot: NotchSnapshot,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeProvider {
    provider: AgentId,
    status: LimitsStatus,
    current_account: bool,
    plan: Option<String>,
    message: Option<String>,
    windows: Vec<LimitWindowDto>,
}
fn current_provider(entries: Vec<ProviderLimitsDto>) -> Option<NativeProvider> {
    entries
        .into_iter()
        .find(|entry| entry.current_account)
        .map(|entry| NativeProvider {
            provider: entry.provider,
            status: entry.status,
            current_account: true,
            plan: entry.plan,
            message: entry.message,
            windows: entry.windows,
        })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Message<'a> {
    version: u8,
    sequence: u64,
    snapshot: &'a NotchSnapshot,
    providers: Vec<&'a NativeProvider>,
    action_error: &'a Option<String>,
}

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

struct Delivery {
    dirty: bool,
    next_heartbeat: Instant,
}

impl Delivery {
    fn new(now: Instant) -> Self {
        Self {
            dirty: true,
            next_heartbeat: now + HEARTBEAT_INTERVAL,
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn due(&self, now: Instant) -> bool {
        self.dirty || now >= self.next_heartbeat
    }

    fn sent(&mut self, now: Instant) {
        self.dirty = false;
        self.next_heartbeat = now + HEARTBEAT_INTERVAL;
    }
}

impl Controller {
    fn wait(&self, timeout: Option<Duration>) {
        let desired = self
            .desired
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if desired.is_some() {
            return;
        }
        match timeout {
            Some(timeout) => {
                drop(self.wake.wait_timeout(desired, timeout));
            }
            None => {
                drop(self.wake.wait(desired));
            }
        }
    }
}

pub fn setup(app: &mut tauri::App) {
    let controller = Arc::new(Controller::default());
    app.manage(controller.clone());
    let app = app.handle().clone();
    thread::spawn(move || supervise(app, controller));
}
pub fn shutdown(app: &AppHandle) {
    let controller = app.state::<Arc<Controller>>();
    controller.lifetime.shutdown();
    controller.wake.notify_all();
}
pub fn sync(app: &AppHandle, snapshot: NotchSnapshot) -> Result<(), String> {
    if snapshot.revision != super::config::revision() {
        return Ok(());
    }
    let controller = app.state::<Arc<Controller>>();
    *controller
        .desired
        .lock()
        .map_err(|_| "Native notch state unavailable.")? = Some(snapshot);
    controller.wake.notify_one();
    Ok(())
}
pub fn add_runtime_error(app: &AppHandle, snapshot: &mut NotchSnapshot) {
    if let Ok(error) = app.state::<Arc<Controller>>().error.lock() {
        if snapshot.error.is_none() {
            snapshot.error = error.clone();
        }
    }
}

fn supervise(app: AppHandle, controller: Arc<Controller>) {
    let (sender, reads) = mpsc::sync_channel::<(usize, Option<NativeProvider>)>(2);
    let mut snapshot = initial_snapshot();
    let mut connection: Option<Connection> = None;
    let mut connected_at = Instant::now();
    let mut providers: [Option<NativeProvider>; 2] = [None, None];
    let mut loading = [false; 2];
    let mut last_read: [Option<Instant>; 2] = [None, None];
    let mut force = [false; 2];
    let mut sequence = 0;
    let mut awaiting_ack: Option<(u64, Instant)> = None;
    let mut next_start = Instant::now();
    let mut retry_delay = 1u64;
    let mut action_error = None;
    let mut last_emitted = None;
    let mut runtime_error = None;
    let mut delivery = Delivery::new(Instant::now());
    while !controller.lifetime.stopped() {
        if let Ok(mut desired) = controller.desired.lock() {
            if let Some(next) = desired.take() {
                if next.revision == super::config::revision() {
                    snapshot = next;
                    next_start = Instant::now();
                    delivery.mark_dirty();
                }
            }
        }
        for (index, entry) in reads.try_iter() {
            providers[index] = entry;
            loading[index] = false;
            last_read[index] = Some(Instant::now());
            delivery.mark_dirty();
        }
        if !snapshot.settings.enabled {
            connection.take();
            awaiting_ack = None;
            runtime_error = None;
            retry_delay = 1;
        } else if connection.is_none() && Instant::now() >= next_start {
            match controller.lifetime.connect() {
                Ok(child) => {
                    connection = Some(child);
                    connected_at = Instant::now();
                    delivery.mark_dirty();
                    action_error = None;
                }
                Err(error) => {
                    runtime_error = Some(error);
                    next_start = Instant::now() + Duration::from_secs(retry_delay);
                    retry_delay = (retry_delay * 2).min(30);
                }
            }
        }
        let mut failure = None;
        if let Some(child) = &connection {
            loop {
                match child.events.try_recv() {
                    Ok(Ok(Action::Ready)) => {}
                    Ok(Ok(Action::Ack { sequence: ack })) => {
                        if awaiting_ack.is_some_and(|(sequence, _)| sequence == ack) {
                            awaiting_ack = None;
                            runtime_error = None;
                            if connected_at.elapsed() >= Duration::from_secs(30) {
                                retry_delay = 1;
                            }
                        }
                    }
                    Ok(Ok(Action::ScreensChanged)) => {
                        let next = super::read();
                        if next.revision == super::config::revision() {
                            snapshot = next;
                            delivery.mark_dirty();
                        }
                        last_read = [None, None];
                    }
                    Ok(Ok(Action::Refresh)) => force = [true, true],
                    Ok(Ok(Action::OpenLimits)) => {
                        action_error = crate::tray::open_limits_window(&app).err();
                        delivery.mark_dirty();
                    }
                    Ok(Err(error)) => {
                        failure = Some(error);
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        failure = Some("Native notch stopped unexpectedly. Retrying…".into());
                        break;
                    }
                }
            }
            if awaiting_ack.is_some_and(|(_, sent)| sent.elapsed() > Duration::from_secs(10)) {
                failure = Some("Native notch stopped responding. Retrying…".into());
            }
            if failure.is_none() && awaiting_ack.is_none() && delivery.due(Instant::now()) {
                sequence += 1;
                let message = Message {
                    version: 1,
                    sequence,
                    snapshot: &snapshot,
                    providers: providers.iter().flatten().collect(),
                    action_error: &action_error,
                };
                let result = serde_json::to_vec(&message)
                    .map_err(|e| e.to_string())
                    .and_then(|message| child.send(message));
                if let Err(error) = result {
                    failure = Some(error);
                } else {
                    awaiting_ack = Some((sequence, Instant::now()));
                    delivery.sent(Instant::now());
                }
            }
        }
        if let Some(error) = failure {
            connection.take();
            awaiting_ack = None;
            runtime_error = Some(error);
            next_start = Instant::now() + Duration::from_secs(retry_delay);
            retry_delay = (retry_delay * 2).min(30);
            delivery.mark_dirty();
        }
        let visible = snapshot.error.is_none()
            && layout(&snapshot.settings, &snapshot.displays, false).is_some();
        if connection.is_some() && visible {
            for (index, agent) in [AgentId::Claude, AgentId::Codex].into_iter().enumerate() {
                let due = usage_refresh_due(
                    last_read[index],
                    Instant::now(),
                    crate::limits_refresh::poll_interval(),
                    force[index],
                );
                if !loading[index] && due {
                    loading[index] = true;
                    let force = std::mem::take(&mut force[index]);
                    let sender = sender.clone();
                    thread::spawn(move || {
                        let _ = sender.send((
                            index,
                            current_provider(crate::limits_refresh::read_limits(agent, force)),
                        ));
                    });
                }
            }
        }
        if let Ok(mut error) = controller.error.lock() {
            *error = runtime_error.clone();
        }
        let mut status = snapshot.clone();
        if status.error.is_none() {
            status.error = runtime_error.clone();
        }
        if last_emitted.as_ref() != Some(&status) && status.revision == super::config::revision() {
            let _ = app.emit(
                "side-notch-changed",
                Changed {
                    snapshot: status.clone(),
                },
            );
            last_emitted = Some(status);
        }
        controller.wait(
            snapshot
                .settings
                .enabled
                .then_some(Duration::from_millis(100)),
        );
    }
}

fn usage_refresh_due(
    last_read: Option<Instant>,
    now: Instant,
    interval: Duration,
    force: bool,
) -> bool {
    force || last_read.is_none_or(|last_read| now.saturating_duration_since(last_read) >= interval)
}

fn initial_snapshot() -> NotchSnapshot {
    let revision = super::config::revision();
    match super::config::read() {
        Ok(settings) if settings.enabled => super::read(),
        Ok(settings) => NotchSnapshot {
            revision,
            supported: true,
            settings,
            displays: Vec::new(),
            error: None,
        },
        Err(error) => NotchSnapshot {
            revision,
            supported: true,
            settings: Default::default(),
            displays: Vec::new(),
            error: Some(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outbound_delivery_is_dirty_driven_with_a_slow_heartbeat() {
        let now = Instant::now();
        let mut delivery = Delivery::new(now);
        assert!(delivery.due(now));
        delivery.sent(now);
        assert!(!delivery.due(now + Duration::from_secs(2)));
        delivery.mark_dirty();
        assert!(delivery.due(now + Duration::from_secs(2)));
        delivery.sent(now + Duration::from_secs(2));
        assert!(!delivery.due(now + Duration::from_secs(20)));
        assert!(delivery.due(now + HEARTBEAT_INTERVAL + Duration::from_secs(2)));
        assert!(HEARTBEAT_INTERVAL >= Duration::from_secs(30));
    }

    #[test]
    fn sends_only_the_current_account_and_omits_account_identifiers() {
        let entries: Vec<ProviderLimitsDto> = serde_json::from_value(serde_json::json!([
            {"provider":"claude","status":"ok","currentAccount":false,"plan":"remembered-plan","windows":[]},
            {"provider":"claude","status":"signedOut","currentAccount":true,"account":{"id":"private-id","label":"private-label"},"plan":"max","windows":[]}
        ])).unwrap();
        let entry = current_provider(entries).unwrap();
        assert_eq!(entry.status, LimitsStatus::SignedOut);
        assert_eq!(entry.plan.as_deref(), Some("max"));
        let payload = serde_json::to_value(entry).unwrap();
        assert!(payload.get("account").is_none());
        assert!(payload.get("credits").is_none());
        assert!(current_provider(Vec::new()).is_none());
    }

    #[test]
    fn notch_usage_uses_the_configured_interval_unless_forced() {
        let last_read = Instant::now();
        let interval = Duration::from_secs(5 * 60);
        assert!(!usage_refresh_due(
            Some(last_read),
            last_read + Duration::from_secs(299),
            interval,
            false
        ));
        assert!(usage_refresh_due(
            Some(last_read),
            last_read + interval,
            interval,
            false
        ));
        assert!(usage_refresh_due(
            Some(last_read),
            last_read + Duration::from_secs(1),
            interval,
            true
        ));
    }
}
