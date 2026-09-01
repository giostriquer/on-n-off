use super::{
    model::{layout, GithubList, NotchSnapshot, RAIL_ORDER},
    protocol::{Action, PROTOCOL_VERSION},
    sessions::{self, LiveSession},
    transport::{Connection, Lifetime},
};
use crate::dto::{
    AgentId, CiState, GithubPrDto, GithubPrsDto, GithubStatus, LimitWindowDto, LimitsStatus,
    MergeKind, ProviderLimitsDto, ReviewDecision,
};
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
/// One provider on the wire: its quota snapshot plus the live sessions read on their own cadence.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageProvider<'a> {
    #[serde(flatten)]
    entry: &'a NativeProvider,
    sessions: &'a [LiveSession],
}

/// Rows per list in the popover; the screen shows the rest.
const MAX_PULL_REQUESTS: usize = 25;

/// The pull-request cell's data: only the selected lists, each capped, with the row fields the
/// popover shows. No account identifiers beyond the author logins GitHub already displays.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct NativePullRequests {
    status: GithubStatus,
    hint: Option<String>,
    stale: bool,
    lists: Vec<NativePrList>,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct NativePrList {
    id: GithubList,
    total: u64,
    items: Vec<NativePr>,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct NativePr {
    id: String,
    number: u64,
    title: String,
    url: String,
    repo: String,
    author: String,
    is_draft: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_decision: Option<ReviewDecision>,
    ci: CiState,
    #[serde(skip_serializing_if = "Option::is_none")]
    merge_kind: Option<MergeKind>,
    updated_at: String,
}

impl NativePr {
    fn from_dto(pr: &GithubPrDto) -> Self {
        Self {
            id: pr.id.clone(),
            number: pr.number,
            title: pr.title.clone(),
            url: pr.url.clone(),
            repo: pr.repo.clone(),
            author: pr.author.clone(),
            is_draft: pr.is_draft,
            review_decision: pr.review_decision,
            ci: pr.ci,
            merge_kind: pr.merge_kind,
            updated_at: pr.updated_at.clone(),
        }
    }
}

fn native_pull_requests(dto: &GithubPrsDto, selected: &[GithubList]) -> NativePullRequests {
    let lists = selected
        .iter()
        .map(|list| {
            let source = match list {
                GithubList::Mine => &dto.data.mine,
                GithubList::ReviewRequested => &dto.data.review_requested,
                GithubList::Assigned => &dto.data.assigned,
            };
            NativePrList {
                id: *list,
                total: source.total,
                items: source
                    .items
                    .iter()
                    .take(MAX_PULL_REQUESTS)
                    .map(NativePr::from_dto)
                    .collect(),
            }
        })
        .collect();
    NativePullRequests {
        status: dto.status,
        hint: dto.hint.clone(),
        stale: dto.stale,
        lists,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Message<'a> {
    version: u64,
    sequence: u64,
    snapshot: &'a NotchSnapshot,
    providers: Vec<MessageProvider<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pull_requests: Option<&'a NativePullRequests>,
    action_error: &'a Option<String>,
}

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Live session rows are small local reads; the popover shows "just now" style ages.
const SESSIONS_INTERVAL: Duration = Duration::from_secs(10);
/// A read that never reports back (a hung child, a panicked thread) releases its slot after this.
const READ_DEADLINE: Duration = Duration::from_secs(60);
const PROVIDER_COUNT: usize = RAIL_ORDER.len();

enum Read {
    Limits(usize, Option<NativeProvider>),
    Sessions(Vec<Vec<LiveSession>>),
    /// The pull requests plus the poll interval the app settings asked for at read time.
    PullRequests(NativePullRequests, Duration),
}

/// A value refreshed by a background read: at most one read in flight, refreshed on an
/// interval, forced on request, and never left "loading" forever.
struct Poll<T> {
    value: T,
    loading: bool,
    started: Option<Instant>,
    last_read: Option<Instant>,
    force: bool,
}

impl<T> Poll<T> {
    fn new(value: T) -> Self {
        Self {
            value,
            loading: false,
            started: None,
            last_read: None,
            force: false,
        }
    }

    fn due(&self, now: Instant, interval: Duration) -> bool {
        !self.loading
            && (self.force
                || self
                    .last_read
                    .is_none_or(|last_read| now.saturating_duration_since(last_read) >= interval))
    }

    /// Marks a read in flight and returns whether it was forced.
    fn start(&mut self, now: Instant) -> bool {
        self.loading = true;
        self.started = Some(now);
        std::mem::take(&mut self.force)
    }

    fn finish(&mut self, value: T, now: Instant) {
        self.value = value;
        self.loading = false;
        self.last_read = Some(now);
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

/// The selected providers' current-account entries in rail order, each with its live sessions.
fn message_providers<'a>(
    snapshot: &NotchSnapshot,
    providers: &'a [Poll<Option<NativeProvider>>],
    sessions: &'a [Vec<LiveSession>],
) -> Vec<MessageProvider<'a>> {
    let selected = snapshot.settings.rail_providers();
    RAIL_ORDER
        .iter()
        .enumerate()
        .filter(|(_, agent)| selected.contains(agent))
        .filter_map(|(index, _)| {
            providers[index]
                .value
                .as_ref()
                .map(|entry| MessageProvider {
                    entry,
                    sessions: &sessions[index],
                })
        })
        .collect()
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

fn supervise(app: AppHandle, controller: Arc<Controller>) {
    let (sender, reads) = mpsc::sync_channel::<Read>(PROVIDER_COUNT + 1);
    let mut snapshot = initial_snapshot();
    let mut connection: Option<Connection> = None;
    let mut connected_at = Instant::now();
    let mut providers: [Poll<Option<NativeProvider>>; PROVIDER_COUNT] =
        [(); PROVIDER_COUNT].map(|_| Poll::new(None));
    let mut sessions: Poll<Vec<Vec<LiveSession>>> = Poll::new(vec![Vec::new(); PROVIDER_COUNT]);
    let mut pulls: Poll<Option<NativePullRequests>> = Poll::new(None);
    let mut pulls_interval = Duration::from_secs(60);
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
        for read in reads.try_iter() {
            match read {
                Read::Limits(index, entry) => {
                    providers[index].finish(entry, Instant::now());
                    delivery.mark_dirty();
                }
                Read::Sessions(latest) => {
                    if latest != sessions.value {
                        delivery.mark_dirty();
                    }
                    sessions.finish(latest, Instant::now());
                }
                Read::PullRequests(latest, interval) => {
                    if pulls.value.as_ref() != Some(&latest) {
                        delivery.mark_dirty();
                    }
                    pulls.finish(Some(latest), Instant::now());
                    pulls_interval = interval;
                }
            }
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
                        for poll in &mut providers {
                            poll.last_read = None;
                        }
                    }
                    Ok(Ok(Action::Refresh)) => {
                        for poll in &mut providers {
                            poll.force = true;
                        }
                        sessions.force = true;
                        pulls.force = true;
                    }
                    Ok(Ok(Action::OpenLimits)) => {
                        action_error = crate::tray::open_limits_window(&app).err();
                        delivery.mark_dirty();
                    }
                    Ok(Ok(Action::OpenPullRequests)) => {
                        action_error = crate::tray::open_github_window(&app).err();
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
                    version: PROTOCOL_VERSION,
                    sequence,
                    snapshot: &snapshot,
                    providers: message_providers(&snapshot, &providers, &sessions.value),
                    pull_requests: snapshot
                        .settings
                        .pull_requests
                        .enabled
                        .then_some(pulls.value.as_ref())
                        .flatten(),
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
        let visible =
            snapshot.error.is_none() && layout(&snapshot.settings, &snapshot.displays).is_some();
        if connection.is_some() && visible {
            let now = Instant::now();
            let selected = snapshot.settings.rail_providers();
            for poll in &mut providers {
                poll.release_stale(now);
            }
            sessions.release_stale(now);
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
            if sessions.due(now, SESSIONS_INTERVAL) {
                sessions.start(now);
                let sender = sender.clone();
                let selected = selected.clone();
                thread::spawn(move || {
                    let _ = sender.send(Read::Sessions(read_sessions(&selected)));
                });
            }
            pulls.release_stale(now);
            if snapshot.settings.pull_requests.enabled && pulls.due(now, pulls_interval) {
                let force = pulls.start(now);
                let lists = snapshot.settings.pull_requests.selected_lists();
                let sender = sender.clone();
                thread::spawn(move || {
                    // `read_prs` memoises within the screen's poll window, so this adds no
                    // GitHub calls beyond what the screen and monitor already make.
                    let dto = crate::github::read_prs(force);
                    let interval = Duration::from_secs(u64::from(
                        crate::settings::load_settings().github_poll_seconds,
                    ));
                    let _ = sender.send(Read::PullRequests(
                        native_pull_requests(&dto, &lists),
                        interval,
                    ));
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
    use crate::side_notch::model::NotchSettings;
    use crate::side_notch::sessions::SessionStatus;

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
        let payload = serde_json::to_value(MessageProvider {
            entry: &entry,
            sessions: &[],
        })
        .unwrap();
        assert!(payload.get("account").is_none());
        assert!(payload.get("credits").is_none());
        assert_eq!(payload["provider"], "claude");
        assert_eq!(payload["sessions"], serde_json::json!([]));
        assert!(current_provider(Vec::new()).is_none());
    }

    #[test]
    fn the_message_lists_selected_providers_in_rail_order_with_their_sessions() {
        let entry = |provider: AgentId| NativeProvider {
            provider,
            status: LimitsStatus::Ok,
            current_account: true,
            plan: None,
            message: None,
            windows: Vec::new(),
        };
        let providers = [
            Poll::new(Some(entry(AgentId::Claude))),
            Poll::new(Some(entry(AgentId::Codex))),
            Poll::new(None),
            Poll::new(Some(entry(AgentId::Cursor))),
        ];
        let session = LiveSession {
            id: "s".into(),
            name: "repo-1a".into(),
            place: "Terminal".into(),
            project: "repo".into(),
            status: SessionStatus::Working,
            last_active_at: "2026-09-01T10:00:00Z".into(),
        };
        let sessions = vec![vec![session.clone()], vec![], vec![], vec![]];
        let snapshot = NotchSnapshot {
            revision: 0,
            supported: true,
            settings: NotchSettings {
                providers: vec![AgentId::Cursor, AgentId::Claude, AgentId::Antigravity],
                ..NotchSettings::default()
            },
            displays: Vec::new(),
            error: None,
        };
        let listed = message_providers(&snapshot, &providers, &sessions);
        assert_eq!(
            listed
                .iter()
                .map(|entry| entry.entry.provider)
                .collect::<Vec<_>>(),
            [AgentId::Claude, AgentId::Cursor],
            "Codex is deselected and Antigravity has not been read yet"
        );
        assert_eq!(listed[0].sessions, [session]);
        assert!(listed[1].sessions.is_empty());
    }

    #[test]
    fn pull_requests_keep_only_the_selected_lists_capped_with_row_fields() {
        let pr = |number: u64| {
            serde_json::json!({
                "id": format!("node-{number}"), "number": number, "title": format!("Fix #{number}"),
                "url": format!("https://github.com/octo/tools/pull/{number}"), "repo": "octo/tools",
                "author": "giovanne", "isDraft": false, "reviewDecision": "APPROVED", "ci": "success",
                "headRef": "fix", "baseRef": "main", "updatedAt": "2026-09-01T10:00:00Z",
                "mergeKind": "ready"
            })
        };
        let many: Vec<_> = (1..=30).map(pr).collect();
        let dto: GithubPrsDto = serde_json::from_value(serde_json::json!({
            "status": "ok", "stale": false, "scope": ["org:octo"],
            "mine": {"total": 30, "items": many},
            "reviewRequested": {"total": 1, "items": [pr(99)]},
            "assigned": {"total": 0, "items": []}
        }))
        .unwrap();
        let native = native_pull_requests(&dto, &[GithubList::Mine, GithubList::Assigned]);
        assert_eq!(native.lists.len(), 2, "review requests were not selected");
        assert_eq!(native.lists[0].id, GithubList::Mine);
        assert_eq!(native.lists[0].total, 30);
        assert_eq!(native.lists[0].items.len(), MAX_PULL_REQUESTS);
        assert_eq!(native.lists[1].items.len(), 0);
        let json = serde_json::to_value(&native).unwrap();
        assert_eq!(json["lists"][0]["id"], "mine");
        assert_eq!(json["lists"][1]["id"], "assigned");
        let row = &json["lists"][0]["items"][0];
        assert_eq!(row["title"], "Fix #1");
        assert_eq!(row["reviewDecision"], "APPROVED");
        assert_eq!(row["mergeKind"], "ready");
        assert_eq!(row["ci"], "success");
        assert!(
            row.get("headRef").is_none(),
            "branch names are not needed in the notch"
        );
    }

    #[test]
    fn a_poll_refreshes_on_its_interval_forces_once_and_never_stays_loading_forever() {
        let now = Instant::now();
        let interval = Duration::from_secs(5 * 60);
        let mut poll = Poll::new(0u8);
        assert!(poll.due(now, interval), "never read yet");
        assert!(!poll.start(now));
        assert!(!poll.due(now, interval), "one read in flight");
        poll.finish(1, now);
        assert!(!poll.due(now + Duration::from_secs(299), interval));
        assert!(poll.due(now + interval, interval));
        poll.force = true;
        assert!(poll.due(now + Duration::from_secs(1), interval));
        assert!(poll.start(now), "the forced read reports force once");
        assert!(!poll.force);
        poll.release_stale(now + READ_DEADLINE);
        assert!(poll.loading, "still inside the deadline");
        poll.release_stale(now + READ_DEADLINE + Duration::from_secs(1));
        assert!(
            !poll.loading,
            "a read that never reported back releases its slot"
        );
        assert_eq!(poll.value, 1);
    }
}
