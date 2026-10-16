use super::model::frame::{self, CellData, ProviderData, RailData};
use super::model::{layout, NotchProvider, NotchSettings, NotchSnapshot, ShowMode, RAIL_ORDER};
use super::sessions::LiveSession;
use crate::dto::{AgentId, GithubPrsDto, ProviderLimitsDto};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod app;

pub(super) const STOPPED: &str = "Native notch stopped unexpectedly. Retrying…";

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const SESSIONS_INTERVAL: Duration = Duration::from_secs(10);
const READ_DEADLINE: Duration = Duration::from_secs(60);
const PROVIDER_COUNT: usize = RAIL_ORDER.len();
const READ_SLOTS: usize = PROVIDER_COUNT + 2;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Action {
    Refresh,
    OpenLimits,
    OpenPullRequests,
    SetShow(ShowMode),
    ScreensChanged,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Inbound {
    Action(Action),
    #[cfg(any(target_os = "windows", test))]
    Handled(Result<(), String>),
    Confirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Link {
    Opened,
    Open,
    #[cfg(any(target_os = "macos", test))]
    Closed,
}

pub(super) trait Transport {
    fn connect(&mut self, now: Instant) -> Result<Link, String>;
    fn receive(&mut self, now: Instant, inbound: &mut Vec<Inbound>) -> Result<(), String>;
    fn ready(&self) -> bool;
    fn send(
        &mut self,
        now: Instant,
        snapshot: &NotchSnapshot,
        frame: RailData,
    ) -> Result<(), String>;
    fn close(&mut self);
    fn settings_changed(&mut self);
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
    fn reread(&self) -> NotchSnapshot;
    fn save(&self, settings: NotchSettings) -> Result<NotchSnapshot, String>;
    fn publish(&self, status: &NotchSnapshot);
}

struct Poll<T> {
    value: T,
    reading_since: Option<Instant>,
    last_read: Option<Instant>,
    force: bool,
    read_at: u64,
}

impl<T: PartialEq> Poll<T> {
    fn new(value: T) -> Self {
        Self {
            value,
            reading_since: None,
            last_read: None,
            force: false,
            read_at: 0,
        }
    }

    fn due(&self, now: Instant, interval: Duration, latest: u64) -> bool {
        let reading = self
            .reading_since
            .is_some_and(|started| now.saturating_duration_since(started) <= READ_DEADLINE);
        !reading
            && (self.force
                || self.read_at != latest
                || self
                    .last_read
                    .is_none_or(|last_read| now.saturating_duration_since(last_read) >= interval))
    }

    fn start(&mut self, now: Instant) -> bool {
        self.reading_since = Some(now);
        std::mem::take(&mut self.force)
    }

    fn finish(&mut self, value: T, now: Instant, read_at: u64) -> bool {
        let changed = self.value != value;
        self.value = value;
        self.reading_since = None;
        self.last_read = Some(now);
        self.read_at = read_at;
        changed
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
        if self.current(&next) {
            self.snapshot = next;
            self.transport.settings_changed();
            self.delivery.mark_dirty();
        }
    }

    pub fn tick(&mut self, now: Instant) {
        self.collect(now);
        if self.exchange(now) && self.visible() {
            self.start_reads(now);
        }
    }

    fn collect(&mut self, now: Instant) {
        for read in self.reads.try_iter() {
            let changed = match read {
                Read::Limits {
                    index,
                    revision,
                    entry,
                } => self.providers[index].finish(entry, now, revision),
                Read::Sessions(latest) => self.sessions.finish(latest, now, 0),
                Read::PullRequests { revision, dto } => {
                    self.pulls.finish(Some(*dto), now, revision)
                }
            };
            if changed {
                self.delivery.mark_dirty();
            }
        }
    }

    fn exchange(&mut self, now: Instant) -> bool {
        if !self.snapshot.settings.enabled {
            self.transport.close();
            self.runtime_error = None;
            return false;
        }
        match self.transport.connect(now) {
            Ok(Link::Opened) => {
                self.delivery.mark_dirty();
                self.action_error = None;
            }
            Ok(Link::Open) => {}
            #[cfg(any(target_os = "macos", test))]
            Ok(Link::Closed) => return false,
            Err(error) => {
                self.runtime_error = Some(error);
                return false;
            }
        }
        let mut inbound = Vec::new();
        let received = self.transport.receive(now, &mut inbound);
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
            Inbound::Action(Action::ScreensChanged) => {
                let next = self.app.reread();
                if self.current(&next) {
                    self.snapshot = next;
                    self.delivery.mark_dirty();
                }
                for poll in &mut self.providers {
                    poll.last_read = None;
                }
            }
            #[cfg(any(target_os = "windows", test))]
            Inbound::Handled(result) => {
                self.action_error = result.err();
                self.delivery.mark_dirty();
            }
            Inbound::Confirmed => self.runtime_error = None,
        }
    }

    fn deliver(&mut self, now: Instant) -> Result<(), String> {
        if !self.delivery.due(now) || !self.transport.ready() {
            return Ok(());
        }
        let frame = self.rail();
        self.transport.send(now, &self.snapshot, frame)?;
        self.delivery.sent(now);
        Ok(())
    }

    fn visible(&self) -> bool {
        self.snapshot.error.is_none()
            && layout(&self.snapshot.settings, &self.snapshot.displays).is_some()
    }

    fn start_reads(&mut self, now: Instant) {
        let selected = self.snapshot.settings.rail_providers();
        let interval = self.app.limits_interval();
        for (index, agent) in RAIL_ORDER.into_iter().enumerate() {
            if !selected.contains(&agent)
                || !self.providers[index].due(now, interval, self.app.limits_revision(agent))
            {
                continue;
            }
            let force = self.providers[index].start(now);
            self.spawn_read(move |app| {
                let (entries, revision) = app.read_limits(agent, force);
                Read::Limits {
                    index,
                    revision,
                    entry: NotchProvider::current(entries),
                }
            });
        }
        if self.sessions.due(now, SESSIONS_INTERVAL, 0) {
            self.sessions.start(now);
            self.spawn_read(move |app| Read::Sessions(read_sessions(app, &selected)));
        }
        if self.snapshot.settings.pull_requests.enabled
            && self.pulls.due(
                now,
                self.app.pull_requests_interval(),
                self.app.pull_requests_revision(),
            )
        {
            let force = self.pulls.start(now);
            self.spawn_read(move |app| {
                let (dto, revision) = app.read_pull_requests(force);
                Read::PullRequests {
                    revision,
                    dto: Box::new(dto),
                }
            });
        }
    }

    fn spawn_read(&self, read: impl FnOnce(&A) -> Read + Send + 'static) {
        let (app, sender) = (self.app.clone(), self.sender.clone());
        self.app.spawn(move || {
            let _ = sender.send(read(&app));
        });
    }

    fn current(&self, snapshot: &NotchSnapshot) -> bool {
        snapshot.revision == self.app.settings_revision()
    }

    pub fn publish(&mut self) {
        let mut status = self.snapshot.clone();
        if status.error.is_none() {
            status.error = self.runtime_error.clone();
        }
        if self.last_emitted.as_ref() != Some(&status) && self.current(&status) {
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
                cells.push(CellData::PullRequests(frame::pr_cell(
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) trait Lifetime: Send + Sync {
    fn stopped(&self) -> bool;
    fn shutdown(&self);
}

#[cfg(test)]
mod tests;
