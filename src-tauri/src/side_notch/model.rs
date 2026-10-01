use crate::dto::AgentId;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
use crate::dto::{
    LimitWindowDto, LimitWindowKind, LimitsStatus, LimitsWorkspaceCreditsDto, ProviderLimitsDto,
};
use serde::{Deserialize, Serialize};

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub const CELL_WIDTH: f64 = 76.0;
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
pub const RAIL_INSET: f64 = 40.0;

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

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShowMode {
    #[default]
    Always,
    OnHover,
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
            providers: vec![AgentId::Claude, AgentId::Codex],
            pull_requests: NotchPullRequests::default(),
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

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn rail_length(count: usize, cell: f64) -> f64 {
    let count = count as f64;
    count * cell + (count - 1.0).max(0.0) * CELL_SPACING + 2.0 * RAIL_INSET
}

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

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn pixel_aligned(value: f64, display_scale: f64) -> f64 {
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
    pub provider: AgentId,
    pub status: LimitsStatus,
    pub message: Option<String>,
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
        let windows = card.reading.windows;
        let workspace_credits = card.reading.workspace_credits;
        let headline_window_id = windows
            .iter()
            .find(|window| window.kind == LimitWindowKind::Weekly)
            .map(|window| window.id.clone());
        let inner_ring = inner_ring(card.provider, &windows, workspace_credits.as_ref());
        Some(Self {
            provider: card.provider,
            status: card.status,
            message: card.message,
            windows,
            headline_window_id,
            inner_ring,
            workspace_credits,
        })
    }
}

#[cfg(target_os = "macos")]
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

    pub fn inner_window(&self) -> Option<(&InnerRing, LimitWindowDto)> {
        let ring = self.inner_ring.as_ref()?;
        let window = match ring {
            InnerRing::Fable { window_id } => self
                .windows
                .iter()
                .find(|window| &window.id == window_id)
                .cloned(),
            InnerRing::WorkspaceShare => {
                self.workspace_credits.as_ref().map(workspace_share_window)
            }
        }?;
        Some((ring, window))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn inner_ring(
    provider: AgentId,
    windows: &[LimitWindowDto],
    share: Option<&LimitsWorkspaceCreditsDto>,
) -> Option<InnerRing> {
    fable_window(provider, windows)
        .map(|window| InnerRing::Fable {
            window_id: window.id.clone(),
        })
        .or_else(|| share.map(|_| InnerRing::WorkspaceShare))
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn fable_window(provider: AgentId, windows: &[LimitWindowDto]) -> Option<&LimitWindowDto> {
    if provider != AgentId::Claude {
        return None;
    }
    windows.iter().find(|window| {
        window.kind == LimitWindowKind::Model
            && window.label.trim().to_lowercase() == "weekly · fable"
    })
}

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

#[cfg(any(target_os = "windows", test))]
pub fn workspace_share_window(share: &LimitsWorkspaceCreditsDto) -> LimitWindowDto {
    LimitWindowDto {
        id: "workspace-credits".into(),
        label: "Workspace credits".into(),
        kind: LimitWindowKind::Model,
        used_percent: share.used_percent,
        resets_at: share.resets_at.clone(),
        window_seconds: None,
        observed_at: String::new(),
    }
}

#[cfg(any(target_os = "windows", test))]
pub fn workspace_share_renewed(
    share: &LimitsWorkspaceCreditsDto,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    share_reset(share).is_some_and(|reset| reset <= now)
}

#[cfg(any(target_os = "windows", test))]
fn share_reset(share: &LimitsWorkspaceCreditsDto) -> Option<chrono::DateTime<chrono::Utc>> {
    let reset = chrono::DateTime::parse_from_rfc3339(share.resets_at.as_deref()?).ok()?;
    Some(reset.with_timezone(&chrono::Utc))
}

#[cfg(any(target_os = "windows", test))]
pub fn workspace_share_note(
    share: &LimitsWorkspaceCreditsDto,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let Some(reset) = share_reset(share) else {
        return String::new();
    };
    let date = reset.with_timezone(&chrono::Local).format("%b %-d");
    if reset <= now {
        format!("Reset {date}")
    } else {
        format!("Resets {date}")
    }
}

#[cfg(test)]
mod tests;
