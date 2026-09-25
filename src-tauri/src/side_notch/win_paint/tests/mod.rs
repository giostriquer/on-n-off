use super::*;
use crate::dto::{LimitWindowKind, LimitsWorkspaceCreditsDto, ProviderLimitsDto, Reading};
use crate::side_notch::model::{Display, NotchSettings};

mod content;
mod layout;
mod render;

fn display(id: &str, x: f64, y: f64, width: f64, height: f64, scale: f64) -> Display {
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
/// The signed-in account's card for `provider` when a read reports `windows`, which the pipeline
/// puts in the order every card lists them: the painter is never handed an order no read produces.
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
/// What the painter is handed for `card`: the host's projection of it, without live sessions.
fn projected(card: ProviderLimitsDto) -> ProviderData {
    ProviderData {
        cell: NotchProvider::current(vec![card]).expect("a signed-in account"),
        sessions: Vec::new(),
    }
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
    }
}
/// A cell whose account reports only its session, `percent` used: the popover lists it, and with no
/// weekly window the ring leads with nothing and the figure is a dash.
fn session_only(provider: AgentId, percent: f64) -> ProviderData {
    projected(signed_in(provider, vec![session_window(percent)]))
}
fn settings() -> NotchSettings {
    NotchSettings {
        enabled: true,
        display_id: Some("d1".into()),
        ..NotchSettings::default()
    }
}
fn data(cells: Vec<CellData>) -> RailData {
    RailData {
        settings: settings(),
        cells,
        action_error: None,
    }
}
/// A window that resets in the future, so `quota_percent` keeps its figure.
fn window(id: &str, label: &str, kind: LimitWindowKind, percent: f64) -> LimitWindowDto {
    LimitWindowDto {
        id: id.into(),
        label: label.into(),
        kind,
        used_percent: percent,
        resets_at: Some("2099-01-01T00:00:00Z".into()),
        window_seconds: None,
        observed_at: "2026-09-01T10:00:00Z".into(),
    }
}
fn claude_with(windows: Vec<LimitWindowDto>) -> ProviderData {
    projected(signed_in(AgentId::Claude, windows))
}
/// A Codex business member's card: the weekly window, which is all Codex reports, and a credit share.
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
fn codex_member(share: LimitsWorkspaceCreditsDto) -> ProviderData {
    projected(codex_member_card(share))
}
/// The plan and its rendering for one open provider popover.
fn popover_render(provider: ProviderData) -> (Plan, tiny_skia::Pixmap) {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![CellData::Provider(provider)]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
    )
    .expect("fits");
    let pixmap = render(&planned);
    (planned, pixmap)
}
/// How much of one column the bar covers, read from the red channel because the
/// popover card behind it is already opaque. A tapering shape reads thinner than a
/// straight-sided one even where anti-aliasing lights the same rows.
fn column_ink(pixmap: &tiny_skia::Pixmap, x: u32, y0: u32, y1: u32) -> u32 {
    (y0..y1)
        .map(|y| u32::from(pixmap.pixel(x, y).map_or(0, |px| px.red())))
        .sum()
}
fn row_ink(pixmap: &tiny_skia::Pixmap, y: u32, x0: u32, x1: u32) -> u32 {
    (x0..x1)
        .map(|x| u32::from(pixmap.pixel(x, y).map_or(0, |px| px.alpha())))
        .sum()
}
