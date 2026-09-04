//! Plan + paint for the Windows side notch: geometry ported from `NotchCore`
//! (mirroring `side_notch::model`), rasterized with `tiny-skia` into a
//! premultiplied-BGRA pixmap. The geometry is pure, but the module is not: `text`
//! shapes through DirectWrite, so a popover's own size depends on the fonts installed
//! on the machine that draws it.
// Rendering helpers pass device coordinates around; folding them into structs would
// obscure the math this module exists to keep transparent.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod marks;
mod text;

use super::model::{layout, Edge, GithubList, NotchSettings, NotchSize, ShowMode};
use crate::dto::{
    AgentId, CiState, GithubStatus, LimitWindowDto, LimitWindowKind, LimitsStatus, MergeKind,
    ReviewDecision,
};
use crate::side_notch::sessions::LiveSession;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

// ---------------------------------------------------------------------------
// Host data (what macOS ships over the pipe; Windows passes it in memory).

/// The host's snapshot for one moment, cells already in rail order.
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
    pub provider: AgentId,
    pub status: LimitsStatus,
    pub message: Option<String>,
    pub windows: Vec<LimitWindowDto>,
    pub sessions: Vec<LiveSession>,
}

/// The pull-request cell's data: only the selected lists, each capped, with the row
/// fields the popover shows.
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
}

impl PrRowData {
    /// Only pull requests on github.com reach the popover, which opens rows in the
    /// browser and copies their links; anything else is dropped here.
    pub fn keeps(url: &str) -> bool {
        url.starts_with("https://github.com/")
    }
}

/// Rows per list; the screen shows the rest.
pub const MAX_PULL_REQUESTS: usize = 25;

// ---------------------------------------------------------------------------
// Geometry.

/// A rect in points; the coordinate space is chosen by the caller (display or
/// window-local) and always top-left.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct R {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Left/right edges run the rail along y; top/bottom along x.
fn vertical(edge: Edge) -> bool {
    matches!(edge, Edge::Left | Edge::Right)
}

impl R {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn mid_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn mid_y(&self) -> f64 {
        self.y + self.h / 2.0
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    fn translated(&self, dx: f64, dy: f64) -> R {
        R::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// The union bounding box of two rects.
fn union(a: &R, b: &R) -> R {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = (a.x + a.w).max(b.x + b.w);
    let bottom = (a.y + a.h).max(b.y + b.h);
    R::new(x, y, right - x, bottom - y)
}

/// Native metrics snapped to the target display's pixel grid: the `NotchMetrics.value`
/// port — compact and large presets produce fractional points that must not straddle
/// pixels on 1x monitors.
fn value(points: f64, size_scale: f64, display_scale: f64) -> f64 {
    (points * size_scale * display_scale).round() / display_scale
}

fn size_scale_of(size: NotchSize) -> f64 {
    match size {
        NotchSize::Compact => 0.875,
        NotchSize::Standard => 1.0,
        NotchSize::Large => 1.125,
    }
}

/// Rail metrics for one size preset, display scale, and edge; the `railLayout` port.
/// Cells are 76 wide and 73 tall whatever the edge: a vertical rail is 76 thick with
/// cells stacked along it, a horizontal bar is 73 thick with cells side by side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub thickness: f64,
    /// A cell's extent along the rail's axis.
    pub cell_length: f64,
    pub cell_spacing: f64,
    /// Distance from either end to the first cell; also the ear-curve length.
    pub inset: f64,
    pub ear: f64,
    pub cell_padding: f64,
    pub content_spacing: f64,
    pub icon_slot: f64,
    pub ring_stroke: f64,
    pub inner_ring_stroke: f64,
    pub inner_ring_inset: f64,
    pub glyph: f64,
    pub label_height: f64,
}

/// The `railLayout` port.
pub fn metrics(size: NotchSize, display_scale: f64, edge: Edge) -> Metrics {
    let v = |points: f64| value(points, size_scale_of(size), display_scale);
    let cell_padding = v(1.0);
    let content_spacing = v(3.0);
    let icon_slot = v(46.0);
    // Tall enough for the 17 pt label at every preset, so the figure never scales to fit.
    let label_height = v(22.0);
    let cell_width = v(76.0);
    let cell_height = icon_slot + label_height + content_spacing + 2.0 * cell_padding;
    let vertical = vertical(edge);
    Metrics {
        thickness: if vertical { cell_width } else { cell_height },
        cell_length: if vertical { cell_height } else { cell_width },
        cell_spacing: v(8.0),
        inset: v(40.0),
        ear: v(40.0),
        cell_padding,
        content_spacing,
        icon_slot,
        ring_stroke: v(4.0),
        inner_ring_stroke: v(3.0),
        inner_ring_inset: v(6.0),
        glyph: v(24.0),
        label_height,
    }
}

/// Cell frames inside the rail (rail-local top-left origin), the `railCellFrames` port.
pub fn rail_cell_frames(edge: Edge, metrics: &Metrics, count: usize) -> Vec<R> {
    (0..count)
        .map(|index| {
            let offset =
                metrics.inset + index as f64 * (metrics.cell_length + metrics.cell_spacing);
            if vertical(edge) {
                R::new(0.0, offset, metrics.thickness, metrics.cell_length)
            } else {
                R::new(offset, 0.0, metrics.cell_length, metrics.thickness)
            }
        })
        .collect()
}

/// The collapsed "show on hover" strip centred on the rail's span; the `notchPillFrame`
/// port (thickness 6, length min(120, rail span)).
pub fn pill_frame(settings: &NotchSettings, rail: &R) -> R {
    let scale = size_scale_of(settings.size);
    let thickness = value(6.0, scale, 1.0);
    let length = value(120.0, scale, 1.0).min(if vertical(settings.edge) {
        rail.h
    } else {
        rail.w
    });
    match settings.edge {
        Edge::Left => R::new(rail.x, rail.mid_y() - length / 2.0, thickness, length),
        Edge::Right => R::new(
            rail.x + rail.w - thickness,
            rail.mid_y() - length / 2.0,
            thickness,
            length,
        ),
        Edge::Top => R::new(rail.mid_x() - length / 2.0, rail.y, length, thickness),
        Edge::Bottom => R::new(
            rail.mid_x() - length / 2.0,
            rail.y + rail.h - thickness,
            length,
            thickness,
        ),
    }
}

pub const POPOVER_WIDTH: f64 = 272.0;
pub const POPOVER_MARGIN: f64 = 8.0;
pub const TAIL_LENGTH: f64 = 8.0;

/// Where a popover of `size` sits next to `cell`: inward from the edge, centred on the
/// cell, clamped to the display's work area; the `popoverFrame` port.
pub fn popover_frame(
    cell: &R,
    edge: Edge,
    size: (f64, f64),
    display: &super::model::Display,
    scale: f64,
) -> R {
    let gap = value(2.0, scale, display.scale);
    let margin = value(POPOVER_MARGIN, scale, display.scale);
    let (w, h) = size;
    let (mut x, mut y) = match edge {
        Edge::Right => (cell.x - gap - w, cell.mid_y() - h / 2.0),
        Edge::Left => (cell.x + cell.w + gap, cell.mid_y() - h / 2.0),
        Edge::Top => (cell.mid_x() - w / 2.0, cell.y + cell.h + gap),
        Edge::Bottom => (cell.mid_x() - w / 2.0, cell.y - gap - h),
    };
    let min_x = display.x + margin;
    let max_x = display.x + display.width - margin - w;
    let min_y = display.work_y + margin;
    let max_y = display.work_y + display.work_height - margin - h;
    x = x.clamp(min_x.min(max_x), max_x.max(min_x));
    y = y.clamp(min_y.min(max_y), max_y.max(min_y));
    R::new(
        value(x, 1.0, display.scale),
        value(y, 1.0, display.scale),
        w,
        h,
    )
}

// ---------------------------------------------------------------------------
// The plan: everything the renderer and the window need for one frame.

/// Where the pointer is, as the window thread sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hover {
    /// OnHover mode: the strip was reached and the rail is open.
    pub rail_open: bool,
    /// The cell whose popover is open (hovered or pinned), as a rail index.
    pub active: Option<usize>,
    pub cap_hovered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    Muted,
    Green,
    Red,
    Amber,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CellContent {
    Provider {
        provider: AgentId,
        primary: Option<QuotaView>,
        fable: Option<QuotaView>,
        label: String,
    },
    PullRequests {
        segments: Vec<CiState>,
        count: u64,
        readable: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuotaView {
    /// `None` once the window reset or the figure is out of range, like the mac
    /// `Quota.percent(at:)`; the ring then draws no arc and the label reads "—".
    pub percent: Option<f64>,
    pub reached: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellPlan {
    pub rect: R,
    pub content: CellContent,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapPlan {
    pub rect: R,
    pub hovered: bool,
    pub pinned: bool,
}

/// One positioned element of the popover; the planner computes rects once and the
/// renderer draws exactly what was placed, so measure/zones/draw never drift.
#[derive(Clone, Debug, PartialEq)]
pub enum PopItem {
    /// A provider glyph or pull-request mark of `size` points.
    Mark {
        kind: MarkKind,
        size: f64,
    },
    Text {
        text: String,
        size: f64,
        weight: TextWeight,
        color: Color,
        /// Right-align inside the item's rect.
        right: bool,
    },
    /// A quota bar: track plus a fill scaled by the percent.
    Bar {
        percent: Option<f64>,
        color: Color,
    },
    Divider,
    /// The pull-request CI dot: hollow when nothing reported.
    Dot {
        ci: CiState,
    },
    CopyIcon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Provider(AgentId),
    PullRequests,
    OpenArrow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextWeight {
    Regular,
    Medium,
    Semibold,
}

impl TextWeight {
    fn face(self) -> text::Weight {
        match self {
            Self::Regular => text::Weight::Regular,
            Self::Medium => text::Weight::Medium,
            Self::Semibold => text::Weight::Semibold,
        }
    }
}

pub type Color = [u8; 4];

/// What a popover interaction can ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Zone {
    /// Open the pull request on GitHub.
    OpenRow { url: String },
    /// Copy "review please: <title>" with the link.
    CopyRow { url: String, title: String },
    /// Open the Limits or Pull requests screen.
    Footer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopoverPlan {
    /// The full popover rect, tail space included.
    pub rect: R,
    /// The card rect (without the tail side).
    pub card: R,
    /// Centre of the tail along the rail's axis, in window-local coordinates.
    pub tail: f64,
    pub entries: Vec<(PopItem, R)>,
    pub zones: Vec<(Zone, R)>,
    pub action_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub edge: Edge,
    pub display_scale: f64,
    pub size_scale: f64,
    /// The window bounds in display points; the pixmap covers exactly this.
    pub window: R,
    pub rail: R,
    pub metrics: Metrics,
    pub cells: Vec<CellPlan>,
    pub cap: CapPlan,
    /// The collapsed strip, when the rail is not open.
    pub pill: Option<R>,
    pub popover: Option<PopoverPlan>,
}

/// The rail cell under a window-local point.
pub fn rail_hit(plan: &Plan, x: f64, y: f64) -> Option<usize> {
    plan.cells.iter().position(|cell| cell.rect.contains(x, y))
}

// ---------------------------------------------------------------------------
// Quota / session / PR projections (the NotchCore `Provider` ports).

fn quota_view(window: &LimitWindowDto) -> QuotaView {
    let percent = quota_percent(window);
    QuotaView {
        percent,
        reached: percent.is_some_and(|percent| percent.round() as u64 == 100),
    }
}

/// Codex's extra/internal windows never reach the rings (the `visibleWindows` port).
fn visible_windows(provider: &ProviderData) -> Vec<LimitWindowDto> {
    if provider.provider != AgentId::Codex {
        return provider.windows.clone();
    }
    provider
        .windows
        .iter()
        .filter(|w| {
            let label = w
                .label
                .split('·')
                .next_back()
                .unwrap_or("")
                .trim()
                .to_lowercase();
            !["gpt-reserve", "gpt-5.3-codex-spark"].contains(&label.as_str())
                && !["base_model_inference", "codex_bengalfox"]
                    .iter()
                    .any(|bucket| {
                        w.id == format!("extra:{bucket}")
                            || w.id.starts_with(&format!("extra:{bucket}:"))
                    })
        })
        .cloned()
        .collect()
}

/// Windows in popover order: the current session first, then weekly, then per-model.
fn ordered_windows(provider: &ProviderData) -> Vec<LimitWindowDto> {
    let rank = |kind: LimitWindowKind| match kind {
        LimitWindowKind::Session => 0,
        LimitWindowKind::Weekly => 1,
        LimitWindowKind::Model => 2,
    };
    let mut windows = visible_windows(provider);
    windows.sort_by_key(|w| rank(w.kind));
    windows
}

/// The outer ring's window: Claude's weekly limit, otherwise the current session, then
/// weekly. Only for a readable current account (the `Provider.primary` port).
fn primary_quota(provider: &ProviderData) -> Option<QuotaView> {
    if provider.status != LimitsStatus::Ok {
        return None;
    }
    let visible = visible_windows(provider);
    let window = if provider.provider == AgentId::Claude {
        visible.iter().find(|w| w.kind == LimitWindowKind::Weekly)
    } else {
        visible
            .iter()
            .find(|w| w.kind == LimitWindowKind::Session)
            .or_else(|| visible.iter().find(|w| w.kind == LimitWindowKind::Weekly))
    }?;
    Some(quota_view(window))
}

/// Claude's Fable weekly window, shown on an inner ring with its own label.
fn fable_quota(provider: &ProviderData) -> Option<QuotaView> {
    if provider.provider != AgentId::Claude || provider.status != LimitsStatus::Ok {
        return None;
    }
    visible_windows(provider)
        .iter()
        .find(|w| {
            w.kind == LimitWindowKind::Model && w.label.trim().to_lowercase() == "weekly · fable"
        })
        .map(quota_view)
}

fn list_title(list: GithubList) -> &'static str {
    match list {
        GithubList::Mine => "Mine",
        GithubList::ReviewRequested => "Review requested",
        GithubList::Assigned => "Assigned",
    }
}

/// The same wording as the Pull requests screen's badges.
fn row_badges(row: &PrRowData) -> Vec<(String, Badge)> {
    let mut badges = Vec::new();
    if row.is_draft {
        badges.push(("Draft".into(), Badge::Muted));
    }
    match row.review_decision {
        Some(ReviewDecision::Approved) => badges.push(("Approved".into(), Badge::Green)),
        Some(ReviewDecision::ChangesRequested) => {
            badges.push(("Changes requested".into(), Badge::Red))
        }
        _ => {}
    }
    match row.merge_kind {
        Some(MergeKind::Conflicts) => badges.push(("Conflicts".into(), Badge::Red)),
        Some(MergeKind::Queued) => badges.push(("Queued".into(), Badge::Green)),
        Some(MergeKind::AutoMerge) => badges.push(("Auto-merge".into(), Badge::Muted)),
        Some(MergeKind::Ready) => badges.push(("Ready to merge".into(), Badge::Green)),
        Some(MergeKind::Behind) => badges.push(("Behind base".into(), Badge::Amber)),
        Some(MergeKind::Blocked) => badges.push(("Blocked".into(), Badge::Amber)),
        None => {}
    }
    badges
}

// ---------------------------------------------------------------------------
// Planning.

fn cell_content(data: &CellData) -> CellContent {
    match data {
        CellData::Provider(provider) => CellContent::Provider {
            provider: provider.provider,
            primary: primary_quota(provider),
            fable: fable_quota(provider),
            label: primary_quota(provider)
                .and_then(|quota| quota.percent)
                .map(format_percent)
                .unwrap_or_else(|| "—".into()),
        },
        CellData::PullRequests(pulls) => {
            let readable = pulls.status == GithubStatus::Ok || pulls.stale;
            let rows: Vec<&PrRowData> = pulls.lists.iter().flat_map(|list| &list.items).collect();
            let mut seen = std::collections::BTreeSet::new();
            let count = rows
                .iter()
                .filter(|row| seen.insert(row.id.as_str()))
                .count() as u64;
            CellContent::PullRequests {
                segments: if readable {
                    rows.iter().take(24).map(|row| row.ci).collect()
                } else {
                    Default::default()
                },
                count,
                readable,
            }
        }
    }
}

/// Builds the full plan for one frame, or `None` when the notch must stay hidden.
pub fn plan(
    settings: &NotchSettings,
    displays: &[super::model::Display],
    data: &RailData,
    hover: Hover,
) -> Option<Plan> {
    let rail_frame = layout(settings, displays)?;
    let display = displays
        .iter()
        .find(|d| Some(&d.id) == settings.display_id.as_ref())?;
    let edge = settings.edge;
    let display_scale = display.scale;
    let size_scale = size_scale_of(settings.size);
    let scaled = |points: f64| value(points, size_scale, display_scale);
    let metrics = metrics(settings.size, display_scale, edge);
    let rail = R::new(
        rail_frame.x,
        rail_frame.y,
        rail_frame.width,
        rail_frame.height,
    );
    let collapsed = settings.show == ShowMode::OnHover && !hover.rail_open;

    let cells: Vec<CellPlan> = rail_cell_frames(edge, &metrics, data.cells.len())
        .into_iter()
        .enumerate()
        .map(|(index, rect)| CellPlan {
            rect: rect.translated(rail.x, rail.y),
            content: cell_content(&data.cells[index]),
            active: hover.active == Some(index),
        })
        .collect();

    let cap = CapPlan {
        rect: if vertical(edge) {
            R::new(rail.x, rail.y, rail.w, metrics.ear)
        } else {
            R::new(rail.x, rail.y, metrics.ear, rail.h)
        },
        hovered: hover.cap_hovered,
        pinned: settings.show == ShowMode::Always,
    };

    let popover = hover.active.filter(|_| !collapsed).and_then(|index| {
        let cell_rect = &cells.get(index)?.rect;
        let card_w = scaled(POPOVER_WIDTH);
        let padding = scaled(12.0);
        let inner_w = card_w - 2.0 * padding;
        let tail_space = if vertical(edge) {
            scaled(TAIL_LENGTH)
        } else {
            0.0
        };
        let (entries, zones) = popover_entries(data, index, inner_w, size_scale, display_scale);
        let card_h = measure(&entries, size_scale, display_scale, padding);
        let height = card_h
            + if vertical(edge) {
                0.0
            } else {
                scaled(TAIL_LENGTH)
            };
        let placed = popover_frame(
            cell_rect,
            edge,
            (card_w + tail_space, height),
            display,
            size_scale,
        );
        let card = R::new(
            if edge == Edge::Left {
                placed.x + tail_space
            } else {
                placed.x
            },
            if edge == Edge::Top {
                placed.y + tail_space
            } else {
                placed.y
            },
            card_w,
            card_h,
        );
        let tail = if vertical(edge) {
            cell_rect.mid_y() - placed.y
        } else {
            cell_rect.mid_x() - placed.x
        };
        Some(PopoverPlan {
            rect: placed,
            card,
            tail,
            entries: entries
                .into_iter()
                .map(|(item, rect)| (item, rect.translated(card.x, card.y)))
                .collect(),
            // Zones come out of the same walk as the entries, in card coordinates:
            // they move onto the card with them, or nothing in the popover is
            // clickable.
            zones: zones
                .into_iter()
                .map(|(zone, rect)| (zone, rect.translated(card.x, card.y)))
                .collect(),
            action_error: data.action_error.clone(),
        })
    });

    let window = match &popover {
        Some(popover) => union(&rail, &popover.rect),
        None => rail,
    };
    let pill = collapsed.then(|| pill_frame(settings, &rail));

    Some(Plan {
        edge,
        display_scale,
        size_scale,
        window,
        rail: rail.translated(-window.x, -window.y),
        metrics,
        cells: cells
            .into_iter()
            .map(|mut cell| {
                cell.rect = cell.rect.translated(-window.x, -window.y);
                cell
            })
            .collect(),
        cap: CapPlan {
            rect: cap.rect.translated(-window.x, -window.y),
            hovered: cap.hovered,
            pinned: cap.pinned,
        },
        pill: pill.map(|pill| pill.translated(-window.x, -window.y)),
        popover: popover.map(|popover| translate_popover(popover, -window.x, -window.y)),
    })
}

fn translate_popover(mut popover: PopoverPlan, dx: f64, dy: f64) -> PopoverPlan {
    popover.rect = popover.rect.translated(dx, dy);
    popover.card = popover.card.translated(dx, dy);
    for (_, rect) in &mut popover.entries {
        *rect = rect.translated(dx, dy);
    }
    for (_, rect) in &mut popover.zones {
        *rect = rect.translated(dx, dy);
    }
    popover
}

// ---------------------------------------------------------------------------
// Popover content -> positioned entries.

/// Walks the popover content, placing every element; one source of truth for
/// measurement, hit zones, and drawing.
fn popover_entries(
    data: &RailData,
    index: usize,
    inner_w: f64,
    size_scale: f64,
    display_scale: f64,
) -> (Vec<(PopItem, R)>, Vec<(Zone, R)>) {
    let v = |points: f64| value(points, size_scale, display_scale);
    let line_h = |size: f64, weight: TextWeight| {
        let _ = weight;
        v(size) * 1.3
    };
    let mut entries: Vec<(PopItem, R)> = Vec::new();
    let mut zones: Vec<(Zone, R)> = Vec::new();
    // Card padding: the content starts inside the card, not on its edge.
    let mut y = v(12.0);
    let x = v(12.0);
    let spacing = v(10.0);
    // Where a glyph's own centre has to land to sit on the middle of a line of text.
    let cap_middle = |points: f64, weight: TextWeight| -> f64 {
        f64::from(text::cap_middle_px(
            device_size(points, display_scale),
            weight.face(),
        )) / display_scale
    };
    let header_lift = cap_middle(13.0, TextWeight::Semibold) - v(13.0) / 2.0;

    fn text_entry(
        entries: &mut Vec<(PopItem, R)>,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        text: String,
        size: f64,
        weight: TextWeight,
        color: Color,
        right: bool,
    ) {
        entries.push((
            PopItem::Text {
                text,
                size,
                weight,
                color,
                right,
            },
            R::new(x, y, w, h),
        ));
    }

    let cell = match data.cells.get(index) {
        Some(cell) => cell,
        None => return (entries, zones),
    };
    match cell {
        CellData::Provider(provider) => {
            entries.push((
                PopItem::Mark {
                    kind: MarkKind::Provider(provider.provider),
                    size: v(13.0),
                },
                // The mac header is an HStack, so the glyph centres on the title's
                // line box rather than hanging from its top.
                R::new(x, y + header_lift, v(13.0), v(13.0)),
            ));
            text_entry(
                &mut entries,
                x + v(13.0) + v(7.0),
                y,
                inner_w - v(13.0) - v(7.0),
                line_h(13.0, TextWeight::Semibold),
                format!("{} Usage", provider_name(provider.provider)),
                v(13.0),
                TextWeight::Semibold,
                [255, 255, 255, 255],
                false,
            );
            y += line_h(13.0, TextWeight::Semibold) + spacing;

            if provider.status != LimitsStatus::Ok {
                let message = provider
                    .message
                    .clone()
                    .unwrap_or_else(|| "Usage unavailable.".into());
                for line in wrap_lines(
                    &message,
                    inner_w,
                    v(11.0),
                    TextWeight::Regular,
                    display_scale,
                    4,
                ) {
                    text_entry(
                        &mut entries,
                        x,
                        y,
                        inner_w,
                        line_h(11.0, TextWeight::Regular),
                        line,
                        v(11.0),
                        TextWeight::Regular,
                        MUTED_INK,
                        false,
                    );
                    y += line_h(11.0, TextWeight::Regular);
                }
                if !provider.windows.is_empty() {
                    text_entry(
                        &mut entries,
                        x,
                        y,
                        inner_w,
                        line_h(10.0, TextWeight::Regular),
                        "Refresh paused. Last observed values below.".into(),
                        v(10.0),
                        TextWeight::Regular,
                        WARN_AMBER,
                        false,
                    );
                    y += line_h(10.0, TextWeight::Regular) + v(4.0);
                }
            } else if provider.windows.is_empty() && provider.message.is_none() {
                text_entry(
                    &mut entries,
                    x,
                    y,
                    inner_w,
                    line_h(11.0, TextWeight::Regular),
                    "Checking usage…".into(),
                    v(11.0),
                    TextWeight::Regular,
                    MUTED_INK,
                    false,
                );
                y += line_h(11.0, TextWeight::Regular);
            }

            for window in ordered_windows(provider) {
                // The note is right-aligned; the label truncates so the two never
                // overlap (SwiftUI's Spacer + lineLimit(1) behaviour). The note yields
                // first: it keeps at most 60 % of the row, the label the rest.
                let note_max = inner_w * 0.6;
                let note = ellipsize(
                    &reset_note(&window),
                    note_max,
                    v(10.0),
                    TextWeight::Regular,
                    display_scale,
                );
                let note_w = measure_weight(&note, v(10.0), TextWeight::Regular, display_scale);
                let label_space = (inner_w - note_w - v(8.0)).max(inner_w * 0.3);
                let label = ellipsize(
                    &window.label,
                    label_space,
                    v(11.0),
                    TextWeight::Semibold,
                    display_scale,
                );
                text_entry(
                    &mut entries,
                    x,
                    y,
                    inner_w,
                    line_h(11.0, TextWeight::Semibold),
                    label,
                    v(11.0),
                    TextWeight::Semibold,
                    [255, 255, 255, 255],
                    false,
                );
                text_entry(
                    &mut entries,
                    x,
                    y,
                    inner_w,
                    line_h(10.0, TextWeight::Regular),
                    note,
                    v(10.0),
                    TextWeight::Regular,
                    MUTED_INK,
                    true,
                );
                y += line_h(11.0, TextWeight::Semibold) + v(5.0);
                let percent = quota_percent(&window);
                entries.push((
                    PopItem::Bar {
                        percent,
                        color: meter_color(percent, provider_color(provider.provider)),
                    },
                    R::new(x, y, inner_w, v(4.0)),
                ));
                y += v(4.0) + v(5.0);
                text_entry(
                    &mut entries,
                    x,
                    y,
                    inner_w,
                    line_h(10.5, TextWeight::Medium),
                    quota_percent(&window)
                        .map(|percent| format!("{} Used", format_percent(percent)))
                        .unwrap_or_else(|| "—".into()),
                    v(10.5),
                    TextWeight::Medium,
                    [255, 255, 255, 255],
                    false,
                );
                y += line_h(10.5, TextWeight::Medium) + spacing;
            }

            if !provider.sessions.is_empty() {
                entries.push((PopItem::Divider, R::new(x, y, inner_w, 1.0)));
                y += spacing;
                for session in &provider.sessions {
                    let working =
                        session.status == crate::side_notch::sessions::SessionStatus::Working;
                    let status_text = if working { "working" } else { "idle" };
                    let status_w =
                        measure_weight(status_text, v(10.5), TextWeight::Medium, display_scale);
                    let name = ellipsize(
                        &session.name,
                        inner_w - status_w - v(8.0),
                        v(11.0),
                        TextWeight::Semibold,
                        display_scale,
                    );
                    text_entry(
                        &mut entries,
                        x,
                        y,
                        inner_w,
                        line_h(11.0, TextWeight::Semibold),
                        name,
                        v(11.0),
                        TextWeight::Semibold,
                        [255, 255, 255, 255],
                        false,
                    );
                    text_entry(
                        &mut entries,
                        x,
                        y,
                        inner_w,
                        line_h(10.5, TextWeight::Medium),
                        if working {
                            "working".into()
                        } else {
                            "idle".into()
                        },
                        v(10.5),
                        TextWeight::Medium,
                        if working {
                            provider_color(provider.provider)
                        } else {
                            MUTED_INK
                        },
                        true,
                    );
                    y += line_h(11.0, TextWeight::Semibold) + v(2.0);
                    text_entry(
                        &mut entries,
                        x,
                        y,
                        inner_w,
                        line_h(10.0, TextWeight::Regular),
                        format!("{} · {}", session.place, session.project),
                        v(10.0),
                        TextWeight::Regular,
                        MUTED_INK,
                        false,
                    );
                    text_entry(
                        &mut entries,
                        x,
                        y,
                        inner_w,
                        line_h(10.0, TextWeight::Regular),
                        age_text(&session.last_active_at),
                        v(10.0),
                        TextWeight::Regular,
                        MUTED_INK,
                        true,
                    );
                    y += line_h(10.0, TextWeight::Regular) + v(8.0);
                }
                // The sessions block ends where the next card child starts.
                y += spacing - v(8.0);
            }
        }
        CellData::PullRequests(pulls) => {
            entries.push((
                PopItem::Mark {
                    kind: MarkKind::PullRequests,
                    size: v(13.0),
                },
                R::new(x, y + header_lift, v(13.0), v(13.0)),
            ));
            text_entry(
                &mut entries,
                x + v(13.0) + v(7.0),
                y,
                inner_w - v(13.0) - v(7.0),
                line_h(13.0, TextWeight::Semibold),
                "Pull requests".into(),
                v(13.0),
                TextWeight::Semibold,
                [255, 255, 255, 255],
                false,
            );
            y += line_h(13.0, TextWeight::Semibold) + spacing;

            let readable = pulls.status == GithubStatus::Ok || pulls.stale;
            if pulls.hint.is_some() && pulls.status != GithubStatus::Ok {
                let color = if readable { WARN_AMBER } else { MUTED_INK };
                for line in wrap_lines(
                    pulls.hint.as_deref().unwrap_or(""),
                    inner_w,
                    v(11.0),
                    TextWeight::Regular,
                    display_scale,
                    4,
                ) {
                    text_entry(
                        &mut entries,
                        x,
                        y,
                        inner_w,
                        line_h(11.0, TextWeight::Regular),
                        line,
                        v(11.0),
                        TextWeight::Regular,
                        color,
                        false,
                    );
                    y += line_h(11.0, TextWeight::Regular);
                }
                y += v(4.0);
            }
            for list in pulls.lists.iter().filter(|_| readable) {
                text_entry(
                    &mut entries,
                    x,
                    y,
                    inner_w,
                    line_h(9.5, TextWeight::Semibold),
                    list_title(list.id).to_uppercase(),
                    v(9.5),
                    TextWeight::Semibold,
                    MUTED_INK,
                    false,
                );
                text_entry(
                    &mut entries,
                    x,
                    y,
                    inner_w,
                    line_h(9.5, TextWeight::Regular),
                    if list.total > list.items.len() as u64 {
                        format!("{} of {}", list.items.len(), list.total)
                    } else {
                        list.items.len().to_string()
                    },
                    v(9.5),
                    TextWeight::Regular,
                    MUTED_INK,
                    true,
                );
                y += line_h(9.5, TextWeight::Semibold) + v(6.0);
                if list.items.is_empty() {
                    text_entry(
                        &mut entries,
                        x,
                        y,
                        inner_w,
                        line_h(10.5, TextWeight::Regular),
                        "Nothing open.".into(),
                        v(10.5),
                        TextWeight::Regular,
                        MUTED_INK,
                        false,
                    );
                    y += line_h(10.5, TextWeight::Regular) + v(6.0);
                }
                for row in &list.items {
                    let title_size = v(11.0);
                    let copy_w = v(20.0);
                    let lines = wrap_lines(
                        &row.title,
                        inner_w - copy_w - v(8.0),
                        title_size,
                        TextWeight::Semibold,
                        display_scale,
                        2,
                    );
                    let copy_h = v(16.0);
                    let row_top = y;
                    for (line_index, line) in lines.iter().enumerate() {
                        let line_h = line_h(11.0, TextWeight::Semibold);
                        text_entry(
                            &mut entries,
                            x,
                            y,
                            inner_w - copy_w - v(8.0),
                            line_h,
                            line.clone(),
                            title_size,
                            TextWeight::Semibold,
                            [255, 255, 255, 255],
                            false,
                        );
                        if line_index == 0 {
                            entries.push((
                                PopItem::CopyIcon,
                                R::new(x + inner_w - copy_w, y, copy_w, copy_h),
                            ));
                            zones.push((
                                Zone::CopyRow {
                                    url: row.url.clone(),
                                    title: row.title.clone(),
                                },
                                R::new(x + inner_w - copy_w, y, copy_w, copy_h.max(y + copy_h - y)),
                            ));
                        }
                        y += line_h;
                    }
                    y += v(2.0);
                    let repo_size = v(10.0);
                    let repo = format!("{} #{}", row.repo, row.number);
                    let mut line_x = x;
                    text_entry(
                        &mut entries,
                        line_x,
                        y,
                        inner_w,
                        line_h(10.0, TextWeight::Regular),
                        repo.clone(),
                        repo_size,
                        TextWeight::Regular,
                        MUTED_INK,
                        false,
                    );
                    line_x += measure_text(&repo, repo_size, display_scale) + v(8.0);
                    // Badges stop before the CI dot at the row's right edge.
                    let badge_limit = x + inner_w - v(7.0) - v(10.0);
                    for (badge, color) in row_badges(row) {
                        let badge_size = v(9.5);
                        let badge_w =
                            measure_weight(&badge, badge_size, TextWeight::Medium, display_scale);
                        if line_x + badge_w > badge_limit {
                            break;
                        }
                        text_entry(
                            &mut entries,
                            line_x,
                            y,
                            inner_w,
                            line_h(9.5, TextWeight::Medium),
                            badge.clone(),
                            badge_size,
                            TextWeight::Medium,
                            badge_color(color),
                            false,
                        );
                        line_x += badge_w + v(6.0);
                    }
                    entries.push((
                        PopItem::Dot { ci: row.ci },
                        R::new(
                            x + inner_w - v(7.0),
                            y + cap_middle(10.0, TextWeight::Regular) - v(7.0) / 2.0,
                            v(7.0),
                            v(7.0),
                        ),
                    ));
                    // The whole row opens on GitHub (except the copy affordance).
                    zones.push((
                        Zone::OpenRow {
                            url: row.url.clone(),
                        },
                        R::new(
                            x,
                            row_top,
                            inner_w,
                            y + line_h(10.0, TextWeight::Regular) - row_top,
                        ),
                    ));
                    y += line_h(10.0, TextWeight::Regular) + v(6.0);
                }
                y += v(8.0);
            }
        }
    }

    // The action error and the footer link.
    if let Some(error) = &data.action_error {
        text_entry(
            &mut entries,
            x,
            y,
            inner_w,
            line_h(10.0, TextWeight::Regular),
            error.clone(),
            v(10.0),
            TextWeight::Regular,
            WARN_AMBER,
            false,
        );
        y += line_h(10.0, TextWeight::Regular);
    }
    let footer_title = match cell {
        CellData::Provider(_) => "Open Limits",
        CellData::PullRequests(_) => "Open Pull requests",
    };
    let footer_size = v(10.5);
    let footer_line = line_h(10.5, TextWeight::Medium);
    let arrow = v(9.0);
    let label_w = measure_weight(footer_title, footer_size, TextWeight::Medium, display_scale);
    let footer_w = label_w + v(3.0) + arrow;
    let footer_x = x + inner_w - footer_w;
    text_entry(
        &mut entries,
        footer_x,
        y,
        label_w,
        footer_line,
        footer_title.into(),
        footer_size,
        TextWeight::Medium,
        MUTED_INK,
        false,
    );
    entries.push((
        PopItem::Mark {
            kind: MarkKind::OpenArrow,
            size: arrow,
        },
        R::new(
            x + inner_w - arrow,
            y + cap_middle(10.5, TextWeight::Medium) - arrow / 2.0,
            arrow,
            arrow,
        ),
    ));
    zones.push((Zone::Footer, R::new(footer_x, y, footer_w, footer_line)));
    // The card's height comes from the entries, so the walk ends here.
    (entries, zones)
}

fn wrap_lines(
    text: &str,
    max_w: f64,
    size: f64,
    weight: TextWeight,
    scale: f64,
    max_lines: usize,
) -> Vec<String> {
    if size <= 0.0 {
        return vec![text.to_string()];
    }
    // Semibold runs about a fifth wider than regular at these sizes, so wrapping at
    // the wrong weight pushes the last word past the box it was measured for.
    text::wrap_px(
        text,
        (max_w * scale) as f32,
        device_size(size, scale),
        weight.face(),
        max_lines,
    )
}

/// The rasterised height of `size` points on a display of `scale`.
fn device_size(size: f64, scale: f64) -> f32 {
    (size * scale).max(1.0) as f32
}

/// A run of text measured in points. GDI hints every pixel size on its own, so a run
/// measured at the point size is not `scale` times narrower than the same run
/// rasterised at point x scale; measuring at the device size and converting back
/// keeps a scaled display from drifting by a fifth on the longer labels.
fn measure_weight(text: &str, size: f64, weight: TextWeight, scale: f64) -> f64 {
    f64::from(text::measure_px(
        text,
        device_size(size, scale),
        weight.face(),
    )) / scale
}

fn measure_text(text: &str, size: f64, scale: f64) -> f64 {
    measure_weight(text, size, TextWeight::Regular, scale)
}

/// Truncates `text` with an ellipsis so it fits `max_px` when drawn at `size`/`weight`.
fn ellipsize(text: &str, max_px: f64, size: f64, weight: TextWeight, scale: f64) -> String {
    if measure_weight(text, size, weight, scale) <= max_px {
        return text.to_string();
    }
    let mut cut = text.to_string();
    while cut.chars().count() > 1
        && measure_weight(&format!("{cut}…"), size, weight, scale) > max_px
    {
        cut.pop();
    }
    format!("{cut}…")
}

/// The window's usable percent: `None` after the reset, `None` when out of range.
fn quota_percent(window: &LimitWindowDto) -> Option<f64> {
    let finite = window.used_percent.is_finite() && (0.0..=100.0).contains(&window.used_percent);
    let expired = window
        .resets_at
        .as_deref()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .is_some_and(|reset| reset <= chrono::Utc::now());
    finite.then_some(window.used_percent).filter(|_| !expired)
}

/// "Resets Tue 8:00 PM" while the window is pending; "Reset … · last seen 97%"
/// afterwards; empty when the provider reported no reset. Minute-granular, computed
/// when the host reads, so a re-render never happens for clock drift alone.
fn reset_note(window: &LimitWindowDto) -> String {
    let Some(reset) = window
        .resets_at
        .as_deref()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
    else {
        return String::new();
    };
    let now = chrono::Utc::now();
    let clock = reset
        .with_timezone(&chrono::Local)
        .format("%a %-I:%M %p")
        .to_string();
    if reset <= now {
        format!("Reset {clock} · last seen {}%", window.used_percent.round())
    } else {
        format!("Resets {clock}")
    }
}

/// "just now", "4 min", "2 h", "3 d" since the last activity.
fn age_text(last_active_at: &str) -> String {
    let Some(instant) = last_active_at.parse::<chrono::DateTime<chrono::Utc>>().ok() else {
        return String::new();
    };
    let minutes = (chrono::Utc::now() - instant).num_minutes();
    if minutes < 1 {
        "just now".into()
    } else if minutes < 60 {
        format!("{minutes} min")
    } else if minutes < 1440 {
        format!("{} h", minutes / 60)
    } else {
        format!("{} d", minutes / 1440)
    }
}

/// The popover's card height: the bottom of the lowest entry plus padding.
fn measure(entries: &[(PopItem, R)], size_scale: f64, display_scale: f64, padding: f64) -> f64 {
    let bottom = entries
        .iter()
        .map(|(_, rect)| rect.y + rect.h)
        .fold(0.0_f64, f64::max);
    let _ = (size_scale, display_scale);
    bottom + padding
}

// ---------------------------------------------------------------------------
// Rendering.

const RAIL_INK: Color = [8, 8, 8, 255];
const POPOVER_INK: Color = [14, 14, 17, 255];
const MUTED_INK: Color = [153, 153, 153, 255];
const TRACK_INK: Color = [44, 44, 44, 255];
const LIVE_GREEN: Color = [74, 200, 120, 255];
const WARN_AMBER: Color = [224, 179, 65, 255];
const TRIP_RED: Color = [226, 89, 76, 255];
const FABLE_ORANGE: Color = [247, 173, 113, 255];
const CLAUDE_ORANGE: Color = [232, 148, 74, 255];
const CODEX_INK: Color = [238, 240, 242, 255];
const CURSOR_BLUE: Color = [122, 162, 255, 255];
const ANTIGRAVITY_MUTE: Color = [140, 147, 157, 255];
const FABLE_TRACK: Color = [53, 42, 38, 255];
/// The mac `ShowToggleCap`'s hovered fill: white at 11 %.
const CAP_HIGHLIGHT: Color = [255, 255, 255, 28];
const UNREADABLE_INK: Color = [77, 77, 77, 255];

fn provider_color(provider: AgentId) -> Color {
    match provider {
        AgentId::Claude => CLAUDE_ORANGE,
        AgentId::Codex => CODEX_INK,
        AgentId::Cursor => CURSOR_BLUE,
        AgentId::Antigravity => ANTIGRAVITY_MUTE,
    }
}

/// The base accent, amber from 70 %, red from 90 %; grey when unreadable.
fn meter_color(percent: Option<f64>, base: Color) -> Color {
    match percent {
        None => UNREADABLE_INK,
        Some(percent) if percent >= 90.0 => TRIP_RED,
        Some(percent) if percent >= 70.0 => WARN_AMBER,
        Some(_) => base,
    }
}

/// The mac `formatPercent`: a used sliver reads "<1%" rather than rounding to zero.
fn format_percent(percent: f64) -> String {
    if percent > 0.0 && percent < 1.0 {
        "<1%".into()
    } else {
        format!("{}%", percent.round())
    }
}

fn ci_color(ci: CiState) -> Color {
    match ci {
        CiState::Success => LIVE_GREEN,
        CiState::Failure | CiState::Error => TRIP_RED,
        CiState::Pending => WARN_AMBER,
        CiState::None => UNREADABLE_INK,
    }
}

fn badge_color(badge: Badge) -> Color {
    match badge {
        Badge::Muted => MUTED_INK,
        Badge::Green => LIVE_GREEN,
        Badge::Red => TRIP_RED,
        Badge::Amber => WARN_AMBER,
    }
}

fn provider_name(provider: AgentId) -> &'static str {
    match provider {
        AgentId::Claude => "Claude",
        AgentId::Codex => "Codex",
        AgentId::Antigravity => "Antigravity",
        AgentId::Cursor => "Cursor",
    }
}

/// Renders the plan into a premultiplied-BGRA pixmap sized to `plan.window`.
pub fn render(plan: &Plan) -> Pixmap {
    let scale = plan.display_scale as f32;
    let width = ((plan.window.w * plan.display_scale).round() as u32).max(1);
    let height = ((plan.window.h * plan.display_scale).round() as u32).max(1);
    let mut pixmap = Pixmap::new(width, height).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
    pixmap.fill(tiny_skia::Color::TRANSPARENT);

    if let Some(pill) = plan.pill {
        draw_pill(&mut pixmap, &pill, scale);
        return pixmap;
    }

    draw_silhouette_masked(
        &mut pixmap,
        &plan.rail,
        plan.edge,
        &plan.metrics,
        scale,
        RAIL_INK,
        None,
    );
    draw_cap(
        &mut pixmap,
        &plan.cap,
        plan.edge,
        &plan.metrics,
        &plan.rail,
        scale,
    );
    for cell in &plan.cells {
        draw_cell(&mut pixmap, cell, plan, scale);
    }
    if let Some(popover) = &plan.popover {
        draw_popover(&mut pixmap, popover, plan, scale);
    }
    pixmap
}

fn paint_solid(pixmap: &mut Pixmap, path: &Path, color: Color, even_odd: bool) {
    paint_solid_masked(pixmap, path, color, even_odd, None);
}

fn paint_solid_masked(
    pixmap: &mut Pixmap,
    path: &Path,
    color: Color,
    even_odd: bool,
    mask: Option<&tiny_skia::Mask>,
) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    pixmap.fill_path(
        path,
        &paint,
        if even_odd {
            FillRule::EvenOdd
        } else {
            FillRule::Winding
        },
        Transform::identity(),
        mask,
    );
}

use tiny_skia::Path;

/// The notch silhouette: a bar whose inner side is straight and whose two ends flare
/// into the screen edge with an S-curve `ear` points long; the `NotchSilhouette` port.
fn draw_silhouette_masked(
    pixmap: &mut Pixmap,
    rail: &R,
    edge: Edge,
    metrics: &Metrics,
    scale: f32,
    color: Color,
    mask: Option<&tiny_skia::Mask>,
) {
    let t = if vertical(edge) { rail.w } else { rail.h } as f32;
    let length = if vertical(edge) { rail.h } else { rail.w } as f32;
    let ear = (metrics.ear as f32).min(length / 2.0);
    // Local frame: the screen edge is x == t (+1 to hide the seam), the rail extends
    // inward to x == 0, and the axis runs along y.
    let edge_x = t + 1.0;
    let mut pb = PathBuilder::new();
    pb.move_to(edge_x, 0.0);
    pb.cubic_to(t, ear * 0.39, t * 0.76, ear * 0.53, t * 0.45, ear * 0.5);
    pb.cubic_to(t * 0.16, ear * 0.46, 0.0, ear * 0.69, 0.0, ear);
    pb.line_to(0.0, length - ear);
    pb.cubic_to(
        0.0,
        length - ear * 0.69,
        t * 0.16,
        length - ear * 0.46,
        t * 0.45,
        length - ear * 0.5,
    );
    pb.cubic_to(
        t * 0.76,
        length - ear * 0.53,
        t,
        length - ear * 0.39,
        edge_x,
        length,
    );
    pb.close();
    let Some(local) = pb.finish() else {
        return;
    };
    // Place the local frame onto the rail rect according to the edge, then to device px.
    let place = match edge {
        Edge::Right => Transform::from_translate(rail.x as f32, rail.y as f32),
        Edge::Left => Transform::from_row(-1.0, 0.0, 0.0, 1.0, rail.x as f32 + t, rail.y as f32),
        Edge::Top => Transform::from_row(0.0, -1.0, 1.0, 0.0, rail.x as f32, rail.y as f32 + t),
        Edge::Bottom => Transform::from_row(0.0, 1.0, 1.0, 0.0, rail.x as f32, rail.y as f32),
    };
    let scaled = Transform::from_scale(scale, scale).pre_concat(place);
    if let Some(path) = local.transform(scaled) {
        paint_solid_masked(pixmap, &path, color, false, mask);
    }
}

fn draw_pill(pixmap: &mut Pixmap, pill: &R, scale: f32) {
    let bg = PathBuilder::from_rect(
        Rect::from_xywh(
            pill.x as f32 * scale,
            pill.y as f32 * scale,
            pill.w as f32 * scale,
            pill.h as f32 * scale,
        )
        .unwrap(),
    );
    paint_solid(pixmap, &bg, [0, 0, 0, 1], false);
    let inset = 1.0;
    let (cx, cy) = (pill.mid_x() as f32 * scale, pill.mid_y() as f32 * scale);
    let vertical = pill.w < pill.h;
    let (rx, ry) = if vertical {
        (
            pill.w as f32 * scale / 2.0 - inset,
            pill.h as f32 * scale / 2.0,
        )
    } else {
        (
            pill.w as f32 * scale / 2.0,
            pill.h as f32 * scale / 2.0 - inset,
        )
    };
    if let Some(path) = capsule_px(cx - rx, cy - ry, rx * 2.0, ry * 2.0) {
        paint_solid(pixmap, &path, [255, 255, 255, 82], false);
    }
}

fn draw_cap(
    pixmap: &mut Pixmap,
    cap: &CapPlan,
    edge: Edge,
    metrics: &Metrics,
    rail: &R,
    scale: f32,
) {
    if !cap.hovered {
        return;
    }
    // The macOS ShowToggleCap: the silhouette itself filled white 0.11, masked to the
    // cap rect, so the highlight follows the flare instead of a hard rectangle.
    if let Some(mut mask) = tiny_skia::Mask::new(pixmap.width(), pixmap.height()) {
        let cap_px = Rect::from_xywh(
            cap.rect.x as f32 * scale,
            cap.rect.y as f32 * scale,
            cap.rect.w as f32 * scale,
            cap.rect.h as f32 * scale,
        )
        .unwrap();
        mask.fill_path(
            &PathBuilder::from_rect(cap_px),
            FillRule::Winding,
            false,
            Transform::identity(),
        );
        draw_silhouette_masked(
            pixmap,
            rail,
            edge,
            metrics,
            scale,
            CAP_HIGHLIGHT,
            Some(&mask),
        );
    }
    marks::pin(
        (
            (cap.rect.x + cap.rect.w * 0.62 - 6.0) as f32 * scale,
            (cap.rect.y + cap.rect.h * 0.62 - 6.0) as f32 * scale,
            12.0 * scale,
            12.0 * scale,
        ),
        // Bright while the rail is pinned open, dim while it waits behind the strip.
        if cap.pinned {
            [255, 255, 255, 242]
        } else {
            [255, 255, 255, 140]
        },
        pixmap,
    );
}

/// SwiftUI's `Capsule` in device pixels: a rectangle with half-round ends. Drawing
/// one as an oval instead pinches it to nothing away from the middle, which turns a
/// 4 pt meter bar into a hairline.
fn capsule_px(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    rounded_rect_path(
        R::new(f64::from(x), f64::from(y), f64::from(w), f64::from(h)),
        f64::from(w.min(h)) / 2.0,
        1.0,
    )
}

fn rounded_rect_path(rect: R, radius: f64, scale: f32) -> Option<Path> {
    let x = rect.x as f32 * scale;
    let y = rect.y as f32 * scale;
    let w = rect.w as f32 * scale;
    let h = rect.h as f32 * scale;
    let r = (radius as f32 * scale).min(w / 2.0).min(h / 2.0);
    if r <= 0.0 || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Strokes an arc centred at (cx, cy) sweeping clockwise from `from_deg`, 0 deg = up,
/// like SwiftUI's Circle().trim + rotationEffect(-90).
fn stroke_ring(
    pixmap: &mut Pixmap,
    cx: f32,
    cy: f32,
    radius: f32,
    stroke: f32,
    from_deg: f32,
    to_deg: f32,
    color: Color,
    round_caps: bool,
) {
    let n = (((to_deg - from_deg).abs() / 5.0).ceil() as usize).max(2);
    let mut pb = PathBuilder::new();
    for step in 0..=n {
        let t = step as f32 / n as f32;
        let angle = (from_deg + (to_deg - from_deg) * t).to_radians();
        let x = cx + radius * angle.cos();
        let y = cy + radius * angle.sin();
        if step == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    if let Some(path) = pb.finish() {
        let stroke_style = Stroke {
            width: stroke,
            line_cap: if round_caps {
                tiny_skia::LineCap::Round
            } else {
                tiny_skia::LineCap::Butt
            },
            ..Default::default()
        };
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
        paint.anti_alias = true;
        if let Some(stroked) = path.stroke(&stroke_style, 1.0) {
            pixmap.fill_path(
                &stroked,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

fn draw_cell(pixmap: &mut Pixmap, cell: &CellPlan, plan: &Plan, scale: f32) {
    let metrics = &plan.metrics;
    let icon_slot = metrics.icon_slot as f32 * scale;
    let ring_stroke = metrics.ring_stroke as f32 * scale;
    // The icon slot is centred horizontally in the cell whatever the edge.
    let center = (
        cell.rect.mid_x() as f32 * scale,
        cell.rect.y as f32 * scale + metrics.cell_padding as f32 * scale + icon_slot / 2.0,
    );
    let radius = (icon_slot - ring_stroke) / 2.0;

    stroke_ring(
        pixmap,
        center.0,
        center.1,
        radius,
        ring_stroke,
        0.0,
        360.0,
        TRACK_INK,
        true,
    );

    let glyph_size = metrics.glyph as f32 * scale;
    let glyph_rect = (
        center.0 - glyph_size / 2.0,
        center.1 - glyph_size / 2.0,
        glyph_size,
        glyph_size,
    );

    let (label, offset) = match &cell.content {
        CellContent::Provider {
            provider,
            primary,
            fable,
            label,
        } => {
            if let Some(primary) = primary {
                let percent = primary.percent.unwrap_or(0.0);
                stroke_ring(
                    pixmap,
                    center.0,
                    center.1,
                    radius,
                    ring_stroke,
                    -90.0,
                    -90.0 + percent.clamp(0.0, 100.0) as f32 * 3.6,
                    meter_color(primary.percent, provider_color(*provider)),
                    true,
                );
            }
            if let Some(fable) = fable {
                let fable_percent = fable.percent.unwrap_or(0.0);
                let inner_radius = radius - metrics.inner_ring_inset as f32 * scale;
                stroke_ring(
                    pixmap,
                    center.0,
                    center.1,
                    inner_radius,
                    metrics.inner_ring_stroke as f32 * scale,
                    0.0,
                    360.0,
                    FABLE_TRACK,
                    false,
                );
                stroke_ring(
                    pixmap,
                    center.0,
                    center.1,
                    inner_radius,
                    metrics.inner_ring_stroke as f32 * scale,
                    -90.0,
                    -90.0 + fable_percent.clamp(0.0, 100.0) as f32 * 3.6,
                    meter_color(fable.percent, FABLE_ORANGE),
                    true,
                );
            }
            marks::provider(*provider, glyph_rect, [255, 255, 255, 255], pixmap);
            (label.clone(), if primary.is_some() { 2.0 } else { 0.0 })
        }
        CellContent::PullRequests {
            segments,
            count,
            readable,
        } => {
            if *readable && !segments.is_empty() {
                let span = 360.0 / segments.len() as f32;
                let gap_deg = if segments.len() > 1 {
                    0.025 * 360.0
                } else {
                    0.0
                };
                for (index, ci) in segments.iter().enumerate() {
                    stroke_ring(
                        pixmap,
                        center.0,
                        center.1,
                        radius,
                        ring_stroke,
                        -90.0 + index as f32 * span + gap_deg / 2.0,
                        -90.0 + (index + 1) as f32 * span - gap_deg / 2.0,
                        ci_color(*ci),
                        false,
                    );
                }
            }
            marks::pull_request(glyph_rect, 1.6 * scale, [255, 255, 255, 255], pixmap);
            (
                if *readable {
                    count.to_string()
                } else {
                    "—".into()
                },
                0.0,
            )
        }
    };

    let label_size = 17.0 * plan.size_scale as f32 * scale;
    let baseline = cell.rect.y as f32 * scale
        + metrics.cell_padding as f32 * scale
        + icon_slot
        + metrics.content_spacing as f32 * scale
        + text::ascent_px(label_size, text::Weight::Medium);
    let measured = text::measure_px(&label, label_size, text::Weight::Medium);
    text::draw_px(
        pixmap,
        center.0 - measured / 2.0 + offset,
        baseline,
        &label,
        label_size,
        text::Weight::Medium,
        [255, 255, 255, 255],
    );
}

fn draw_popover(pixmap: &mut Pixmap, popover: &PopoverPlan, plan: &Plan, scale: f32) {
    let edge = plan.edge;
    let card = &popover.card;
    let tail_space = if vertical(edge) {
        TAIL_LENGTH * plan.size_scale
    } else {
        0.0
    };
    let tail_space_y = if vertical(edge) {
        0.0
    } else {
        TAIL_LENGTH * plan.size_scale
    };

    if let Some(path) = rounded_rect_path(*card, 12.0, scale) {
        paint_solid(pixmap, &path, POPOVER_INK, false);
        let mut paint = Paint::default();
        paint.set_color_rgba8(255, 255, 255, 28);
        paint.anti_alias = true;
        if let Some(stroked) = path.stroke(
            &Stroke {
                width: 1.0,
                ..Default::default()
            },
            1.0,
        ) {
            pixmap.fill_path(
                &stroked,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    // The tail: a triangle pointing from the card toward the rail.
    let half = (7.0 * plan.size_scale) as f32 * scale;
    let length = tail_space.max(tail_space_y) as f32 * scale;
    // `tail` is measured from the card's own leading edge, the way the mac popover
    // positions its tail inside the card's GeometryReader.
    let along = if vertical(edge) {
        (card.y + popover.tail) as f32 * scale
    } else {
        (card.x + popover.tail) as f32 * scale
    };
    let (tx, ty) = match edge {
        // The tail attaches to the card's screen-edge side and points at the rail.
        Edge::Right => ((card.x + card.w) as f32 * scale, along),
        Edge::Left => (card.x as f32 * scale, along),
        Edge::Top => (along, card.y as f32 * scale),
        Edge::Bottom => (along, (card.y + card.h) as f32 * scale),
    };
    let mut tail_pb = PathBuilder::new();
    match edge {
        Edge::Right => {
            tail_pb.move_to(tx, ty - half);
            tail_pb.line_to(tx + length, ty);
            tail_pb.line_to(tx, ty + half);
        }
        Edge::Left => {
            tail_pb.move_to(tx, ty - half);
            tail_pb.line_to(tx - length, ty);
            tail_pb.line_to(tx, ty + half);
        }
        Edge::Top => {
            tail_pb.move_to(tx - half, ty);
            tail_pb.line_to(tx, ty - length);
            tail_pb.line_to(tx + half, ty);
        }
        Edge::Bottom => {
            tail_pb.move_to(tx - half, ty);
            tail_pb.line_to(tx, ty + length);
            tail_pb.line_to(tx + half, ty);
        }
    }
    tail_pb.close();
    if let Some(path) = tail_pb.finish() {
        paint_solid(pixmap, &path, POPOVER_INK, false);
    }

    for (item, rect) in &popover.entries {
        let px = R::new(
            rect.x * scale as f64,
            rect.y * scale as f64,
            rect.w * scale as f64,
            rect.h * scale as f64,
        );
        match item {
            PopItem::Mark { kind, size } => {
                let device = (
                    px.x as f32,
                    px.y as f32,
                    *size as f32 * scale,
                    *size as f32 * scale,
                );
                match kind {
                    MarkKind::Provider(provider) => {
                        marks::provider(*provider, device, [255, 255, 255, 255], pixmap)
                    }
                    MarkKind::PullRequests => {
                        marks::pull_request(device, 1.4 * scale, [255, 255, 255, 255], pixmap)
                    }
                    MarkKind::OpenArrow => {
                        marks::open_arrow(device, 1.2 * scale, MUTED_INK, pixmap)
                    }
                }
            }
            PopItem::Text {
                text,
                size,
                weight,
                color,
                right,
            } => {
                let device_size = *size as f32 * scale;
                let weight = weight.face();
                let baseline = px.y as f32 + text::ascent_px(device_size, weight);
                if *right {
                    text::draw_px_right(
                        pixmap,
                        px.x as f32,
                        px.w as f32,
                        baseline,
                        text,
                        device_size,
                        weight,
                        *color,
                    );
                } else {
                    text::draw_px(
                        pixmap,
                        px.x as f32,
                        baseline,
                        text,
                        device_size,
                        weight,
                        *color,
                    );
                }
            }
            PopItem::Bar { percent, color } => {
                if let Some(path) = capsule_px(px.x as f32, px.y as f32, px.w as f32, px.h as f32) {
                    paint_solid(pixmap, &path, [255, 255, 255, 36], false);
                }
                if let Some(percent) = percent {
                    let fill_w = (px.w * percent.clamp(0.0, 100.0) / 100.0).max(px.h);
                    if let Some(path) =
                        capsule_px(px.x as f32, px.y as f32, fill_w as f32, px.h as f32)
                    {
                        paint_solid(pixmap, &path, *color, false);
                    }
                }
            }
            PopItem::Divider => {
                let divider = PathBuilder::from_rect(
                    Rect::from_xywh(px.x as f32, px.y as f32, px.w as f32, px.h as f32).unwrap(),
                );
                paint_solid(pixmap, &divider, [255, 255, 255, 26], false);
            }
            PopItem::Dot { ci } => {
                let mut dot_pb = PathBuilder::new();
                dot_pb.push_circle(px.mid_x() as f32, px.mid_y() as f32, px.w as f32 / 2.0);
                if let Some(path) = dot_pb.finish() {
                    if *ci == CiState::None {
                        marks::stroke(pixmap, &path, 1.0 * scale, [89, 89, 89, 255]);
                    } else {
                        paint_solid(pixmap, &path, ci_color(*ci), false);
                    }
                }
            }
            PopItem::CopyIcon => {
                marks::copy_icon(
                    (px.x as f32, px.y as f32, px.w as f32, px.h as f32),
                    1.2 * scale,
                    MUTED_INK,
                    pixmap,
                );
            }
        }
    }
    let _ = tail_space_y;
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod visual;
