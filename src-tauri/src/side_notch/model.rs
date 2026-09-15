use crate::dto::AgentId;
use serde::{Deserialize, Serialize};

/// A cell's width on screen (points at the standard size); a vertical rail is this thick.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub const CELL_WIDTH: f64 = 76.0;
/// A cell's height on screen, from the same parts the helper's `railLayout` adds up (icon slot,
/// gap, percent label, padding); a horizontal bar is this thick.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub const CELL_HEIGHT: f64 = ICON_SLOT + CONTENT_SPACING + LABEL_HEIGHT + 2.0 * CELL_PADDING;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
const ICON_SLOT: f64 = 46.0;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
const CONTENT_SPACING: f64 = 3.0;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
const LABEL_HEIGHT: f64 = 22.0;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
const CELL_PADDING: f64 = 1.0;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub const CELL_SPACING: f64 = 8.0;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
/// Also the length of the ear curve that flares each end into the screen edge.
pub const RAIL_INSET: f64 = 40.0;

/// Providers in the order the rail lays them out.
pub const RAIL_ORDER: [AgentId; 4] = [
    AgentId::Claude,
    AgentId::Codex,
    AgentId::Antigravity,
    AgentId::Cursor,
];

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NotchSize {
    Compact,
    #[default]
    Standard,
    Large,
}

impl NotchSize {
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    fn scale(self) -> f64 {
        match self {
            Self::Compact => 0.875,
            Self::Standard => 1.0,
            Self::Large => 1.125,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Edge {
    Left,
    #[default]
    Right,
    Top,
    Bottom,
}

impl Edge {
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    fn is_vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// Whether the rail stays open or waits behind a small pill at the edge.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShowMode {
    #[default]
    Always,
    OnHover,
}

/// The Pull requests screen's three lists, in the order the popover shows them.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GithubList {
    Mine,
    ReviewRequested,
    Assigned,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub const GITHUB_LIST_ORDER: [GithubList; 3] = [
    GithubList::Mine,
    GithubList::ReviewRequested,
    GithubList::Assigned,
];

/// The pull-request cell: on by default, showing only the user's own pull requests.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct NotchPullRequests {
    pub enabled: bool,
    pub lists: Vec<GithubList>,
}

impl Default for NotchPullRequests {
    fn default() -> Self {
        Self {
            enabled: true,
            lists: vec![GithubList::Mine],
        }
    }
}

impl NotchPullRequests {
    /// The selected lists in screen order, without duplicates.
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn selected_lists(&self) -> Vec<GithubList> {
        GITHUB_LIST_ORDER
            .into_iter()
            .filter(|list| self.lists.contains(list))
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct NotchSettings {
    pub enabled: bool,
    pub display_id: Option<String>,
    pub edge: Edge,
    pub size: NotchSize,
    pub show: ShowMode,
    #[serde(default = "documented_providers")]
    pub providers: Vec<AgentId>,
    pub pull_requests: NotchPullRequests,
}

/// What a settings document written before the notch had a provider list meant: every
/// provider. A fresh install starts narrower (see `Default`), but nobody who already
/// has a rail loses cells from it.
fn documented_providers() -> Vec<AgentId> {
    RAIL_ORDER.to_vec()
}

impl Default for NotchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            display_id: None,
            edge: Edge::default(),
            size: NotchSize::default(),
            show: ShowMode::default(),
            // Only the providers that publish a subscription quota worth a ring.
            // Antigravity has none and Cursor only reports one on some setups, so a
            // first run would rail two dashes; both are one toggle away in settings.
            providers: vec![AgentId::Claude, AgentId::Codex],
            pull_requests: NotchPullRequests::default(),
        }
    }
}

impl NotchSettings {
    /// The selected providers in rail order, without duplicates.
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn rail_providers(&self) -> Vec<AgentId> {
        RAIL_ORDER
            .into_iter()
            .filter(|agent| self.providers.contains(agent))
            .collect()
    }

    /// Cells on the rail: one per selected provider, then the pull-request cell when it is on.
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn cell_count(&self) -> usize {
        self.rail_providers().len() + usize::from(self.pull_requests.enabled)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub work_y: f64,
    pub work_height: f64,
    pub scale: f64,
    pub mirrored: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NotchSnapshot {
    /// Orders settings reads, writes, and window events within this app run.
    pub revision: u64,
    pub supported: bool,
    pub settings: NotchSettings,
    pub displays: Vec<Display>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub struct Layout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Length of the rail along its axis for `count` cells of `cell` length, before size scaling.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn rail_length(count: usize, cell: f64) -> f64 {
    let count = count as f64;
    count * cell + (count - 1.0).max(0.0) * CELL_SPACING + 2.0 * RAIL_INSET
}

/// The rail's frame in top-left display coordinates, or `None` when the notch must stay hidden:
/// disabled, no provider, the selected display missing, ambiguous, or mirrored, or a rail that
/// does not fit inside the display's work area.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn layout(settings: &NotchSettings, displays: &[Display]) -> Option<Layout> {
    if !settings.enabled {
        return None;
    }
    let count = settings.cell_count();
    if count == 0 {
        return None;
    }
    let id = settings.display_id.as_deref()?;
    let mut matches = displays.iter().filter(|display| display.id == id);
    let display = matches.next()?;
    if matches.next().is_some() || display.mirrored {
        return None;
    }
    let scale = settings.size.scale();
    let vertical = settings.edge.is_vertical();
    let thickness = if vertical { CELL_WIDTH } else { CELL_HEIGHT } * scale;
    let length = rail_length(count, if vertical { CELL_HEIGHT } else { CELL_WIDTH }) * scale;
    let aligned = |value: f64| pixel_aligned(value, display.scale);
    if vertical {
        if length > display.work_height {
            return None;
        }
        Some(Layout {
            x: aligned(match settings.edge {
                Edge::Left => display.x,
                _ => display.x + display.width - thickness,
            }),
            y: aligned(display.work_y + (display.work_height - length) / 2.0),
            width: thickness,
            height: length,
        })
    } else {
        if length > display.width || thickness > display.work_height {
            return None;
        }
        Some(Layout {
            x: aligned(display.x + (display.width - length) / 2.0),
            y: aligned(match settings.edge {
                Edge::Top => display.work_y,
                _ => display.work_y + display.work_height - thickness,
            }),
            width: length,
            height: thickness,
        })
    }
}

/// A point snapped to the display's pixel grid, the `pixelAligned` port. Centring the
/// rail in a work area that does not divide evenly leaves it on a half pixel, and the
/// window origin moves with the popover: without this the whole rail slides half a
/// pixel whenever a popover opens above its top edge.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn pixel_aligned(value: f64, display_scale: f64) -> f64 {
    let scale = if display_scale.is_finite() && display_scale > 0.0 {
        display_scale
    } else {
        1.0
    };
    (value * scale).round() / scale
}

/// The side notch meter ramp, twin of `NotchCore/Meter.swift`.
///
/// It used to step to a warning amber at 70 %. That amber is far lighter than the accents it
/// replaced and its hue points away from red, so a meter that was filling up went paler and
/// yellower exactly as it ran out, which reads as cooling down. Interpolating the accent toward the
/// trip red keeps the ramp monotonic: every step sits closer to red than the one before it. The
/// blend is eased rather than linear so crossing 70 % announces itself instead of creeping.
///
/// It lives here rather than in `win_paint.rs` for the reason the layout maths does: that file has
/// no test target, so a ramp kept there is tested on neither CI leg. The gate is `windows` plus
/// `test` rather than both platforms, because the painter is the only consumer and an item cfg'd
/// into a target that never calls it fails `-D warnings` as dead code. Under `test` it compiles on
/// both legs, which is what lets one regression test cover the Windows ramp from either runner.
/// The amber still means "pending" on CI rollups and badges, so it stays in the painter palette.
#[cfg(any(target_os = "windows", test))]
pub type Color = [u8; 4];

#[cfg(any(target_os = "windows", test))]
pub const TRIP_RED: Color = [226, 89, 76, 255];
#[cfg(any(target_os = "windows", test))]
pub const UNREADABLE_INK: Color = [77, 77, 77, 255];

#[cfg(any(target_os = "windows", test))]
pub fn meter_color(percent: Option<f64>, base: Color) -> Color {
    match percent {
        None => UNREADABLE_INK,
        Some(percent) if percent >= 90.0 => TRIP_RED,
        Some(percent) if percent > 70.0 => mix(base, TRIP_RED, ((percent - 70.0) / 20.0).sqrt()),
        Some(_) => base,
    }
}

/// Two palette entries blended in sRGB, `amount` clamped to 0...1. Alpha is blended with the rest
/// rather than taken from `from`, so the helper is right for any pair and not only for the opaque
/// ramp colours it is used on today.
#[cfg(any(target_os = "windows", test))]
fn mix(from: Color, to: Color, amount: f64) -> Color {
    let t = amount.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
    [
        channel(from[0], to[0]),
        channel(from[1], to[1]),
        channel(from[2], to[2]),
        channel(from[3], to[3]),
    ]
}

#[cfg(test)]
mod tests;
