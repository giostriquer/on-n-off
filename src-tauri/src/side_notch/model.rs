use crate::dto::AgentId;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
use crate::dto::{
    GithubPrDto, LimitWindowDto, LimitWindowKind, LimitsWorkspaceCreditsDto, ProviderLimitsDto,
    ReadStanding, WindowRole,
};
#[cfg(any(target_os = "windows", test))]
use crate::limits::window_at::{Meter, WindowAt};
use serde::{Deserialize, Serialize};

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub mod frame;

pub use crate::provider_catalogue::RAIL_ORDER;

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
    pub fn scale(self) -> f64 {
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
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShowMode {
    #[default]
    Always,
    OnHover,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NotchStyle {
    #[default]
    Glass,
    Solid,
}

#[cfg(any(target_os = "macos", test))]
pub fn liquid_glass_on(macos_major: isize) -> bool {
    macos_major >= 26
}

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
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn selected_lists(&self) -> Vec<GithubList> {
        GITHUB_LIST_ORDER
            .into_iter()
            .filter(|list| self.lists.contains(list))
            .collect()
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrContext {
    Work,
    Personal,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
impl PrContext {
    pub fn of(pr: &GithubPrDto) -> Option<Self> {
        pr.in_organization.map(|organization| {
            if organization {
                Self::Work
            } else {
                Self::Personal
            }
        })
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
    pub popover_style: NotchStyle,
}

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
            providers: crate::provider_catalogue::WITH_LIMITS.to_vec(),
            pull_requests: NotchPullRequests::default(),
            popover_style: NotchStyle::default(),
        }
    }
}

impl NotchSettings {
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn rail_providers(&self) -> Vec<AgentId> {
        RAIL_ORDER
            .into_iter()
            .filter(|agent| self.providers.contains(agent))
            .collect()
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
    pub revision: u64,
    pub supported: bool,
    pub liquid_glass: bool,
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

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub struct RailMetrics {
    pub thickness: f64,
    pub cell_length: f64,
    pub cell_spacing: f64,
    pub inset: f64,
    #[cfg(any(target_os = "windows", test))]
    pub cell: CellMetrics,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg(any(target_os = "windows", test))]
pub struct CellMetrics {
    pub padding: f64,
    pub content_spacing: f64,
    pub icon_slot: f64,
    pub label_height: f64,
    pub ring_stroke: f64,
    pub inner_ring_stroke: f64,
    pub inner_ring_inset: f64,
    pub glyph: f64,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
impl RailMetrics {
    pub fn length(&self, count: usize) -> f64 {
        let count = count as f64;
        count * self.cell_length + (count - 1.0).max(0.0) * self.cell_spacing + 2.0 * self.inset
    }

    #[cfg(any(target_os = "windows", test))]
    pub fn ear(&self) -> f64 {
        self.inset
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn rail_metrics(size: NotchSize, display_scale: f64, edge: Edge) -> RailMetrics {
    let value = |points: f64| pixel_aligned(points * size.scale(), display_scale);
    let cell_padding = value(1.0);
    let content_spacing = value(3.0);
    let icon_slot = value(46.0);
    let label_height = value(22.0);
    let cell_width = value(76.0);
    let cell_height = icon_slot + label_height + content_spacing + 2.0 * cell_padding;
    let vertical = edge.is_vertical();
    RailMetrics {
        thickness: if vertical { cell_width } else { cell_height },
        cell_length: if vertical { cell_height } else { cell_width },
        cell_spacing: value(8.0),
        inset: value(40.0),
        #[cfg(any(target_os = "windows", test))]
        cell: CellMetrics {
            padding: cell_padding,
            content_spacing,
            icon_slot,
            label_height,
            ring_stroke: value(4.0),
            inner_ring_stroke: value(3.0),
            inner_ring_inset: value(6.0),
            glyph: value(24.0),
        },
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn selected_display<'a>(
    settings: &NotchSettings,
    displays: &'a [Display],
) -> Option<&'a Display> {
    let id = settings.display_id.as_deref()?;
    let mut matches = displays.iter().filter(|display| display.id == id);
    let display = matches.next()?;
    if matches.next().is_some() || display.mirrored {
        return None;
    }
    Some(display)
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn layout(settings: &NotchSettings, displays: &[Display], cells: usize) -> Option<Layout> {
    if !settings.enabled || cells == 0 {
        return None;
    }
    let display = selected_display(settings, displays)?;
    let metrics = rail_metrics(settings.size, display.scale, settings.edge);
    let vertical = settings.edge.is_vertical();
    let thickness = metrics.thickness;
    let length = metrics.length(cells);
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

#[cfg(any(target_os = "windows", test))]
pub fn rail_cell_frames(edge: Edge, metrics: &RailMetrics, count: usize) -> Vec<Layout> {
    (0..count)
        .map(|index| {
            let offset =
                metrics.inset + index as f64 * (metrics.cell_length + metrics.cell_spacing);
            if edge.is_vertical() {
                Layout {
                    x: 0.0,
                    y: offset,
                    width: metrics.thickness,
                    height: metrics.cell_length,
                }
            } else {
                Layout {
                    x: offset,
                    y: 0.0,
                    width: metrics.cell_length,
                    height: metrics.thickness,
                }
            }
        })
        .collect()
}

#[cfg(any(target_os = "windows", test))]
pub fn pill_frame(settings: &NotchSettings, displays: &[Display], cells: usize) -> Option<Layout> {
    let rail = layout(settings, displays, cells)?;
    let display = selected_display(settings, displays)?;
    let value = |points: f64| pixel_aligned(points * settings.size.scale(), display.scale);
    let thickness = value(6.0);
    let length = value(120.0).min(if settings.edge.is_vertical() {
        rail.height
    } else {
        rail.width
    });
    let mid_x = rail.x + rail.width / 2.0;
    let mid_y = rail.y + rail.height / 2.0;
    let (x, y, width, height) = match settings.edge {
        Edge::Left => (rail.x, mid_y - length / 2.0, thickness, length),
        Edge::Right => (
            rail.x + rail.width - thickness,
            mid_y - length / 2.0,
            thickness,
            length,
        ),
        Edge::Top => (mid_x - length / 2.0, rail.y, length, thickness),
        Edge::Bottom => (
            mid_x - length / 2.0,
            rail.y + rail.height - thickness,
            length,
            thickness,
        ),
    };
    Some(Layout {
        x,
        y,
        width,
        height,
    })
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn pixel_aligned(value: f64, display_scale: f64) -> f64 {
    let scale = if display_scale.is_finite() && display_scale > 0.0 {
        display_scale
    } else {
        1.0
    };
    (value * scale).round() / scale
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq)]
pub struct NotchProvider {
    standing: ReadStanding,
    pub windows: Vec<LimitWindowDto>,
    headline_window_id: Option<String>,
    inner_ring: Option<InnerRing>,
    pub workspace_credits: Option<LimitsWorkspaceCreditsDto>,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InnerRing {
    #[serde(rename_all = "camelCase")]
    Fable {
        window_id: String,
    },
    WorkspaceShare,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
impl NotchProvider {
    pub fn current(entries: Vec<ProviderLimitsDto>) -> Option<Self> {
        let card = entries.into_iter().find(|entry| entry.current_account)?;
        let standing = ReadStanding::of(&card);
        let windows = card.reading.windows;
        let workspace_credits = card.reading.workspace_credits;
        let headline_window_id = windows
            .iter()
            .find(|window| window.kind == LimitWindowKind::Weekly)
            .map(|window| window.id.clone());
        let inner_ring = inner_ring(&windows, workspace_credits.as_ref());
        Some(Self {
            standing,
            windows,
            headline_window_id,
            inner_ring,
            workspace_credits,
        })
    }
}

#[cfg(any(target_os = "macos", test))]
impl NotchProvider {
    pub fn headline_window_id(&self) -> Option<&str> {
        self.headline_window_id.as_deref()
    }

    pub fn inner_ring(&self) -> Option<&InnerRing> {
        self.inner_ring.as_ref()
    }
}

#[cfg(any(target_os = "windows", test))]
impl NotchProvider {
    pub fn headline(&self) -> Option<&LimitWindowDto> {
        let id = self.headline_window_id.as_deref()?;
        self.windows.iter().find(|window| window.id == id)
    }

    pub fn inner_at(&self, now: chrono::DateTime<chrono::Utc>) -> Option<(&InnerRing, WindowAt)> {
        let ring = self.inner_ring.as_ref()?;
        let at = match ring {
            InnerRing::Fable { window_id } => self
                .windows
                .iter()
                .find(|window| &window.id == window_id)
                .map(|window| WindowAt::of(window, now)),
            InnerRing::WorkspaceShare => self
                .workspace_credits
                .as_ref()
                .map(|share| WindowAt::of_share(share, now)),
        }?;
        Some((ring, at))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn inner_ring(
    windows: &[LimitWindowDto],
    share: Option<&LimitsWorkspaceCreditsDto>,
) -> Option<InnerRing> {
    windows
        .iter()
        .find(|window| window.role == Some(WindowRole::InnerRing))
        .map(|window| InnerRing::Fable {
            window_id: window.id.clone(),
        })
        .or_else(|| share.map(|_| InnerRing::WorkspaceShare))
}

#[cfg(any(target_os = "windows", test))]
pub type Color = [u8; 4];

#[cfg(any(target_os = "windows", test))]
pub const TRIP_RED: Color = [226, 89, 76, 255];
#[cfg(any(target_os = "windows", test))]
pub const UNREADABLE_INK: Color = [77, 77, 77, 255];

#[cfg(any(target_os = "windows", test))]
pub fn meter_color(meter: Meter, base: Color) -> Color {
    match meter {
        Meter::Unreadable => UNREADABLE_INK,
        Meter::Base => base,
        Meter::Blend(fraction) => mix(base, TRIP_RED, fraction),
        Meter::Trip => TRIP_RED,
    }
}

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

#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareWording {
    pub left: String,
    pub renewed: String,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn workspace_share_wording(share: &LimitsWorkspaceCreditsDto) -> ShareWording {
    let limit = amount(&share.limit);
    let used = amount(&share.used);
    let left = if !share.reached {
        format!(
            "{} of {} left",
            format_amount((limit - used).max(0.0)),
            format_amount(limit)
        )
    } else if used >= limit {
        format!("all {} used", format_amount(limit))
    } else {
        "limit reached".into()
    };
    ShareWording {
        left,
        renewed: format!("{} of {} left", format_amount(limit), format_amount(limit)),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn amount(text: &str) -> f64 {
    text.trim().parse().unwrap_or(0.0)
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn format_amount(value: f64) -> String {
    let cents = (value.max(0.0) * 100.0).round() as u128;
    let whole = (cents / 100).to_string();
    let mut grouped = String::new();
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    match cents % 100 {
        0 => grouped,
        fraction if fraction.is_multiple_of(10) => format!("{grouped}.{}", fraction / 10),
        fraction => format!("{grouped}.{fraction:02}"),
    }
}

#[cfg(test)]
mod tests;
