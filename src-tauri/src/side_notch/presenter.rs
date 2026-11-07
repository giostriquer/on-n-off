use super::model::frame::RailData;
use super::model::{Display, NotchSettings, ShowMode};
use super::plan::{self, CellId, Hit, Hover, Page, Plan, TextMetrics, Zone};
use chrono::{DateTime, Utc};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum WinAction {
    ScreensChanged,
    Refresh,
    OpenLimits,
    OpenPullRequests,
    SetShow(ShowMode),
    OpenUrl(String),
    CopyReviewRequest { title: String, url: String },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Moved { x: i32, y: i32 },
    Left,
    Pressed { x: i32, y: i32 },
}

const HOVER_OPEN_DELAY: Duration = Duration::from_millis(120);
const HOVER_CLOSE_GRACE: Duration = Duration::from_millis(350);
pub const POINTER_POLL: Duration = Duration::from_millis(80);
pub const SCREEN_POLL: Duration = Duration::from_millis(500);

pub struct Presenter {
    text_metrics: &'static dyn TextMetrics,
    data: Option<RailData>,
    displays: Vec<Display>,
    cursor_inside: bool,
    hover: Hover,
    pinned: Option<CellId>,
    hovered_cell: Option<CellId>,
    open_at: Option<Instant>,
    last_inside: Instant,
    suppressed: bool,
    dirty: bool,
    displays_read: bool,
    presented: Option<Plan>,
}

impl Presenter {
    pub fn new(text_metrics: &'static dyn TextMetrics) -> Self {
        Self {
            text_metrics,
            data: None,
            displays: Vec::new(),
            cursor_inside: false,
            hover: Hover::default(),
            pinned: None,
            hovered_cell: None,
            open_at: None,
            last_inside: Instant::now(),
            suppressed: false,
            dirty: true,
            displays_read: false,
            presented: None,
        }
    }

    pub fn accept(&mut self, data: RailData) {
        if self.data.as_ref() == Some(&data) {
            return;
        }
        self.data = Some(data);
        self.dirty = true;
    }

    pub fn set_displays(&mut self, displays: Vec<Display>) -> Option<WinAction> {
        let first = !std::mem::replace(&mut self.displays_read, true);
        if self.displays == displays {
            return None;
        }
        self.displays = displays;
        self.dirty = true;
        (!first).then_some(WinAction::ScreensChanged)
    }

    pub fn set_suppressed(&mut self, suppressed: bool) {
        if self.suppressed != suppressed {
            self.suppressed = suppressed;
            self.dirty = true;
        }
    }

    fn settings(&self) -> Option<&NotchSettings> {
        Some(&self.data.as_ref()?.settings)
    }

    pub fn scale(&self) -> f64 {
        self.presented
            .as_ref()
            .map_or(1.0, |plan| plan.display_scale)
    }

    #[cfg(target_os = "windows")]
    pub fn selected(&self) -> Option<&Display> {
        super::model::selected_display(self.settings()?, &self.displays)
    }

    pub fn present(&mut self, now: DateTime<Utc>) -> Option<&Plan> {
        let mut next = self.plan(now);
        let moved = match (&self.presented, &next) {
            (Some(shown), Some(next)) => next.moved_from(shown),
            (None, Some(_)) => true,
            (_, None) => false,
        };
        if moved {
            self.pinned = None;
            self.hovered_cell = None;
            self.hover = Hover::default();
            next = self.plan(now);
        }
        self.presented = next;
        self.presented.as_ref()
    }

    fn plan(&self, now: DateTime<Utc>) -> Option<Plan> {
        if self.suppressed {
            return None;
        }
        let data = self.data.as_ref()?;
        if !data.settings.enabled {
            return None;
        }
        plan::plan(&self.displays, data, self.hover, now, self.text_metrics)
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn dismiss(&mut self) {
        if self.pinned.is_none() && self.hover.active.is_none() && self.hovered_cell.is_none() {
            return;
        }
        self.pinned = None;
        self.hover.active = None;
        self.hovered_cell = None;
        self.dirty = true;
    }

    pub fn cursor_at(&mut self, x: f64, y: f64, inside: bool, now: Instant) {
        self.cursor_inside = inside;
        if !inside {
            self.hovered_cell = None;
            self.open_at = None;
            self.clear_cap_highlight();
            return;
        }
        self.last_inside = now;
        let Some(plan) = self.presented.as_ref() else {
            self.hover = Hover::default();
            self.pinned = None;
            self.hovered_cell = None;
            self.open_at = None;
            return;
        };
        if !self.hover.rail_open && plan.pill.is_some() {
            self.hover.rail_open = true;
            self.dirty = true;
        }
        if !self.open_on_screen(plan) {
            return;
        }
        let hit = plan.hit(x, y);
        let on_cap = hit == Some(Hit::Cap);
        if on_cap != self.hover.cap_hovered {
            self.hover.cap_hovered = on_cap;
            self.dirty = true;
        }
        let cell = match hit {
            Some(Hit::Cell(id)) => Some(id),
            _ => None,
        };
        if cell != self.hovered_cell {
            self.hovered_cell = cell;
            match (self.pinned, cell) {
                (Some(_), Some(cell)) => {
                    self.pinned = Some(cell);
                    self.hover.active = Some(cell);
                    self.dirty = true;
                }
                (Some(_), None) => {}
                (None, _) => self.open_at = cell.map(|_| now + HOVER_OPEN_DELAY),
            }
        }
    }

    pub fn cursor_left(&mut self) {
        self.cursor_inside = false;
    }

    pub fn pinned(&self) -> Option<CellId> {
        self.pinned
    }

    fn clear_cap_highlight(&mut self) {
        if self.hover.cap_hovered {
            self.hover.cap_hovered = false;
            self.dirty = true;
        }
    }

    pub fn advance(&mut self, now: Instant) {
        if let Some(at) = self.open_at {
            if now >= at {
                self.open_at = None;
                if self.cursor_inside && self.pinned.is_none() && self.hovered_cell.is_some() {
                    self.hover.active = self.hovered_cell;
                    self.dirty = true;
                }
            }
        }
        if self.cursor_inside {
            self.last_inside = now;
        }
        let elapsed = now.saturating_duration_since(self.last_inside);
        if elapsed > HOVER_CLOSE_GRACE && self.pinned.is_none() {
            self.clear_cap_highlight();
            if self.hover.active.is_some() {
                self.hover.active = None;
                self.dirty = true;
            }
            if !self.always_shown() && self.hover.rail_open {
                self.hover.rail_open = false;
                self.dirty = true;
            }
        }
    }

    fn always_shown(&self) -> bool {
        self.settings()
            .is_some_and(|settings| settings.show == ShowMode::Always)
    }

    fn open_on_screen(&self, plan: &Plan) -> bool {
        plan.cap.pinned || self.hover.rail_open
    }

    pub fn pointer_poll_active(&self) -> bool {
        !self.suppressed && self.settings().is_some_and(|settings| settings.enabled)
    }

    pub fn clicked(&mut self, x: f64, y: f64) -> Vec<WinAction> {
        let Some(plan) = self.presented.as_ref() else {
            return Vec::new();
        };
        if !self.open_on_screen(plan) {
            return Vec::new();
        }
        self.last_inside = Instant::now();
        match plan.hit(x, y) {
            Some(Hit::Cap) => vec![WinAction::SetShow(if plan.cap.pinned {
                ShowMode::OnHover
            } else {
                ShowMode::Always
            })],
            Some(Hit::Cell(cell)) => {
                let pinned = (self.pinned != Some(cell)).then_some(cell);
                self.pinned = pinned;
                self.hover.active = pinned;
                self.dirty = true;
                Vec::new()
            }
            Some(Hit::Popover(Zone::OpenRow { url })) => vec![WinAction::OpenUrl(url.clone())],
            Some(Hit::Popover(Zone::CopyRow { url, title })) => {
                vec![WinAction::CopyReviewRequest {
                    title: title.clone(),
                    url: url.clone(),
                }]
            }
            Some(Hit::Popover(Zone::Footer(page))) => {
                self.pinned = None;
                self.hover.active = None;
                self.dirty = true;
                vec![
                    match page {
                        Page::Limits => WinAction::OpenLimits,
                        Page::PullRequests => WinAction::OpenPullRequests,
                    },
                    WinAction::Refresh,
                ]
            }
            None => Vec::new(),
        }
    }

    pub fn input(&mut self, input: Input, now: Instant) -> Vec<WinAction> {
        let scale = self.scale();
        match input {
            Input::Moved { x, y } => {
                self.cursor_at(f64::from(x) / scale, f64::from(y) / scale, true, now);
                Vec::new()
            }
            Input::Left => {
                self.cursor_left();
                Vec::new()
            }
            Input::Pressed { x, y } => self.clicked(f64::from(x) / scale, f64::from(y) / scale),
        }
    }

    pub fn deadline(&self, now: Instant) -> Instant {
        let mut deadline = now + SCREEN_POLL;
        if self.pointer_poll_active() {
            deadline = deadline.min(now + POINTER_POLL);
        }
        if let Some(at) = self.open_at {
            deadline = deadline.min(at);
        }
        let grace = self.last_inside + HOVER_CLOSE_GRACE + Duration::from_millis(5);
        if grace > now {
            deadline = deadline.min(grace);
        }
        deadline
    }
}

#[cfg(test)]
pub(super) mod tests;
