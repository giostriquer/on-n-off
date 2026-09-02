use crate::dto::AgentId;
use serde::{Deserialize, Serialize};

/// A cell's width on screen (points at the standard size); a vertical rail is this thick.
#[cfg(any(target_os = "macos", test))]
pub const CELL_WIDTH: f64 = 76.0;
/// A cell's height on screen, from the same parts the helper's `railLayout` adds up (icon slot,
/// gap, percent label, padding); a horizontal bar is this thick.
#[cfg(any(target_os = "macos", test))]
pub const CELL_HEIGHT: f64 = ICON_SLOT + CONTENT_SPACING + LABEL_HEIGHT + 2.0 * CELL_PADDING;
#[cfg(any(target_os = "macos", test))]
const ICON_SLOT: f64 = 46.0;
#[cfg(any(target_os = "macos", test))]
const CONTENT_SPACING: f64 = 3.0;
#[cfg(any(target_os = "macos", test))]
const LABEL_HEIGHT: f64 = 22.0;
#[cfg(any(target_os = "macos", test))]
const CELL_PADDING: f64 = 1.0;
#[cfg(any(target_os = "macos", test))]
pub const CELL_SPACING: f64 = 8.0;
#[cfg(any(target_os = "macos", test))]
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
    #[cfg(any(target_os = "macos", test))]
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
    #[cfg(any(target_os = "macos", test))]
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
    pub fn rail_providers(&self) -> Vec<AgentId> {
        RAIL_ORDER
            .into_iter()
            .filter(|agent| self.providers.contains(agent))
            .collect()
    }

    /// Cells on the rail: one per selected provider, then the pull-request cell when it is on.
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
#[cfg(any(target_os = "macos", test))]
pub struct Layout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Length of the rail along its axis for `count` cells of `cell` length, before size scaling.
#[cfg(any(target_os = "macos", test))]
pub fn rail_length(count: usize, cell: f64) -> f64 {
    let count = count as f64;
    count * cell + (count - 1.0).max(0.0) * CELL_SPACING + 2.0 * RAIL_INSET
}

/// The rail's frame in top-left display coordinates, or `None` when the notch must stay hidden:
/// disabled, no provider, the selected display missing, ambiguous, or mirrored, or a rail that
/// does not fit inside the display's work area.
#[cfg(any(target_os = "macos", test))]
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
mod tests {
    use super::*;

    fn display(id: &str, x: f64, scale: f64) -> Display {
        Display {
            id: id.into(),
            name: "Same monitor name".into(),
            x,
            y: 0.0,
            width: 1728.0,
            height: 1117.0,
            work_y: 33.0,
            work_height: 1084.0,
            scale,
            mirrored: false,
        }
    }

    fn settings(id: &str, edge: Edge) -> NotchSettings {
        NotchSettings {
            enabled: true,
            display_id: Some(id.into()),
            edge,
            pull_requests: NotchPullRequests {
                enabled: false,
                lists: vec![GithubList::Mine],
            },
            ..NotchSettings::default()
        }
    }

    #[test]
    fn follows_the_selected_uuid_across_reordering_and_mixed_scales() {
        let displays = vec![
            display("external", 0.0, 1.0),
            display("retina", -1728.0, 2.0),
        ];
        // Four cells: 4 × 73 + 3 × 8 + 2 × 40 = 396.
        assert_eq!(
            layout(&settings("retina", Edge::Right), &displays),
            Some(Layout {
                x: -76.0,
                y: 377.0,
                width: 76.0,
                height: 396.0
            })
        );
        let reversed: Vec<_> = displays.into_iter().rev().collect();
        assert_eq!(
            layout(&settings("retina", Edge::Left), &reversed),
            Some(Layout {
                x: -1728.0,
                y: 377.0,
                width: 76.0,
                height: 396.0
            })
        );
    }

    #[test]
    fn never_falls_back_to_another_display_when_the_selection_is_missing_or_mirrored() {
        let mut current = settings("missing", Edge::Left);
        let mut displays = vec![display("other", 0.0, 1.0)];
        assert_eq!(layout(&current, &displays), None);
        current.display_id = Some("other".into());
        displays[0].mirrored = true;
        assert_eq!(layout(&current, &displays), None);
    }

    #[test]
    fn top_and_bottom_edges_center_a_horizontal_rail_inside_the_work_area() {
        let displays = [display("main", 100.0, 2.0)];
        // Four cells side by side: 4 × 76 + 3 × 8 + 2 × 40 = 408, as tall as a cell.
        let top = layout(&settings("main", Edge::Top), &displays).unwrap();
        assert_eq!(
            top,
            Layout {
                x: 100.0 + (1728.0 - 408.0) / 2.0,
                y: 33.0,
                width: 408.0,
                height: 73.0
            }
        );
        let bottom = layout(&settings("main", Edge::Bottom), &displays).unwrap();
        assert_eq!(bottom.y, 33.0 + 1084.0 - 73.0);
        assert_eq!(bottom.x, top.x);
    }

    #[test]
    fn the_rail_shrinks_with_fewer_providers_and_hides_without_any() {
        let displays = [display("main", 0.0, 1.0)];
        let mut current = settings("main", Edge::Right);
        current.providers = vec![AgentId::Cursor, AgentId::Claude, AgentId::Claude];
        assert_eq!(current.rail_providers(), [AgentId::Claude, AgentId::Cursor]);
        assert_eq!(layout(&current, &displays).unwrap().height, 234.0);
        current.providers.clear();
        assert_eq!(layout(&current, &displays), None);
    }

    #[test]
    fn a_rail_that_does_not_fit_stays_hidden_instead_of_overflowing() {
        let mut small = display("small", 0.0, 1.0);
        small.work_height = 300.0;
        assert_eq!(
            layout(&settings("small", Edge::Right), &[small.clone()]),
            None
        );
        small.width = 300.0;
        small.work_height = 1000.0;
        assert_eq!(layout(&settings("small", Edge::Top), &[small]), None);
    }

    #[test]
    fn the_pull_request_cell_is_on_by_default_with_only_the_users_own_list() {
        let defaults = NotchSettings::default();
        assert!(defaults.pull_requests.enabled);
        assert_eq!(defaults.pull_requests.lists, [GithubList::Mine]);
        assert_eq!(defaults.cell_count(), 5);
        let legacy: NotchSettings =
            serde_json::from_str(r#"{"enabled":true,"displayId":"main","edge":"right"}"#).unwrap();
        assert_eq!(legacy.cell_count(), 5, "older documents gain the cell");
        let mut current = settings("main", Edge::Right);
        current.pull_requests = NotchPullRequests {
            enabled: true,
            lists: vec![GithubList::Assigned, GithubList::Mine, GithubList::Assigned],
        };
        assert_eq!(
            current.pull_requests.selected_lists(),
            [GithubList::Mine, GithubList::Assigned]
        );
        let displays = [display("main", 0.0, 1.0)];
        assert_eq!(
            layout(&current, &displays).unwrap().height,
            rail_length(5, CELL_HEIGHT)
        );
        let json = serde_json::to_value(&current).unwrap();
        assert_eq!(json["pullRequests"]["enabled"], true);
        assert_eq!(json["pullRequests"]["lists"][0], "assigned");
    }

    #[test]
    fn remains_hidden_until_explicitly_enabled() {
        assert_eq!(
            layout(&NotchSettings::default(), &[display("main", 0.0, 1.0)]),
            None
        );
    }

    #[test]
    fn legacy_documents_default_to_always_show_all_providers_and_the_standard_size() {
        let legacy: NotchSettings =
            serde_json::from_str(r#"{"enabled":true,"displayId":"main","edge":"right"}"#).unwrap();
        assert_eq!(legacy.size, NotchSize::Standard);
        assert_eq!(legacy.show, ShowMode::Always);
        assert_eq!(legacy.providers, RAIL_ORDER.to_vec());
        let hover: NotchSettings = serde_json::from_str(
            r#"{"enabled":true,"displayId":"main","edge":"top","show":"onHover","providers":["codex"]}"#,
        )
        .unwrap();
        assert_eq!(hover.show, ShowMode::OnHover);
        assert_eq!(hover.edge, Edge::Top);
        assert_eq!(hover.providers, vec![AgentId::Codex]);
        let json = serde_json::to_value(&hover).unwrap();
        assert_eq!(json["show"], "onHover");
        assert_eq!(json["edge"], "top");
    }

    #[test]
    fn presets_scale_the_whole_rail() {
        let displays = [display("main", 0.0, 1.0)];
        for (size, scale) in [
            (NotchSize::Compact, 0.875),
            (NotchSize::Standard, 1.0),
            (NotchSize::Large, 1.125),
        ] {
            let mut current = settings("main", Edge::Right);
            current.size = size;
            let rail = layout(&current, &displays).unwrap();
            assert_eq!(rail.width, CELL_WIDTH * scale);
            assert_eq!(rail.height, rail_length(4, CELL_HEIGHT) * scale);
        }
    }
}
