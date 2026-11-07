use super::*;
use crate::dto::{
    GithubPrsDto, LimitWindowDto, LimitWindowKind, LimitsStatus, LimitsWorkspaceCreditsDto,
    ProviderLimitsDto, Reading,
};
use crate::side_notch::model::frame::{self, ProviderCell};
use crate::side_notch::model::{NotchProvider, NotchPullRequests, NotchSettings};
use crate::side_notch::sessions::LiveSession;

mod chrome;
mod content;
mod geometry;
mod golden;
mod layout;
mod spacing;
mod text;

fn plan(displays: &[Display], data: &RailData, hover: Hover, now: DateTime<Utc>) -> Option<Plan> {
    super::plan(displays, data, hover, now, &FixedAdvance)
}

pub fn open_on(cell: CellId) -> Hover {
    Hover {
        active: Some(cell),
        ..Hover::default()
    }
}
pub const CLAUDE: CellId = CellId::Provider(AgentId::Claude);

pub fn now() -> DateTime<Utc> {
    "2026-09-03T10:30:00Z".parse().unwrap()
}

pub fn display(id: &str, x: f64, y: f64, width: f64, height: f64, scale: f64) -> Display {
    Display {
        id: id.into(),
        name: "Test Display".into(),
        x,
        y,
        width,
        height,
        work_y: y,
        work_height: height,
        scale,
        mirrored: false,
    }
}
fn signed_in(provider: AgentId, windows: Vec<LimitWindowDto>) -> ProviderLimitsDto {
    crate::limits::signed_in_card(
        provider,
        "acct",
        Reading {
            windows,
            ..Reading::default()
        },
    )
}
fn decided(
    settings: NotchSettings,
    reads: Vec<(AgentId, Option<NotchProvider>, Vec<LiveSession>)>,
    pulls: Option<&GithubPrsDto>,
) -> CellData {
    frame::rail(&settings, reads, pulls, None)
        .cells
        .pop()
        .expect("the model decided a cell")
}
fn projected(card: ProviderLimitsDto) -> ProviderCell {
    let provider = card.provider;
    let reading = NotchProvider::current(vec![card]).expect("a signed-in account");
    let only = NotchSettings {
        providers: vec![provider],
        pull_requests: NotchPullRequests {
            enabled: false,
            ..NotchPullRequests::default()
        },
        ..settings()
    };
    match decided(only, vec![(provider, Some(reading), Vec::new())], None) {
        CellData::Provider(cell) => cell,
        CellData::PullRequests(_) => panic!("a provider cell"),
    }
}
pub fn pr(number: u64, title: &str, repo: &str) -> serde_json::Value {
    serde_json::json!({
        "id": format!("n{number}"), "number": number, "title": title,
        "url": format!("https://github.com/{repo}/pull/{number}"), "repo": repo,
        "author": "octocat", "isDraft": false, "ci": "success", "headRef": "fix",
        "baseRef": "main", "updatedAt": "2026-09-01T10:00:00Z"
    })
}
fn pulls_read(
    status: &str,
    hint: Option<&str>,
    total: usize,
    rows: Vec<serde_json::Value>,
) -> CellData {
    let dto: GithubPrsDto = serde_json::from_value(serde_json::json!({
        "status": status, "hint": hint, "stale": false, "scope": [],
        "mine": {"total": total, "items": rows},
        "reviewRequested": {"total": 0, "items": []},
        "assigned": {"total": 0, "items": []}
    }))
    .expect("a pull request read");
    let only = NotchSettings {
        providers: Vec::new(),
        ..settings()
    };
    decided(only, Vec::new(), Some(&dto))
}
pub fn pulls(rows: Vec<serde_json::Value>) -> CellData {
    let total = rows.len();
    pulls_read("ok", None, total, rows)
}
fn session_window(percent: f64) -> LimitWindowDto {
    LimitWindowDto {
        id: "w".into(),
        label: "Current session".into(),
        kind: LimitWindowKind::Session,
        used_percent: percent,
        resets_at: None,
        window_seconds: None,
        observed_at: "2026-09-01T10:00:00Z".into(),
        role: None,
        starts_on_first_message: false,
    }
}
pub fn session_only(provider: AgentId, percent: f64) -> ProviderCell {
    projected(signed_in(provider, vec![session_window(percent)]))
}
pub fn settings() -> NotchSettings {
    NotchSettings {
        enabled: true,
        display_id: Some("d1".into()),
        ..NotchSettings::default()
    }
}
pub fn data(cells: Vec<CellData>) -> RailData {
    data_for(settings(), cells)
}
pub fn data_for(settings: NotchSettings, cells: Vec<CellData>) -> RailData {
    RailData {
        settings,
        cells,
        action_error: None,
    }
}
pub fn window(id: &str, label: &str, kind: LimitWindowKind, percent: f64) -> LimitWindowDto {
    LimitWindowDto {
        id: id.into(),
        label: label.into(),
        kind,
        used_percent: percent,
        resets_at: Some("2099-01-01T00:00:00Z".into()),
        window_seconds: None,
        observed_at: "2026-09-01T10:00:00Z".into(),
        role: None,
        starts_on_first_message: false,
    }
}
pub fn claude_with(windows: Vec<LimitWindowDto>) -> ProviderCell {
    projected(signed_in(AgentId::Claude, windows))
}
fn codex_member_card(share: LimitsWorkspaceCreditsDto) -> ProviderLimitsDto {
    crate::limits::signed_in_card(
        AgentId::Codex,
        "acct",
        Reading {
            windows: vec![window(
                "primary",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                31.0,
            )],
            workspace_credits: Some(share),
            ..Reading::default()
        },
    )
}
fn codex_member(share: LimitsWorkspaceCreditsDto) -> ProviderCell {
    projected(codex_member_card(share))
}
fn popover_plan(provider: ProviderCell) -> Plan {
    let opened = open_on(CellId::Provider(provider.provider));
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    plan(
        &displays,
        &data(vec![CellData::Provider(provider)]),
        opened,
        now(),
    )
    .expect("fits")
}
pub const WORK: Color = [96, 165, 250, 255];
pub const PERSONAL: Color = [170, 155, 215, 255];

fn pr_row(number: u64, repo: &str, in_organization: Option<bool>) -> serde_json::Value {
    let mut row = pr(number, &format!("Fix #{number}"), repo);
    row["inOrganization"] = serde_json::to_value(in_organization).unwrap();
    row
}
pub fn tagged_rows() -> CellData {
    let mut approved = pr_row(1, "acme/webapp", Some(true));
    approved["reviewDecision"] = "APPROVED".into();
    pulls(vec![
        approved,
        pr_row(2, "octocat/dotfiles", Some(false)),
        pr_row(3, "acme/webapp", None),
        pr_row(
            1234,
            "octocat/a-repository-name-long-enough-to-run-into-the-ci-dot",
            Some(false),
        ),
    ])
}
fn measure_weight(text: &str, size: f64, weight: TextWeight, scale: f64) -> f64 {
    super::text::Typesetter::new(&FixedAdvance, scale).width(text, size, weight)
}
