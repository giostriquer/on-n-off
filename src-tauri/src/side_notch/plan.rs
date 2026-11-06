mod popover;
mod text;

#[cfg(test)]
pub use text::FixedAdvance;
pub use text::{TextMetrics, TextWeight};

use super::model::frame::{CellData, PrSegment, RailData, Tone};
use super::model::{
    layout, pill_frame, pixel_aligned, rail_cell_frames, rail_metrics, selected_display, Color,
    Display, Edge, InnerRing, Layout, NotchSettings, RailMetrics, ShowMode, TRIP_RED,
};
use crate::dto::{AgentId, CiState};
use crate::limits::window_at::{WindowAt, UNREADABLE};
use chrono::{DateTime, Utc};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct R {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
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

impl From<Layout> for R {
    fn from(frame: Layout) -> Self {
        R::new(frame.x, frame.y, frame.width, frame.height)
    }
}

fn union(a: &R, b: &R) -> R {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = (a.x + a.w).max(b.x + b.w);
    let bottom = (a.y + a.h).max(b.y + b.h);
    R::new(x, y, right - x, bottom - y)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Scale {
    size: f64,
    display: f64,
}

impl Scale {
    fn value(self, points: f64) -> f64 {
        pixel_aligned(points * self.size, self.display)
    }
}

const POPOVER_WIDTH: f64 = 272.0;
const POPOVER_MARGIN: f64 = 8.0;
pub const TAIL_LENGTH: f64 = 8.0;

fn popover_frame(cell: &R, edge: Edge, size: (f64, f64), display: &Display, size_scale: f64) -> R {
    let scale = Scale {
        size: size_scale,
        display: display.scale,
    };
    let gap = scale.value(2.0);
    let margin = scale.value(POPOVER_MARGIN);
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
        pixel_aligned(x, display.scale),
        pixel_aligned(y, display.scale),
        w,
        h,
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hover {
    pub rail_open: bool,
    pub active: Option<usize>,
    pub cap_hovered: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CellContent {
    Provider {
        provider: AgentId,
        headline: Option<WindowAt>,
        inner: Option<(InnerRing, WindowAt)>,
        label: String,
    },
    PullRequests {
        label: String,
        segments: Vec<PrSegment>,
    },
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

#[derive(Clone, Debug, PartialEq)]
pub enum PopItem {
    Mark {
        kind: MarkKind,
        size: f64,
    },
    Text {
        text: String,
        size: f64,
        weight: TextWeight,
        color: Color,
        right: bool,
    },
    Bar {
        percent: Option<f64>,
        color: Color,
    },
    Divider,
    Dot {
        ci: CiState,
    },
    CopyIcon {
        color: Color,
    },
    Stripe {
        color: Color,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Provider(AgentId),
    PullRequests,
    OpenArrow,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Zone {
    OpenRow { url: String },
    CopyRow { url: String, title: String },
    Footer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopoverPlan {
    pub rect: R,
    pub card: R,
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
    pub window: R,
    pub rail: R,
    pub metrics: RailMetrics,
    pub cells: Vec<CellPlan>,
    pub cap: CapPlan,
    pub pill: Option<R>,
    pub popover: Option<PopoverPlan>,
}

pub fn rail_hit(plan: &Plan, x: f64, y: f64) -> Option<usize> {
    plan.cells.iter().position(|cell| cell.rect.contains(x, y))
}

fn cell_content(data: &CellData, now: DateTime<Utc>) -> CellContent {
    match data {
        CellData::Provider(provider) => {
            let reading = provider.reading.as_ref();
            let headline = reading
                .and_then(|reading| reading.headline())
                .map(|window| WindowAt::of(window, now));
            CellContent::Provider {
                provider: provider.provider,
                headline,
                inner: reading
                    .and_then(|reading| reading.inner_at(now))
                    .map(|(ring, at)| (ring.clone(), at)),
                label: headline.map_or_else(|| UNREADABLE.into(), |at| at.used_text()),
            }
        }
        CellData::PullRequests(cell) => CellContent::PullRequests {
            label: cell.label.clone(),
            segments: cell
                .reading
                .as_ref()
                .map(|pulls| pulls.segments.clone())
                .unwrap_or_default(),
        },
    }
}

pub fn plan(
    settings: &NotchSettings,
    displays: &[Display],
    data: &RailData,
    hover: Hover,
    now: DateTime<Utc>,
    text_metrics: &dyn TextMetrics,
) -> Option<Plan> {
    let rail = R::from(layout(settings, displays, data.cells.len())?);
    let display = selected_display(settings, displays)?;
    let edge = settings.edge;
    let scale = Scale {
        size: settings.size.scale(),
        display: display.scale,
    };
    let metrics = rail_metrics(settings.size, scale.display, edge);
    let collapsed = settings.show == ShowMode::OnHover && !hover.rail_open;

    let cells: Vec<CellPlan> = rail_cell_frames(edge, &metrics, data.cells.len())
        .into_iter()
        .enumerate()
        .map(|(index, frame)| CellPlan {
            rect: R::from(frame).translated(rail.x, rail.y),
            content: cell_content(&data.cells[index], now),
            active: hover.active == Some(index),
        })
        .collect();

    let cap = CapPlan {
        rect: if edge.is_vertical() {
            R::new(rail.x, rail.y, rail.w, metrics.ear())
        } else {
            R::new(rail.x, rail.y, metrics.ear(), rail.h)
        },
        hovered: hover.cap_hovered,
        pinned: settings.show == ShowMode::Always,
    };

    let popover = hover.active.filter(|_| !collapsed).and_then(|index| {
        let cell_rect = &cells.get(index)?.rect;
        let card_w = scale.value(POPOVER_WIDTH);
        let padding = scale.value(12.0);
        let inner_w = card_w - 2.0 * padding;
        let tail_space = if edge.is_vertical() {
            scale.value(TAIL_LENGTH)
        } else {
            0.0
        };
        let contents = popover::contents(
            data.cells.get(index)?,
            data.action_error.as_deref(),
            inner_w,
            scale,
            text_metrics,
            now,
        );
        let card_h = contents.bottom() + padding;
        let height = card_h
            + if edge.is_vertical() {
                0.0
            } else {
                scale.value(TAIL_LENGTH)
            };
        let placed = popover_frame(
            cell_rect,
            edge,
            (card_w + tail_space, height),
            display,
            scale.size,
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
        let tail = if edge.is_vertical() {
            cell_rect.mid_y() - placed.y
        } else {
            cell_rect.mid_x() - placed.x
        };
        Some(PopoverPlan {
            rect: placed,
            card,
            tail,
            entries: contents
                .entries
                .into_iter()
                .map(|(item, rect)| (item, rect.translated(card.x, card.y)))
                .collect(),
            zones: contents
                .zones
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
    let pill = if collapsed {
        pill_frame(settings, displays, data.cells.len()).map(R::from)
    } else {
        None
    };

    Some(Plan {
        edge,
        display_scale: scale.display,
        size_scale: scale.size,
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

const WORKSPACE_CREDITS: &str = "Workspace credits";
pub const MUTED_INK: Color = [153, 153, 153, 255];
pub const LIVE_GREEN: Color = [74, 200, 120, 255];
pub const WARN_AMBER: Color = [224, 179, 65, 255];
const CLAUDE_ORANGE: Color = [217, 119, 87, 255];
const CODEX_INK: Color = [238, 240, 242, 255];
const CURSOR_BLUE: Color = [122, 162, 255, 255];
const ANTIGRAVITY_MUTE: Color = [140, 147, 157, 255];
const MUSE_BLUE: Color = [0, 130, 251, 255];
const WORK_BLUE: Color = [96, 165, 250, 255];
const PERSONAL_LAVENDER: Color = [170, 155, 215, 255];

fn tone_color(tone: Tone) -> Color {
    match tone {
        Tone::Muted => MUTED_INK,
        Tone::Green => LIVE_GREEN,
        Tone::Red => TRIP_RED,
        Tone::Amber => WARN_AMBER,
        Tone::Work => WORK_BLUE,
        Tone::Personal => PERSONAL_LAVENDER,
    }
}

pub fn provider_color(provider: AgentId) -> Color {
    match provider {
        AgentId::Claude => CLAUDE_ORANGE,
        AgentId::Codex => CODEX_INK,
        AgentId::Cursor => CURSOR_BLUE,
        AgentId::Antigravity => ANTIGRAVITY_MUTE,
        AgentId::Muse => MUSE_BLUE,
    }
}

#[cfg(test)]
pub(super) mod tests;
