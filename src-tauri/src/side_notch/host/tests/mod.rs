use super::*;
use crate::dto::{LimitWindowDto, LimitWindowKind, Reading};
use crate::side_notch::model::frame::{CellData, PrCell};
use crate::side_notch::model::Display;
use crate::side_notch::sessions::SessionStatus;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

mod frame;
mod link;
mod polls;
mod wire;

type Job = Box<dyn FnOnce() + Send>;

const READ_ALL: [Asked; 3] = [
    Asked::Limits(AgentId::Claude, false),
    Asked::Limits(AgentId::Codex, false),
    Asked::Limits(AgentId::Muse, false),
];

#[derive(Clone, Debug, PartialEq)]
enum Asked {
    Limits(AgentId, bool),
    PullRequests(bool),
    Sessions(AgentId),
    Open(Page),
}

#[derive(Default)]
struct World {
    limits: HashMap<AgentId, Vec<ProviderLimitsDto>>,
    limits_revisions: HashMap<AgentId, u64>,
    pull_requests: Option<GithubPrsDto>,
    pull_requests_revision: u64,
    sessions: HashMap<AgentId, Vec<LiveSession>>,
    limits_interval: Option<Duration>,
    pull_requests_interval: Option<Duration>,
    hold: bool,
    held: Vec<Job>,
    asked: Vec<Asked>,
    open_error: Option<String>,
    save_error: Option<String>,
    reread: Option<NotchSnapshot>,
    saved: Vec<NotchSettings>,
    published: Vec<NotchSnapshot>,
    settings_revision: u64,
}

#[derive(Clone, Default)]
struct FakeApp(Arc<Mutex<World>>);

impl FakeApp {
    fn world(&self) -> MutexGuard<'_, World> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn release(&self) {
        let held = std::mem::take(&mut self.world().held);
        for read in held {
            read();
        }
    }

    fn take_asked(&self) -> Vec<Asked> {
        std::mem::take(&mut self.world().asked)
    }

    fn limits_asked(&self) -> Vec<Asked> {
        self.take_asked()
            .into_iter()
            .filter(|asked| matches!(asked, Asked::Limits(..)))
            .collect()
    }
}

impl App for FakeApp {
    fn read_limits(&self, agent: AgentId, force: bool) -> (Vec<ProviderLimitsDto>, u64) {
        let mut world = self.world();
        world.asked.push(Asked::Limits(agent, force));
        let entries = world.limits.get(&agent).cloned().unwrap_or_default();
        (
            entries,
            world.limits_revisions.get(&agent).copied().unwrap_or(0),
        )
    }

    fn limits_revision(&self, agent: AgentId) -> u64 {
        self.world()
            .limits_revisions
            .get(&agent)
            .copied()
            .unwrap_or(0)
    }

    fn limits_interval(&self) -> Duration {
        self.world()
            .limits_interval
            .unwrap_or(Duration::from_secs(5 * 60))
    }

    fn read_pull_requests(&self, force: bool) -> (GithubPrsDto, u64) {
        let mut world = self.world();
        world.asked.push(Asked::PullRequests(force));
        let dto = world
            .pull_requests
            .clone()
            .unwrap_or_else(|| prs(Vec::new()));
        (dto, world.pull_requests_revision)
    }

    fn pull_requests_revision(&self) -> u64 {
        self.world().pull_requests_revision
    }

    fn pull_requests_interval(&self) -> Duration {
        self.world()
            .pull_requests_interval
            .unwrap_or(Duration::from_secs(60))
    }

    fn read_sessions(&self, agent: AgentId) -> Vec<LiveSession> {
        let mut world = self.world();
        world.asked.push(Asked::Sessions(agent));
        world.sessions.get(&agent).cloned().unwrap_or_default()
    }

    fn spawn(&self, read: impl FnOnce() + Send + 'static) {
        let hold = self.world().hold;
        if hold {
            self.world().held.push(Box::new(read));
        } else {
            read();
        }
    }

    fn open(&self, page: Page) -> Result<(), String> {
        let mut world = self.world();
        world.asked.push(Asked::Open(page));
        world.open_error.clone().map_or(Ok(()), Err)
    }

    fn settings_revision(&self) -> u64 {
        self.world().settings_revision
    }

    fn reread(&self) -> NotchSnapshot {
        self.world().reread.clone().unwrap_or_else(snapshot)
    }

    fn save(&self, settings: NotchSettings) -> Result<NotchSnapshot, String> {
        let mut world = self.world();
        world.saved.push(settings.clone());
        if let Some(error) = world.save_error.clone() {
            return Err(error);
        }
        world.settings_revision += 1;
        Ok(NotchSnapshot {
            revision: world.settings_revision,
            settings,
            ..snapshot()
        })
    }

    fn publish(&self, status: &NotchSnapshot) {
        self.world().published.push(status.clone());
    }
}

#[derive(Default)]
struct Wire {
    up: bool,
    opened: usize,
    closed: usize,
    waiting: bool,
    unready: bool,
    settings_changed: usize,
    frames: Vec<RailData>,
    snapshots: Vec<NotchSnapshot>,
    inbound: Vec<Inbound>,
    refuse: Option<String>,
    fail_receive: Option<String>,
    fail_send: Option<String>,
}

#[derive(Clone, Default)]
struct FakeTransport(Arc<Mutex<Wire>>);

impl FakeTransport {
    fn wire(&self) -> MutexGuard<'_, Wire> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }
}

impl Transport for FakeTransport {
    fn connect(&mut self, _: Instant) -> Result<Link, String> {
        let mut wire = self.wire();
        if wire.up {
            return Ok(Link::Open);
        }
        if wire.waiting {
            return Ok(Link::Closed);
        }
        if let Some(error) = wire.refuse.clone() {
            return Err(error);
        }
        wire.up = true;
        wire.opened += 1;
        Ok(Link::Opened)
    }

    fn receive(&mut self, _: Instant, inbound: &mut Vec<Inbound>) -> Result<(), String> {
        let mut wire = self.wire();
        inbound.append(&mut wire.inbound);
        if let Some(error) = wire.fail_receive.take() {
            wire.up = false;
            return Err(error);
        }
        Ok(())
    }

    fn ready(&self) -> bool {
        !self.wire().unready
    }

    fn send(
        &mut self,
        _: Instant,
        snapshot: &NotchSnapshot,
        frame: RailData,
    ) -> Result<(), String> {
        let mut wire = self.wire();
        if let Some(error) = wire.fail_send.take() {
            wire.up = false;
            return Err(error);
        }
        wire.snapshots.push(snapshot.clone());
        wire.frames.push(frame);
        Ok(())
    }

    fn close(&mut self) {
        let mut wire = self.wire();
        if wire.up {
            wire.closed += 1;
        }
        wire.up = false;
    }

    fn settings_changed(&mut self) {
        self.wire().settings_changed += 1;
    }
}

struct Rig {
    app: FakeApp,
    wire: FakeTransport,
    host: Host<FakeTransport, FakeApp>,
    now: Instant,
}

impl Rig {
    fn new(app: FakeApp, snapshot: NotchSnapshot) -> Self {
        let wire = FakeTransport::default();
        let now = Instant::now();
        let host = Host::new(app.clone(), wire.clone(), snapshot, now);
        Self {
            app,
            wire,
            host,
            now,
        }
    }

    fn started(app: FakeApp) -> Self {
        let mut rig = Self::new(app, snapshot());
        rig.tick(Duration::ZERO);
        rig.tick(Duration::from_millis(100));
        rig
    }

    fn tick(&mut self, after: Duration) {
        self.now += after;
        self.host.tick(self.now);
        self.host.publish();
    }

    fn act(&mut self, item: Inbound) {
        self.wire.wire().inbound.push(item);
        self.tick(Duration::from_millis(100));
    }

    fn frames(&self) -> usize {
        self.wire.wire().frames.len()
    }

    fn last_frame(&self) -> RailData {
        self.wire
            .wire()
            .frames
            .last()
            .cloned()
            .expect("a frame was sent")
    }

    fn published(&self) -> Vec<NotchSnapshot> {
        self.app.world().published.clone()
    }
}

fn display() -> Display {
    Display {
        id: "display-a".into(),
        name: "Built-in Display".into(),
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
        work_y: 33.0,
        work_height: 949.0,
        scale: 2.0,
        mirrored: false,
    }
}

fn snapshot() -> NotchSnapshot {
    NotchSnapshot {
        revision: 0,
        supported: true,
        liquid_glass: false,
        settings: NotchSettings {
            enabled: true,
            display_id: Some("display-a".into()),
            ..NotchSettings::default()
        },
        displays: vec![display()],
        error: None,
    }
}

fn card(provider: AgentId, percent: f64) -> Vec<ProviderLimitsDto> {
    vec![crate::limits::signed_in_card(
        provider,
        "acct",
        Reading {
            windows: vec![LimitWindowDto {
                id: "weekly".into(),
                label: "Weekly · all models".into(),
                kind: LimitWindowKind::Weekly,
                used_percent: percent,
                resets_at: None,
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
            }],
            ..Reading::default()
        },
    )]
}

fn session(id: &str) -> LiveSession {
    LiveSession {
        id: id.into(),
        name: format!("webapp-{id}"),
        place: "Terminal".into(),
        project: "webapp".into(),
        status: SessionStatus::Working,
        last_active_at: "2026-09-01T10:00:00Z".into(),
    }
}

fn pr(number: u64) -> serde_json::Value {
    serde_json::json!({
        "id": format!("node-{number}"), "number": number, "title": format!("Fix #{number}"),
        "url": format!("https://github.com/acme/webapp/pull/{number}"), "repo": "acme/webapp",
        "author": "octocat", "isDraft": false, "ci": "success", "headRef": "fix",
        "baseRef": "main", "updatedAt": "2026-09-01T10:00:00Z"
    })
}

fn prs(mine: Vec<serde_json::Value>) -> GithubPrsDto {
    serde_json::from_value(serde_json::json!({
        "status": "ok", "stale": false, "scope": [],
        "mine": {"total": mine.len(), "items": mine},
        "reviewRequested": {"total": 0, "items": []},
        "assigned": {"total": 0, "items": []}
    }))
    .unwrap()
}

fn providers(frame: &RailData) -> Vec<(AgentId, f64)> {
    frame
        .cells
        .iter()
        .filter_map(|cell| match cell {
            CellData::Provider(provider) => provider
                .reading
                .as_ref()
                .map(|reading| (provider.provider, reading.windows[0].used_percent)),
            CellData::PullRequests(_) => None,
        })
        .collect()
}

fn replace_limits(app: &FakeApp, agent: AgentId, percent: f64) {
    let mut world = app.world();
    world.limits.insert(agent, card(agent, percent));
    *world.limits_revisions.entry(agent).or_default() += 1;
}

fn replace_pull_requests(app: &FakeApp, mine: Vec<serde_json::Value>) {
    let mut world = app.world();
    world.pull_requests = Some(prs(mine));
    world.pull_requests_revision += 1;
}

fn app_with(cards: &[(AgentId, f64)]) -> FakeApp {
    let app = FakeApp::default();
    for (agent, percent) in cards {
        app.world().limits.insert(*agent, card(*agent, *percent));
    }
    app
}
