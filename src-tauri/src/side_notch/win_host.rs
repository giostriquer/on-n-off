//! The Windows side-notch supervisor: the `window.rs::supervise` port. Same poll and
//! delivery cadence (intervals, read deadlines, force flags, dirty + heartbeat), with
//! the pipe transport replaced by an in-process channel to the window thread.

#![allow(unsafe_code)]

use super::model::{layout, GithubList, NotchSnapshot, RAIL_ORDER};
use super::win_paint::{
    CellData, PrCellData, PrListData, PrRowData, ProviderData, RailData, MAX_PULL_REQUESTS,
};
use super::win_window::{WinAction, WindowMsg};
use crate::dto::{
    AgentId, GithubPrDto, GithubPrsDto, LimitWindowDto, LimitsStatus, ProviderLimitsDto,
};
use crate::side_notch::sessions::{self, LiveSession};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Live session rows are small local reads; the popover shows "just now" style ages.
const SESSIONS_INTERVAL: Duration = Duration::from_secs(10);
/// A read that never reports back (a hung child, a panicked thread) releases its slot after this.
const READ_DEADLINE: Duration = Duration::from_secs(60);
const PROVIDER_COUNT: usize = RAIL_ORDER.len();
/// One in-flight read per provider, plus the sessions read and the pull-request read.
const READ_SLOTS: usize = PROVIDER_COUNT + 2;

/// A value refreshed by a background read: at most one read in flight, refreshed on an
/// interval, forced on request, and never left "loading" forever.
pub(crate) struct Poll<T> {
    pub value: T,
    loading: bool,
    started: Option<Instant>,
    last_read: Option<Instant>,
    force: bool,
}

impl<T> Poll<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            loading: false,
            started: None,
            last_read: None,
            force: false,
        }
    }

    pub fn due(&self, now: Instant, interval: Duration) -> bool {
        !self.loading
            && (self.force
                || self
                    .last_read
                    .is_none_or(|last_read| now.saturating_duration_since(last_read) >= interval))
    }

    /// Marks a read in flight and returns whether it was forced.
    pub fn start(&mut self, now: Instant) -> bool {
        self.loading = true;
        self.started = Some(now);
        std::mem::take(&mut self.force)
    }

    pub fn finish(&mut self, value: T, now: Instant) {
        self.value = value;
        self.loading = false;
        self.last_read = Some(now);
    }

    pub fn release_stale(&mut self, now: Instant) {
        if self.loading
            && self
                .started
                .is_some_and(|started| now.saturating_duration_since(started) > READ_DEADLINE)
        {
            self.loading = false;
        }
    }
}

/// Frames go to the window when something changed, or as a slow heartbeat so a stale
/// render can never outlive its data.
pub(crate) struct Delivery {
    dirty: bool,
    next_heartbeat: Instant,
}

impl Delivery {
    pub fn new(now: Instant) -> Self {
        Self {
            dirty: true,
            next_heartbeat: now + HEARTBEAT_INTERVAL,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn due(&self, now: Instant) -> bool {
        self.dirty || now >= self.next_heartbeat
    }

    pub fn sent(&mut self, now: Instant) {
        self.dirty = false;
        self.next_heartbeat = now + HEARTBEAT_INTERVAL;
    }
}

/// One provider on the in-memory wire: its quota snapshot.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeProvider {
    pub provider: AgentId,
    pub status: LimitsStatus,
    pub message: Option<String>,
    pub windows: Vec<LimitWindowDto>,
}

fn current_provider(entries: Vec<ProviderLimitsDto>) -> Option<NativeProvider> {
    entries
        .into_iter()
        .find(|entry| entry.current_account)
        .map(|entry| NativeProvider {
            provider: entry.provider,
            status: entry.status,
            message: entry.message,
            windows: entry.windows,
        })
}

/// The pull-request cell's data: only the selected lists, each capped, with the row
/// fields the popover shows. No account identifiers beyond the author logins GitHub
/// already displays.
fn to_row(pr: &GithubPrDto) -> Option<PrRowData> {
    if !PrRowData::keeps(&pr.url) {
        return None;
    }
    Some(PrRowData {
        id: pr.id.clone(),
        number: pr.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        repo: pr.repo.clone(),
        is_draft: pr.is_draft,
        review_decision: pr.review_decision,
        ci: pr.ci,
        merge_kind: pr.merge_kind,
    })
}

fn pr_cell(dto: &GithubPrsDto, selected: &[GithubList]) -> PrCellData {
    PrCellData {
        status: dto.status,
        hint: dto.hint.clone(),
        stale: dto.stale,
        lists: selected
            .iter()
            .map(|list| {
                let source = match list {
                    GithubList::Mine => &dto.data.mine,
                    GithubList::ReviewRequested => &dto.data.review_requested,
                    GithubList::Assigned => &dto.data.assigned,
                };
                PrListData {
                    id: *list,
                    total: source.total,
                    items: source
                        .items
                        .iter()
                        .filter_map(to_row)
                        .take(MAX_PULL_REQUESTS)
                        .collect(),
                }
            })
            .collect(),
    }
}

/// The selected providers' current-account entries in rail order.
fn rail_cells(
    snapshot: &NotchSnapshot,
    providers: &[Poll<Option<NativeProvider>>],
    session_rows: &[Vec<LiveSession>],
    pulls: &Poll<Option<GithubPrsDto>>,
) -> Vec<CellData> {
    let selected = snapshot.settings.rail_providers();
    let mut cells: Vec<CellData> = RAIL_ORDER
        .iter()
        .enumerate()
        .filter(|(_, agent)| selected.contains(agent))
        .filter_map(|(index, _)| {
            providers[index].value.as_ref().map(|entry| {
                CellData::Provider(ProviderData {
                    provider: entry.provider,
                    status: entry.status,
                    message: entry.message.clone(),
                    windows: entry.windows.clone(),
                    sessions: session_rows[index].clone(),
                })
            })
        })
        .collect();
    if snapshot.settings.pull_requests.enabled {
        if let Some(dto) = &pulls.value {
            cells.push(CellData::PullRequests(pr_cell(
                dto,
                &snapshot.settings.pull_requests.selected_lists(),
            )));
        }
    }
    cells
}

/// Live sessions for the selected providers only, in rail order; hidden cells cost nothing.
fn read_sessions(selected: &[AgentId]) -> Vec<Vec<LiveSession>> {
    let Ok(home) = crate::paths::user_home() else {
        return vec![Vec::new(); PROVIDER_COUNT];
    };
    let now = chrono::Utc::now();
    RAIL_ORDER
        .into_iter()
        .map(|agent| {
            if selected.contains(&agent) {
                sessions::read(agent, &home, now)
            } else {
                Vec::new()
            }
        })
        .collect()
}

enum Read {
    Limits(usize, Option<NativeProvider>),
    Sessions(Vec<Vec<LiveSession>>),
    /// The screen's whole answer; the selected lists are projected when a frame is
    /// built, so a settings change shows at once instead of after the next poll.
    PullRequests(GithubPrsDto),
}

#[derive(Default)]
struct Controller {
    desired: Mutex<Option<NotchSnapshot>>,
    wake: Condvar,
    stopped: AtomicBool,
    error: Mutex<Option<String>>,
    /// The live window-thread sender, kept so shutdown can end the overlay loop.
    window: Mutex<Option<mpsc::Sender<WindowMsg>>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Changed {
    snapshot: NotchSnapshot,
}

pub fn setup(app: &mut tauri::App) {
    let controller = Arc::new(Controller {
        stopped: AtomicBool::new(false),
        ..Controller::default()
    });
    app.manage(controller.clone());
    let app = app.handle().clone();
    thread::Builder::new()
        .name("side-notch-host".into())
        .spawn(move || supervise(app, controller))
        .expect("spawn the side notch supervisor");
}

pub fn shutdown(app: &AppHandle) {
    let controller = app.state::<Arc<Controller>>();
    controller.stopped.store(true, Ordering::SeqCst);
    if let Ok(window) = controller.window.lock() {
        if let Some(sender) = window.as_ref() {
            let _ = sender.send(WindowMsg::Shutdown);
        }
    }
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

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
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

fn supervise(app: AppHandle, controller: Arc<Controller>) {
    let (sender, reads) = mpsc::sync_channel::<Read>(READ_SLOTS);
    let mut snapshot = initial_snapshot();
    let (action_tx, action_rx) = mpsc::channel::<WinAction>();
    let mut action_rx = action_rx;
    let mut window_tx = super::win_window::spawn(action_tx);
    if let Ok(mut window) = controller.window.lock() {
        *window = Some(window_tx.clone());
    }
    let mut providers: [Poll<Option<NativeProvider>>; PROVIDER_COUNT] =
        [(); PROVIDER_COUNT].map(|_| Poll::new(None));
    let mut session_poll: Poll<Vec<Vec<LiveSession>>> = Poll::new(vec![Vec::new(); PROVIDER_COUNT]);
    let mut pulls: Poll<Option<GithubPrsDto>> = Poll::new(None);
    let mut pulls_interval = Duration::from_secs(60);
    let mut retry_delay = 1u64;
    let mut action_error = None;
    let mut last_emitted = None;
    let mut runtime_error = None;
    let mut delivery = Delivery::new(Instant::now());

    while !controller.stopped() {
        if let Ok(mut desired) = controller.desired.lock() {
            if let Some(next) = desired.take() {
                if next.revision == super::config::revision() {
                    snapshot = next;
                    delivery.mark_dirty();
                }
            }
        }
        for read in reads.try_iter() {
            match read {
                Read::Limits(index, entry) => {
                    providers[index].finish(entry, Instant::now());
                    delivery.mark_dirty();
                }
                Read::Sessions(latest) => {
                    if latest != session_poll.value {
                        delivery.mark_dirty();
                    }
                    session_poll.finish(latest, Instant::now());
                }
                Read::PullRequests(latest) => {
                    if pulls.value.as_ref() != Some(&latest) {
                        delivery.mark_dirty();
                    }
                    pulls.finish(Some(latest), Instant::now());
                }
            }
        }
        if !snapshot.settings.enabled {
            runtime_error = None;
            retry_delay = 1;
        }
        // The window thread died (a panic): surface it and restart with backoff.
        let action_error_ref = action_error.as_deref();
        let message = if delivery.due(Instant::now()) {
            WindowMsg::Data(rail_data(
                &snapshot,
                &providers,
                &session_poll,
                &pulls,
                action_error_ref,
            ))
        } else {
            WindowMsg::Ping
        };
        if window_tx.send(message).is_err() {
            runtime_error = Some("Native notch stopped unexpectedly. Retrying…".into());
            retry_delay = (retry_delay * 2).min(30);
            delivery.mark_dirty();
            let (action_tx, action_rx_next) = mpsc::channel::<WinAction>();
            window_tx = super::win_window::spawn(action_tx);
            action_rx = action_rx_next;
            if let Ok(mut window) = controller.window.lock() {
                *window = Some(window_tx.clone());
            }
            delivery.sent(Instant::now());
            continue;
        }
        if delivery.due(Instant::now()) {
            delivery.sent(Instant::now());
        }
        for action in action_rx.try_iter() {
            handle_action(
                &app,
                &mut snapshot,
                &mut providers,
                &mut session_poll,
                &mut pulls,
                action,
                &mut action_error,
            );
            delivery.mark_dirty();
        }
        let visible =
            snapshot.error.is_none() && layout(&snapshot.settings, &snapshot.displays).is_some();
        if visible {
            let now = Instant::now();
            let selected = snapshot.settings.rail_providers();
            for poll in &mut providers {
                poll.release_stale(now);
            }
            session_poll.release_stale(now);
            let interval = crate::limits_refresh::poll_interval();
            for (index, agent) in RAIL_ORDER.into_iter().enumerate() {
                if !selected.contains(&agent) || !providers[index].due(now, interval) {
                    continue;
                }
                let force = providers[index].start(now);
                let sender = sender.clone();
                thread::spawn(move || {
                    let _ = sender.send(Read::Limits(
                        index,
                        current_provider(crate::limits_refresh::read_limits(agent, force)),
                    ));
                });
            }
            if session_poll.due(now, SESSIONS_INTERVAL) {
                session_poll.start(now);
                let sender = sender.clone();
                let selected = selected.clone();
                thread::spawn(move || {
                    let _ = sender.send(Read::Sessions(read_sessions(&selected)));
                });
            }
            pulls.release_stale(now);
            if snapshot.settings.pull_requests.enabled && pulls.due(now, pulls_interval) {
                let force = pulls.start(now);
                // One settings read per poll keeps the cadence in step with the screen's.
                pulls_interval = Duration::from_secs(u64::from(
                    crate::settings::load_settings().github_poll_seconds,
                ));
                let sender = sender.clone();
                thread::spawn(move || {
                    // `read_prs` memoises within the screen's poll window, so this adds
                    // no GitHub calls beyond what the screen and monitor already make.
                    let _ = sender.send(Read::PullRequests(crate::github::read_prs(force)));
                });
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

fn rail_data(
    snapshot: &NotchSnapshot,
    providers: &[Poll<Option<NativeProvider>>],
    sessions: &Poll<Vec<Vec<LiveSession>>>,
    pulls: &Poll<Option<GithubPrsDto>>,
    action_error: Option<&str>,
) -> RailData {
    RailData {
        settings: snapshot.settings.clone(),
        cells: rail_cells(snapshot, providers, &sessions.value, pulls),
        action_error: action_error.map(str::to_string),
    }
}

fn handle_action(
    app: &AppHandle,
    snapshot: &mut NotchSnapshot,
    providers: &mut [Poll<Option<NativeProvider>>],
    session_poll: &mut Poll<Vec<Vec<LiveSession>>>,
    pulls: &mut Poll<Option<GithubPrsDto>>,
    action: WinAction,
    action_error: &mut Option<String>,
) {
    match action {
        WinAction::Refresh => {
            for poll in &mut *providers {
                poll.force = true;
            }
            session_poll.force = true;
            pulls.force = true;
        }
        WinAction::OpenLimits => {
            *action_error = crate::tray::open_limits_window(app).err();
        }
        WinAction::OpenPullRequests => {
            *action_error = crate::tray::open_github_window(app).err();
        }
        WinAction::SetShow(show) => {
            // The same validated save the Settings card uses; the new revision reaches
            // the card through the changed event below.
            let mut settings = snapshot.settings.clone();
            settings.show = show;
            match super::save(settings) {
                Ok(next) => *snapshot = next,
                Err(error) => *action_error = Some(error),
            }
        }
        WinAction::OpenUrl(url) => {
            use tauri_plugin_opener::OpenerExt;
            *action_error = if crate::item_install::is_openable_url(&url) {
                app.opener()
                    .open_url(&url, None::<&str>)
                    .err()
                    .map(|error| error.to_string())
            } else {
                Some(format!("refusing to open {url}"))
            };
        }
        WinAction::CopyReviewRequest { title, url } => {
            *action_error = copy_review_request(&title, &url).err();
        }
    }
}

/// "review please: <title> <url>", the plain-text form of the macOS helper's
/// rich-text pasteboard write; the Windows clipboard keeps one text format.
fn copy_review_request(title: &str, url: &str) -> Result<(), String> {
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        OpenClipboard(None).map_err(|e| e.to_string())?;
        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| e.to_string())?;
            let text = format!("review please: {title} {url}\0");
            let bytes: Vec<u16> = text.encode_utf16().collect();
            let handle: HGLOBAL =
                GlobalAlloc(GMEM_MOVEABLE, bytes.len() * 2).map_err(|e| e.to_string())?;
            let locked = GlobalLock(handle);
            if locked.is_null() {
                let _ = GlobalUnlock(handle);
                let _ = GlobalFree(Some(handle));
                return Err("Cannot lock the clipboard buffer.".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), locked as *mut u16, bytes.len());
            let _ = GlobalUnlock(handle);
            // On success the system owns the handle; on failure we free it.
            match SetClipboardData(CF_UNICODETEXT, Some(HANDLE(handle.0))) {
                Ok(_) => Ok(()),
                Err(error) => {
                    let _ = GlobalFree(Some(handle));
                    Err(error.to_string())
                }
            }
        })();
        let _ = CloseClipboard();
        result
    }
}

#[cfg(test)]
mod tests;
