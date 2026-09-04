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
    pub providers: Vec<AgentId>,
    pub pull_requests: NotchPullRequests,
}

impl Default for NotchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            display_id: None,
            edge: Edge::default(),
            size: NotchSize::default(),
            show: ShowMode::default(),
            providers: RAIL_ORDER.to_vec(),
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
    if vertical {
        if length > display.work_height {
            return None;
        }
        Some(Layout {
            x: match settings.edge {
                Edge::Left => display.x,
                _ => display.x + display.width - thickness,
            },
            y: display.work_y + (display.work_height - length) / 2.0,
            width: thickness,
            height: length,
        })
    } else {
        if length > display.width || thickness > display.work_height {
            return None;
        }
        Some(Layout {
            x: display.x + (display.width - length) / 2.0,
            y: match settings.edge {
                Edge::Top => display.work_y,
                _ => display.work_y + display.work_height - thickness,
            },
            width: length,
            height: thickness,
        })
    }
}

#[cfg(test)]
mod tests;
