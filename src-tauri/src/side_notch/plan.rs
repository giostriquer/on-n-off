mod cell;
mod popover;
mod text;

#[cfg(test)]
pub use text::FixedAdvance;
use text::Typesetter;
pub use text::{TextMetrics, TextWeight};

use super::model::frame::{CellData, RailData, Tone};
use super::model::{
    layout, pill_frame, pixel_aligned, rail_cell_frames, rail_metrics, selected_display, Color,
    Display, Edge, Layout, RailMetrics, ShowMode, TRIP_RED,
};
use crate::dto::AgentId;
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
const TAIL_LENGTH: f64 = 8.0;
const TAIL_HALF_WIDTH: f64 = 7.0;
const CARD_RADIUS: f64 = 12.0;
const PIN_SIZE: f64 = 12.0;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellId {
    Provider(AgentId),
    PullRequests,
}

impl CellId {
    pub fn of(cell: &CellData) -> Self {
        match cell {
            CellData::Provider(provider) => Self::Provider(provider.provider),
            CellData::PullRequests(_) => Self::PullRequests,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hover {
    pub rail_open: bool,
    pub active: Option<CellId>,
    pub cap_hovered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArcInk {
    Solid(Color),
    Conflict,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    pub centre: (f64, f64),
    pub radius: f64,
    pub stroke: f64,
    pub from: f64,
    pub to: f64,
    pub ink: ArcInk,
    pub round: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub size: f64,
    pub weight: TextWeight,
    pub color: Color,
    pub right: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellPlan {
    pub id: CellId,
    pub rect: R,
    pub active: bool,
    pub arcs: Vec<Arc>,
    pub glyph: R,
    pub label: Text,
    pub label_rect: R,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapPlan {
    pub rect: R,
    pub hovered: bool,
    pub pinned: bool,
    pub pin: R,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PopItem {
    Mark { kind: MarkKind, size: f64 },
    Text(Text),
    Bar { fill: Option<f64>, color: Color },
    Divider,
    Dot { ink: Color, hollow: bool },
    CopyIcon { color: Color },
    Stripe { color: Color },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Provider(AgentId),
    PullRequests,
    OpenArrow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Limits,
    PullRequests,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Zone {
    OpenRow { url: String },
    CopyRow { url: String, title: String },
    Footer(Page),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit<'a> {
    Cap,
    Cell(CellId),
    Popover(&'a Zone),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tail {
    pub base: [(f64, f64); 2],
    pub apex: (f64, f64),
}

impl Tail {
    fn translated(self, dx: f64, dy: f64) -> Self {
        let shift = |(x, y): (f64, f64)| (x + dx, y + dy);
        Tail {
            base: self.base.map(shift),
            apex: shift(self.apex),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopoverPlan {
    pub rect: R,
    pub card: R,
    pub radius: f64,
    pub tail: Tail,
    pub entries: Vec<(PopItem, R)>,
    pub zones: Vec<(Zone, R)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub edge: Edge,
    pub display_scale: f64,
    pub window: R,
    pub rail: R,
    pub metrics: RailMetrics,
    pub cells: Vec<CellPlan>,
    pub cap: CapPlan,
    pub pill: Option<R>,
    pub popover: Option<PopoverPlan>,
    frame: R,
}

impl Plan {
    pub fn moved_from(&self, earlier: &Plan) -> bool {
        self.frame != earlier.frame
            || self.cap.pinned != earlier.cap.pinned
            || !self
                .cells
                .iter()
                .map(|cell| cell.id)
                .eq(earlier.cells.iter().map(|cell| cell.id))
    }

    pub fn hit(&self, x: f64, y: f64) -> Option<Hit<'_>> {
        if self.cap.rect.contains(x, y) {
            return Some(Hit::Cap);
        }
        if let Some(cell) = self.cells.iter().find(|cell| cell.rect.contains(x, y)) {
            return Some(Hit::Cell(cell.id));
        }
        self.popover
            .as_ref()?
            .zones
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(zone, _)| Hit::Popover(zone))
    }
}

pub fn plan(
    displays: &[Display],
    data: &RailData,
    hover: Hover,
    now: DateTime<Utc>,
    text_metrics: &dyn TextMetrics,
) -> Option<Plan> {
    let settings = &data.settings;
    let rail = R::from(layout(settings, displays, data.cells.len())?);
    let display = selected_display(settings, displays)?;
    let edge = settings.edge;
    let scale = Scale {
        size: settings.size.scale(),
        display: display.scale,
    };
    let metrics = rail_metrics(settings.size, scale.display, edge);
    let collapsed = settings.show == ShowMode::OnHover && !hover.rail_open;

    let frames: Vec<R> = rail_cell_frames(edge, &metrics, data.cells.len())
        .into_iter()
        .map(|frame| R::from(frame).translated(rail.x, rail.y))
        .collect();

    let cap = if edge.is_vertical() {
        R::new(rail.x, rail.y, rail.w, metrics.ear())
    } else {
        R::new(rail.x, rail.y, metrics.ear(), rail.h)
    };

    let popover = hover.active.filter(|_| !collapsed).and_then(|id| {
        let index = data.cells.iter().position(|cell| CellId::of(cell) == id)?;
        let cell_rect = &frames[index];
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
            radius: scale.value(CARD_RADIUS),
            tail: tail_of(
                &card,
                edge,
                tail,
                scale.value(TAIL_LENGTH),
                scale.value(TAIL_HALF_WIDTH),
            ),
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

    let typeset = Typesetter::new(text_metrics, scale.display);
    let cells = frames
        .into_iter()
        .zip(&data.cells)
        .map(|(frame, cell)| {
            cell::cell(
                cell,
                frame.translated(-window.x, -window.y),
                hover.active == Some(CellId::of(cell)),
                &metrics.cell,
                scale,
                &typeset,
                now,
            )
        })
        .collect();
    let cap = cap.translated(-window.x, -window.y);

    Some(Plan {
        frame: rail,
        edge,
        display_scale: scale.display,
        window,
        rail: rail.translated(-window.x, -window.y),
        metrics,
        cells,
        cap: CapPlan {
            rect: cap,
            hovered: hover.cap_hovered,
            pinned: settings.show == ShowMode::Always,
            pin: R::new(
                cap.x + cap.w * 0.62 - PIN_SIZE / 2.0,
                cap.y + cap.h * 0.62 - PIN_SIZE / 2.0,
                PIN_SIZE,
                PIN_SIZE,
            ),
        },
        pill: pill.map(|pill| pill.translated(-window.x, -window.y)),
        popover: popover.map(|popover| translate_popover(popover, -window.x, -window.y)),
    })
}

fn tail_of(card: &R, edge: Edge, along: f64, length: f64, half: f64) -> Tail {
    let at = if edge.is_vertical() {
        card.y + along
    } else {
        card.x + along
    };
    match edge {
        Edge::Right => Tail {
            base: [(card.x + card.w, at - half), (card.x + card.w, at + half)],
            apex: (card.x + card.w + length, at),
        },
        Edge::Left => Tail {
            base: [(card.x, at - half), (card.x, at + half)],
            apex: (card.x - length, at),
        },
        Edge::Top => Tail {
            base: [(at - half, card.y), (at + half, card.y)],
            apex: (at, card.y - length),
        },
        Edge::Bottom => Tail {
            base: [(at - half, card.y + card.h), (at + half, card.y + card.h)],
            apex: (at, card.y + card.h + length),
        },
    }
}

fn translate_popover(mut popover: PopoverPlan, dx: f64, dy: f64) -> PopoverPlan {
    popover.rect = popover.rect.translated(dx, dy);
    popover.card = popover.card.translated(dx, dy);
    popover.tail = popover.tail.translated(dx, dy);
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
