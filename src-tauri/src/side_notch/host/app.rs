use super::{App, Host, Lifetime, Page, Transport};
use crate::dto::{AgentId, GithubPrsDto, ProviderLimitsDto};
use crate::side_notch::model::{NotchSettings, NotchSnapshot};
use crate::side_notch::sessions::LiveSession;
use std::{
    sync::{atomic::AtomicU64, Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

struct Controller {
    desired: Mutex<Option<NotchSnapshot>>,
    wake: Condvar,
    error: Mutex<Option<String>>,
    lifetime: Arc<dyn Lifetime>,
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
    #[cfg(target_os = "macos")]
    let (transport, lifetime) = crate::side_notch::transport::Pipe::new();
    #[cfg(target_os = "windows")]
    let (transport, lifetime) =
        crate::side_notch::win_transport::Overlay::new(app.handle().clone());
    let controller = Arc::new(Controller {
        desired: Mutex::new(None),
        wake: Condvar::new(),
        error: Mutex::new(None),
        lifetime,
    });
    app.manage(controller.clone());
    let app = app.handle().clone();
    thread::Builder::new()
        .name("side-notch-host".into())
        .spawn(move || supervise(app, &controller, transport))
        .expect("spawn the side notch supervisor");
}

pub fn shutdown(app: &AppHandle) {
    let controller = app.state::<Arc<Controller>>();
    controller.lifetime.shutdown();
    controller.wake.notify_all();
}

pub fn sync(app: &AppHandle, snapshot: NotchSnapshot) -> Result<(), String> {
    if snapshot.revision != crate::side_notch::config::revision() {
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

fn supervise(app: AppHandle, controller: &Controller, transport: impl Transport) {
    let mut host = Host::new(
        Production::new(app),
        transport,
        initial_snapshot(),
        Instant::now(),
    );
    while !controller.lifetime.stopped() {
        if let Ok(mut desired) = controller.desired.lock() {
            if let Some(next) = desired.take() {
                host.desire(next);
            }
        }
        host.tick(Instant::now());
        if let Ok(mut error) = controller.error.lock() {
            *error = host.runtime_error();
        }
        host.publish();
        controller.wait(host.enabled().then_some(Duration::from_millis(100)));
    }
}

fn initial_snapshot() -> NotchSnapshot {
    let revision = crate::side_notch::config::revision();
    match crate::side_notch::config::read() {
        Ok(settings) if settings.enabled => crate::side_notch::read(),
        Ok(settings) => NotchSnapshot {
            revision,
            supported: true,
            liquid_glass: crate::side_notch::liquid_glass(),
            settings,
            displays: Vec::new(),
            error: None,
        },
        Err(error) => NotchSnapshot {
            revision,
            supported: true,
            liquid_glass: crate::side_notch::liquid_glass(),
            settings: Default::default(),
            displays: Vec::new(),
            error: Some(error),
        },
    }
}

pub(super) trait Shell: Clone + Send + 'static {
    fn open(&self, page: Page) -> Result<(), String>;
    fn emit(&self, status: &NotchSnapshot);
}

#[derive(Clone)]
pub(super) struct Production<S> {
    shell: S,
    limits_poll: &'static AtomicU64,
    pull_requests_poll: &'static AtomicU64,
}

impl<S> Production<S> {
    fn new(shell: S) -> Self {
        Self {
            shell,
            limits_poll: &crate::limits_refresh::POLL_SECONDS,
            pull_requests_poll: &crate::github::POLL_SECONDS,
        }
    }
}

impl<S: Shell> App for Production<S> {
    fn read_limits(&self, agent: AgentId, force: bool) -> (Vec<ProviderLimitsDto>, u64) {
        crate::limits_refresh::read_limits_revisioned(agent, force)
    }

    fn limits_revision(&self, agent: AgentId) -> u64 {
        crate::limits_refresh::revision(agent)
    }

    fn limits_interval(&self) -> Duration {
        crate::limits_refresh::check_interval(crate::limits_refresh::poll_interval_in(
            self.limits_poll,
        ))
    }

    fn read_pull_requests(&self, force: bool) -> (GithubPrsDto, u64) {
        crate::github::read_prs_revisioned(force)
    }

    fn pull_requests_revision(&self) -> u64 {
        crate::github::revision()
    }

    fn pull_requests_interval(&self) -> Duration {
        crate::github::poll_interval_in(self.pull_requests_poll)
    }

    fn read_sessions(&self, agent: AgentId) -> Vec<LiveSession> {
        crate::paths::user_home()
            .map(|home| crate::side_notch::sessions::read(agent, &home, chrono::Utc::now()))
            .unwrap_or_default()
    }

    fn spawn(&self, read: impl FnOnce() + Send + 'static) {
        thread::spawn(read);
    }

    fn open(&self, page: Page) -> Result<(), String> {
        self.shell.open(page)
    }

    fn settings_revision(&self) -> u64 {
        crate::side_notch::config::revision()
    }

    fn reread(&self) -> NotchSnapshot {
        crate::side_notch::read()
    }

    fn save(&self, settings: NotchSettings) -> Result<NotchSnapshot, String> {
        crate::side_notch::save(settings)
    }

    fn publish(&self, status: &NotchSnapshot) {
        self.shell.emit(status);
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Changed {
    snapshot: NotchSnapshot,
}

impl Shell for AppHandle {
    fn open(&self, page: Page) -> Result<(), String> {
        match page {
            Page::Limits => crate::tray::open_limits_window(self),
            Page::PullRequests => crate::tray::open_github_window(self),
        }
    }

    fn emit(&self, status: &NotchSnapshot) {
        let _ = Emitter::emit(
            self,
            "side-notch-changed",
            Changed {
                snapshot: status.clone(),
            },
        );
    }
}

#[cfg(test)]
mod tests;
