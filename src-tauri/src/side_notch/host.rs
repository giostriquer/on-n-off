use super::model::{
    layout, GithubList, NotchProvider, NotchSettings, NotchSnapshot, PrContext, ShowMode,
    RAIL_ORDER,
};
use super::sessions::LiveSession;
use crate::dto::{
    AgentId, CiState, GithubPrDto, GithubPrsDto, GithubStatus, MergeKind, ProviderLimitsDto,
    ReviewDecision,
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
#[cfg(target_os = "windows")]
use std::{
    sync::{Arc, Condvar, Mutex},
    thread,
};
#[cfg(target_os = "windows")]
use tauri::{AppHandle, Emitter, Manager};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const SESSIONS_INTERVAL: Duration = Duration::from_secs(10);
const READ_DEADLINE: Duration = Duration::from_secs(60);
const PROVIDER_COUNT: usize = RAIL_ORDER.len();
const READ_SLOTS: usize = PROVIDER_COUNT + 2;
pub const MAX_PULL_REQUESTS: usize = 25;

#[derive(Clone, Debug, PartialEq)]
pub struct RailData {
    pub settings: NotchSettings,
    pub cells: Vec<CellData>,
    pub action_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CellData {
    Provider(ProviderData),
    PullRequests(PrCellData),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderData {
    pub cell: NotchProvider,
    pub sessions: Vec<LiveSession>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrCellData {
    pub status: GithubStatus,
    pub hint: Option<String>,
    pub stale: bool,
    pub lists: Vec<PrListData>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrListData {
    pub id: GithubList,
    pub total: u64,
    pub items: Vec<PrRowData>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrRowData {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub is_draft: bool,
    pub review_decision: Option<ReviewDecision>,
    pub ci: CiState,
    pub merge_kind: Option<MergeKind>,
    pub context: Option<PrContext>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Action {
    Refresh,
    OpenLimits,
    OpenPullRequests,
    SetShow(ShowMode),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Inbound {
    Action(Action),
    Handled(Result<(), String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Link {
    Opened,
    Open,
}

pub(super) trait Transport {
    fn connect(&mut self) -> Result<Link, String>;
    fn receive(&mut self, inbound: &mut Vec<Inbound>) -> Result<(), String>;
    fn send(&mut self, frame: RailData) -> Result<(), String>;
    fn close(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Limits,
    PullRequests,
}

pub(super) trait App: Clone + Send + 'static {
    fn read_limits(&self, agent: AgentId, force: bool) -> (Vec<ProviderLimitsDto>, u64);
    fn limits_revision(&self, agent: AgentId) -> u64;
    fn limits_interval(&self) -> Duration;
    fn read_pull_requests(&self, force: bool) -> (GithubPrsDto, u64);
    fn pull_requests_revision(&self) -> u64;
    fn pull_requests_interval(&self) -> Duration;
    fn read_sessions(&self, agent: AgentId) -> Vec<LiveSession>;
    fn spawn(&self, read: impl FnOnce() + Send + 'static);
    fn open(&self, page: Page) -> Result<(), String>;
    fn settings_revision(&self) -> u64;
    fn save(&self, settings: NotchSettings) -> Result<NotchSnapshot, String>;
    fn publish(&self, status: &NotchSnapshot);
}

struct Poll<T> {
    value: T,
    loading: bool,
    started: Option<Instant>,
    last_read: Option<Instant>,
    force: bool,
    read_at: u64,
}

impl<T> Poll<T> {
    fn new(value: T) -> Self {
        Self {
            value,
            loading: false,
            started: None,
            last_read: None,
            force: false,
            read_at: 0,
        }
    }

    fn due(&self, now: Instant, interval: Duration, latest: u64) -> bool {
        !self.loading
            && (self.force
                || self.read_at != latest
                || self
                    .last_read
                    .is_none_or(|last_read| now.saturating_duration_since(last_read) >= interval))
    }

    fn start(&mut self, now: Instant) -> bool {
        self.loading = true;
        self.started = Some(now);
        std::mem::take(&mut self.force)
    }

    fn finish(&mut self, value: T, now: Instant, read_at: u64) {
        self.value = value;
        self.loading = false;
        self.last_read = Some(now);
        self.read_at = read_at;
    }

    fn release_stale(&mut self, now: Instant) {
        if self.loading
            && self
                .started
                .is_some_and(|started| now.saturating_duration_since(started) > READ_DEADLINE)
        {
            self.loading = false;
        }
    }
}

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

enum Read {
    Limits {
        index: usize,
        revision: u64,
        entry: Option<NotchProvider>,
    },
    Sessions(Vec<Vec<LiveSession>>),
    PullRequests {
        revision: u64,
        dto: Box<GithubPrsDto>,
    },
}

pub(super) struct Host<T, A> {
    app: A,
    transport: T,
    snapshot: NotchSnapshot,
    providers: [Poll<Option<NotchProvider>>; PROVIDER_COUNT],
    sessions: Poll<Vec<Vec<LiveSession>>>,
    pulls: Poll<Option<GithubPrsDto>>,
    pulls_interval: Duration,
    delivery: Delivery,
    action_error: Option<String>,
    runtime_error: Option<String>,
    last_emitted: Option<NotchSnapshot>,
    sender: mpsc::SyncSender<Read>,
    reads: mpsc::Receiver<Read>,
}

impl<T: Transport, A: App> Host<T, A> {
    pub fn new(app: A, transport: T, snapshot: NotchSnapshot, now: Instant) -> Self {
        let (sender, reads) = mpsc::sync_channel::<Read>(READ_SLOTS);
        Self {
            app,
            transport,
            snapshot,
            providers: [(); PROVIDER_COUNT].map(|_| Poll::new(None)),
            sessions: Poll::new(vec![Vec::new(); PROVIDER_COUNT]),
            pulls: Poll::new(None),
            pulls_interval: Duration::from_secs(60),
            delivery: Delivery::new(now),
            action_error: None,
            runtime_error: None,
            last_emitted: None,
            sender,
            reads,
        }
    }

    pub fn enabled(&self) -> bool {
        self.snapshot.settings.enabled
    }

    pub fn runtime_error(&self) -> Option<String> {
        self.runtime_error.clone()
    }

    pub fn desire(&mut self, next: NotchSnapshot) {
        if next.revision == self.app.settings_revision() {
            self.snapshot = next;
            self.delivery.mark_dirty();
        }
    }

    pub fn tick(&mut self, now: Instant) {
        self.collect(now);
        if self.exchange(now) && self.visible() {
            self.start_reads(now);
        }
        self.publish();
    }

    fn collect(&mut self, now: Instant) {
        for read in self.reads.try_iter() {
            match read {
                Read::Limits {
                    index,
                    revision,
                    entry,
                } => {
                    self.providers[index].finish(entry, now, revision);
                    self.delivery.mark_dirty();
                }
                Read::Sessions(latest) => {
                    if latest != self.sessions.value {
                        self.delivery.mark_dirty();
                    }
                    self.sessions.finish(latest, now, 0);
                }
                Read::PullRequests { revision, dto } => {
                    if self.pulls.value.as_ref() != Some(&*dto) {
                        self.delivery.mark_dirty();
                    }
                    self.pulls.finish(Some(*dto), now, revision);
                }
            }
        }
    }

    fn exchange(&mut self, now: Instant) -> bool {
        if !self.snapshot.settings.enabled {
            self.transport.close();
            self.runtime_error = None;
            return false;
        }
        match self.transport.connect() {
            Ok(Link::Opened) => {
                self.delivery.mark_dirty();
                self.action_error = None;
            }
            Ok(Link::Open) => {}
            Err(error) => {
                self.runtime_error = Some(error);
                return false;
            }
        }
        let mut inbound = Vec::new();
        let received = self.transport.receive(&mut inbound);
        for item in inbound {
            self.handle(item);
        }
        match received.and_then(|()| self.deliver(now)) {
            Ok(()) => true,
            Err(error) => {
                self.runtime_error = Some(error);
                self.delivery.mark_dirty();
                false
            }
        }
    }

    fn handle(&mut self, item: Inbound) {
        match item {
            Inbound::Action(Action::Refresh) => {
                for poll in &mut self.providers {
                    poll.force = true;
                }
                self.sessions.force = true;
                self.pulls.force = true;
            }
            Inbound::Action(Action::OpenLimits) => {
                self.action_error = self.app.open(Page::Limits).err();
                self.delivery.mark_dirty();
            }
            Inbound::Action(Action::OpenPullRequests) => {
                self.action_error = self.app.open(Page::PullRequests).err();
                self.delivery.mark_dirty();
            }
            Inbound::Action(Action::SetShow(show)) => {
                let mut settings = self.snapshot.settings.clone();
                settings.show = show;
                match self.app.save(settings) {
                    Ok(next) => {
                        self.snapshot = next;
                        self.action_error = None;
                    }
                    Err(error) => self.action_error = Some(error),
                }
                self.delivery.mark_dirty();
            }
            Inbound::Handled(result) => {
                self.action_error = result.err();
                self.delivery.mark_dirty();
            }
        }
    }

    fn deliver(&mut self, now: Instant) -> Result<(), String> {
        if !self.delivery.due(now) {
            return Ok(());
        }
        let frame = self.rail();
        self.transport.send(frame)?;
        self.delivery.sent(now);
        Ok(())
    }

    fn visible(&self) -> bool {
        self.snapshot.error.is_none()
            && layout(&self.snapshot.settings, &self.snapshot.displays).is_some()
    }

    fn start_reads(&mut self, now: Instant) {
        let selected = self.snapshot.settings.rail_providers();
        for poll in &mut self.providers {
            poll.release_stale(now);
        }
        self.sessions.release_stale(now);
        let interval = self.app.limits_interval();
        for (index, agent) in RAIL_ORDER.into_iter().enumerate() {
            if !selected.contains(&agent)
                || !self.providers[index].due(now, interval, self.app.limits_revision(agent))
            {
                continue;
            }
            let force = self.providers[index].start(now);
            let (app, sender) = (self.app.clone(), self.sender.clone());
            self.app.spawn(move || {
                let (entries, revision) = app.read_limits(agent, force);
                let _ = sender.send(Read::Limits {
                    index,
                    revision,
                    entry: NotchProvider::current(entries),
                });
            });
        }
        if self.sessions.due(now, SESSIONS_INTERVAL, 0) {
            self.sessions.start(now);
            let (app, sender) = (self.app.clone(), self.sender.clone());
            let selected = selected.clone();
            self.app.spawn(move || {
                let _ = sender.send(Read::Sessions(read_sessions(&app, &selected)));
            });
        }
        self.pulls.release_stale(now);
        if self.snapshot.settings.pull_requests.enabled
            && self
                .pulls
                .due(now, self.pulls_interval, self.app.pull_requests_revision())
        {
            let force = self.pulls.start(now);
            self.pulls_interval = self.app.pull_requests_interval();
            let (app, sender) = (self.app.clone(), self.sender.clone());
            self.app.spawn(move || {
                let (dto, revision) = app.read_pull_requests(force);
                let _ = sender.send(Read::PullRequests {
                    revision,
                    dto: Box::new(dto),
                });
            });
        }
    }

    fn publish(&mut self) {
        let mut status = self.snapshot.clone();
        if status.error.is_none() {
            status.error = self.runtime_error.clone();
        }
        if self.last_emitted.as_ref() != Some(&status)
            && status.revision == self.app.settings_revision()
        {
            self.app.publish(&status);
            self.last_emitted = Some(status);
        }
    }

    fn rail(&self) -> RailData {
        RailData {
            settings: self.snapshot.settings.clone(),
            cells: self.cells(),
            action_error: self.action_error.clone(),
        }
    }

    fn cells(&self) -> Vec<CellData> {
        let selected = self.snapshot.settings.rail_providers();
        let mut cells: Vec<CellData> = RAIL_ORDER
            .iter()
            .enumerate()
            .filter(|(_, agent)| selected.contains(agent))
            .filter_map(|(index, _)| {
                self.providers[index].value.as_ref().map(|entry| {
                    CellData::Provider(ProviderData {
                        cell: entry.clone(),
                        sessions: self.sessions.value[index].clone(),
                    })
                })
            })
            .collect();
        if self.snapshot.settings.pull_requests.enabled {
            if let Some(dto) = &self.pulls.value {
                cells.push(CellData::PullRequests(pr_cell(
                    dto,
                    &self.snapshot.settings.pull_requests.selected_lists(),
                )));
            }
        }
        cells
    }
}

fn read_sessions(app: &impl App, selected: &[AgentId]) -> Vec<Vec<LiveSession>> {
    RAIL_ORDER
        .into_iter()
        .map(|agent| {
            if selected.contains(&agent) {
                app.read_sessions(agent)
            } else {
                Vec::new()
            }
        })
        .collect()
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
                        .filter_map(pr_row)
                        .take(MAX_PULL_REQUESTS)
                        .collect(),
                }
            })
            .collect(),
    }
}

fn pr_row(pr: &GithubPrDto) -> Option<PrRowData> {
    if !pr.url.starts_with("https://github.com/") {
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
        context: PrContext::of(pr),
    })
}

#[cfg(target_os = "windows")]
pub(super) trait Lifetime: Send + Sync {
    fn stopped(&self) -> bool;
    fn shutdown(&self);
}

#[cfg(target_os = "windows")]
struct Controller {
    desired: Mutex<Option<NotchSnapshot>>,
    wake: Condvar,
    error: Mutex<Option<String>>,
    lifetime: Arc<dyn Lifetime>,
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
pub fn setup(app: &mut tauri::App) {
    let (transport, lifetime) = super::win_transport::Overlay::new(app.handle().clone());
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

#[cfg(target_os = "windows")]
pub fn shutdown(app: &AppHandle) {
    let controller = app.state::<Arc<Controller>>();
    controller.lifetime.shutdown();
    controller.wake.notify_all();
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
pub fn add_runtime_error(app: &AppHandle, snapshot: &mut NotchSnapshot) {
    if let Ok(error) = app.state::<Arc<Controller>>().error.lock() {
        if snapshot.error.is_none() {
            snapshot.error = error.clone();
        }
    }
}

#[cfg(target_os = "windows")]
fn supervise(app: AppHandle, controller: &Controller, transport: impl Transport) {
    let mut host = Host::new(app, transport, initial_snapshot(), Instant::now());
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
        controller.wait(host.enabled().then_some(Duration::from_millis(100)));
    }
}

#[cfg(target_os = "windows")]
fn initial_snapshot() -> NotchSnapshot {
    let revision = super::config::revision();
    match super::config::read() {
        Ok(settings) if settings.enabled => super::read(),
        Ok(settings) => NotchSnapshot {
            revision,
            supported: true,
            liquid_glass: super::liquid_glass(),
            settings,
            displays: Vec::new(),
            error: None,
        },
        Err(error) => NotchSnapshot {
            revision,
            supported: true,
            liquid_glass: super::liquid_glass(),
            settings: Default::default(),
            displays: Vec::new(),
            error: Some(error),
        },
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Changed {
    snapshot: NotchSnapshot,
}

#[cfg(target_os = "windows")]
impl App for AppHandle {
    fn read_limits(&self, agent: AgentId, force: bool) -> (Vec<ProviderLimitsDto>, u64) {
        crate::limits_refresh::read_limits_revisioned(agent, force)
    }

    fn limits_revision(&self, agent: AgentId) -> u64 {
        crate::limits_refresh::revision(agent)
    }

    fn limits_interval(&self) -> Duration {
        crate::limits_refresh::check_interval(crate::limits_refresh::poll_interval())
    }

    fn read_pull_requests(&self, force: bool) -> (GithubPrsDto, u64) {
        crate::github::read_prs_revisioned(force)
    }

    fn pull_requests_revision(&self) -> u64 {
        crate::github::revision()
    }

    fn pull_requests_interval(&self) -> Duration {
        Duration::from_secs(u64::from(
            crate::settings::load_settings().github_poll_seconds,
        ))
    }

    fn read_sessions(&self, agent: AgentId) -> Vec<LiveSession> {
        crate::paths::user_home()
            .map(|home| super::sessions::read(agent, &home, chrono::Utc::now()))
            .unwrap_or_default()
    }

    fn spawn(&self, read: impl FnOnce() + Send + 'static) {
        thread::spawn(read);
    }

    fn open(&self, page: Page) -> Result<(), String> {
        match page {
            Page::Limits => crate::tray::open_limits_window(self),
            Page::PullRequests => crate::tray::open_github_window(self),
        }
    }

    fn settings_revision(&self) -> u64 {
        super::config::revision()
    }

    fn save(&self, settings: NotchSettings) -> Result<NotchSnapshot, String> {
        super::save(settings)
    }

    fn publish(&self, status: &NotchSnapshot) {
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
