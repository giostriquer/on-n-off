mod cell;
mod popover;
mod text;

#[cfg(test)]
pub use text::FixedAdvance;
use text::Typesetter;
pub use text::{TextMetrics, TextWeight};

use super::model::frame::{CellData, RailData, Tone};
use super::model::{
    layout, pill_frame, pixel_aligned, popover_metrics, rail_cell_frames, rail_metrics,
    selected_display, Color, Display, Edge, Layout, PopoverMetrics, RailMetrics, ShowMode,
    TRIP_RED,
};
use crate::dto::AgentId;
use chrono::{DateTime, TimeZone, Utc};
use std::fmt;

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
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
    fn within(&self, bounds: &R) -> Option<R> {
        let (left, top) = (self.x.max(bounds.x), self.y.max(bounds.y));
        let right = (self.x + self.w).min(bounds.x + bounds.w);
        let bottom = self.bottom().min(bounds.bottom());
        (right > left && bottom > top).then(|| R::new(left, top, right - left, bottom - top))
    }
}

impl From<Layout> for R {
    fn from(frame: Layout) -> Self {
        R::new(frame.x, frame.y, frame.width, frame.height)
    }
}

impl From<R> for Layout {
    fn from(rect: R) -> Self {
        Layout {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
        }
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub rect: R,
    pub radius: f64,
    pub width: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    pub rect: R,
    pub radius: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopoverPlan {
    pub rect: R,
    pub card: R,
    pub radius: f64,
    pub border: Border,
    pub clip: Option<Clip>,
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

pub fn plan<Tz: TimeZone>(
    displays: &[Display],
    data: &RailData,
    hover: Hover,
    now: DateTime<Tz>,
    text_metrics: &dyn TextMetrics,
) -> Option<Plan>
where
    Tz::Offset: fmt::Display,
{
    let zone = now.timezone();
    let now = now.with_timezone(&Utc);
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

    let opened = hover.active.filter(|_| !collapsed).and_then(|id| {
        let index = data.cells.iter().position(|cell| CellId::of(cell) == id)?;
        let cell = frames[index];
        let chrome = popover_metrics(settings.size, display);
        let contents = popover::contents(
            data.cells.get(index)?,
            data.action_error.as_deref(),
            &chrome,
            scale,
            text_metrics,
            now,
            &zone,
        );
        let content_h = contents.bottom() + chrome.padding;
        let tail = chrome.tail_length;
        let fitting = if edge.is_vertical() {
            (chrome.width + tail, content_h)
        } else {
            (chrome.width, content_h + tail)
        };
        let rect = chrome.frame(&cell.into(), edge, fitting);
        let card = R::from(chrome.card(&rect, edge));
        Some(Opened {
            cell,
            rect: rect.into(),
            card,
            contents,
            overflow: if content_h > chrome.max_height {
                content_h - card.h
            } else {
                0.0
            },
            chrome,
        })
    });

    let window = opened
        .as_ref()
        .map_or(rail, |opened| union(&rail, &opened.rect));
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
                &typeset,
                now,
            )
        })
        .collect();
    let cap = cap.translated(-window.x, -window.y);
    let pin = metrics.pin;
    let (pin_x, pin_y) = metrics.cap_pin(edge);

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
                cap.x + pin_x - pin / 2.0,
                cap.y + pin_y - pin / 2.0,
                pin,
                pin,
            ),
        },
        pill: pill.map(|pill| pill.translated(-window.x, -window.y)),
        popover: opened.map(|opened| opened.in_window(&window, edge)),
    })
}

struct Opened {
    cell: R,
    rect: R,
    card: R,
    contents: popover::Contents,
    overflow: f64,
    chrome: PopoverMetrics,
}

impl Opened {
    fn in_window(self, window: &R, edge: Edge) -> PopoverPlan {
        let card = self.card.translated(-window.x, -window.y);
        let cell = self.cell.translated(-window.x, -window.y);
        let (dx, dy) = (
            self.card.x - window.x,
            self.card.y - window.y - self.overflow,
        );
        let border = self.chrome.border;
        let radius = self.chrome.corner_radius;
        let inside = Clip {
            rect: R::new(
                card.x + border,
                card.y + border,
                card.w - 2.0 * border,
                card.h - 2.0 * border,
            ),
            radius: radius - border,
        };
        let clipped = self.overflow > 0.0;
        let shown = |rect: &R| rect.bottom() > inside.rect.y && rect.y < inside.rect.bottom();
        PopoverPlan {
            rect: self.rect.translated(-window.x, -window.y),
            card,
            radius,
            border: Border {
                rect: R::new(
                    card.x + border / 2.0,
                    card.y + border / 2.0,
                    card.w - border,
                    card.h - border,
                ),
                radius: radius - border / 2.0,
                width: border,
            },
            clip: clipped.then_some(inside),
            tail: tail_of(
                &card,
                edge,
                (cell.mid_x(), cell.mid_y()),
                self.chrome.tail_length,
                self.chrome.tail_half_width,
            ),
            entries: self
                .contents
                .entries
                .into_iter()
                .map(|(item, rect)| (item, rect.translated(dx, dy)))
                .filter(|(_, rect)| !clipped || shown(rect))
                .collect(),
            zones: self
                .contents
                .zones
                .into_iter()
                .filter_map(|(zone, rect)| {
                    let rect = rect.translated(dx, dy);
                    let rect = if clipped {
                        rect.within(&inside.rect)?
                    } else {
                        rect
                    };
                    Some((zone, rect))
                })
                .collect(),
        }
    }
}

fn tail_of(card: &R, edge: Edge, toward: (f64, f64), length: f64, half: f64) -> Tail {
    let at = if edge.is_vertical() {
        toward.1
    } else {
        toward.0
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
