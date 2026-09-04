//! The Windows notch window: one transparent, always-on-top, non-activating layered
//! window on a dedicated thread, owning the hover/pin state machine (the
//! `PanelController` port) and presenting plans from `win_paint` through
//! `UpdateLayeredWindow`.

#![allow(unsafe_code)]

use super::model::{NotchSettings, ShowMode};
use super::win_paint::{self, Hover, Plan, RailData};
use crate::side_notch::model::Display;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::{
    sync::mpsc::{Receiver, Sender},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use tao::event::{ElementState, Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder};
use tao::platform::windows::{
    EventLoopBuilderExtWindows, WindowBuilderExtWindows, WindowExtWindows,
};
use tao::window::{Window, WindowBuilder};
use tiny_skia::Pixmap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindowRect,
    IsWindowVisible, SetWindowsHookExW, ShowWindow, UnhookWindowsHookEx, MSLLHOOKSTRUCT, SW_HIDE,
    SW_SHOWNOACTIVATE, WH_MOUSE_LL, WM_LBUTTONDOWN, WS_EX_LAYERED,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, UpdateLayeredWindow, GWL_EXSTYLE,
    GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_NOZORDER, ULW_ALPHA,
};

/// What the window asks the host to do; the macOS `ClientAction` set plus the two
/// row affordances the macOS helper performs itself (open in browser, copy).
#[derive(Clone, Debug, PartialEq)]
pub enum WinAction {
    Refresh,
    OpenLimits,
    OpenPullRequests,
    SetShow(ShowMode),
    OpenUrl(String),
    CopyReviewRequest { title: String, url: String },
}

/// Messages from the host thread.
#[derive(Debug)]
pub enum WindowMsg {
    /// The full content, when it changed or the heartbeat says it is time.
    Data(RailData),
    /// The supervisor's liveness probe; carries no payload.
    Ping,
    Shutdown,
}

const HOVER_OPEN_DELAY: Duration = Duration::from_millis(120);
const HOVER_CLOSE_GRACE: Duration = Duration::from_millis(350);
const POINTER_POLL: Duration = Duration::from_millis(80);
const SCREEN_POLL: Duration = Duration::from_millis(500);

/// The hover/pin state machine, pure enough to drive from tests with a fake clock.
#[derive(Debug)]
pub struct Machine {
    pub data: Option<RailData>,
    pub displays: Vec<Display>,
    pub cursor_inside: bool,
    pub hover: Hover,
    /// The cell pinned by a click; stays open until re-clicked or the pointer leaves.
    pub pinned: Option<usize>,
    /// The cell the pointer is over, awaiting the hover-open delay.
    hovered_cell: Option<usize>,
    open_at: Option<Instant>,
    last_inside: Instant,
    suppressed: bool,
    /// Bumped whenever the presented frame should change.
    dirty: bool,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self {
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
        }
    }

    /// A new host snapshot. A layout-affecting settings change drops hover state, like
    /// the macOS helper does when panels move. Identical payloads are ignored so the
    /// supervisor's liveness probe can ride the same channel freely.
    pub fn accept(&mut self, data: RailData) {
        if self.data.as_ref() == Some(&data) {
            return;
        }
        let moved = match &self.data {
            None => true,
            Some(previous) => {
                previous.settings.enabled != data.settings.enabled
                    || previous.settings.display_id != data.settings.display_id
                    || previous.settings.edge != data.settings.edge
                    || previous.settings.size != data.settings.size
                    || previous.settings.show != data.settings.show
                    || previous.settings.providers != data.settings.providers
                    || previous.settings.pull_requests.enabled
                        != data.settings.pull_requests.enabled
                    || previous.settings.pull_requests.lists != data.settings.pull_requests.lists
            }
        };
        if moved {
            self.pinned = None;
            self.hovered_cell = None;
            self.hover = Hover::default();
        }
        self.data = Some(data);
        self.dirty = true;
    }

    pub fn set_displays(&mut self, displays: Vec<Display>) {
        if self.displays != displays {
            self.displays = displays;
            self.dirty = true;
        }
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

    /// The selected display's scale, for converting cursor pixels to points.
    pub fn scale(&self) -> f64 {
        let Some(settings) = self.settings() else {
            return 1.0;
        };
        let Some(id) = settings.display_id.as_deref() else {
            return 1.0;
        };
        self.displays
            .iter()
            .find(|display| display.id == id)
            .map(|display| display.scale)
            .unwrap_or(1.0)
    }

    /// The plan for the current state, or `None` while hidden.
    pub fn plan(&self) -> Option<Plan> {
        if self.suppressed {
            return None;
        }
        let data = self.data.as_ref()?;
        if !data.settings.enabled {
            return None;
        }
        win_paint::plan(&data.settings, &self.displays, data, self.hover)
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Closes a pinned popover and the hover behind it, the way a click outside the
    /// overlay does. Every other transition on this machine is a method with a test, and
    /// this one has to be too: it is the one the outside-click hook drives, and the hook
    /// is the part that has already been wrong once.
    pub fn dismiss(&mut self) {
        if self.pinned.is_none() && self.hover.active.is_none() && self.hovered_cell.is_none() {
            return;
        }
        self.pinned = None;
        self.hover.active = None;
        self.hovered_cell = None;
        self.dirty = true;
    }

    /// One pointer sample, in window-local points. The cursor may be outside the
    /// window (sampled globally while the rail is open, mirroring the macOS
    /// controller's `NSEvent.mouseLocation` sampling).
    pub fn cursor_at(&mut self, x: f64, y: f64, inside: bool, now: Instant) {
        self.cursor_inside = inside;
        if !inside {
            // A pending hover-open must not fire once the pointer is gone, and the
            // cap's pin fades with it (the mac cap only lights while hovered).
            self.hovered_cell = None;
            self.open_at = None;
            self.clear_cap_highlight();
            return;
        }
        self.last_inside = now;
        let Some(plan) = self.plan() else {
            self.hover = Hover::default();
            self.pinned = None;
            self.hovered_cell = None;
            self.open_at = None;
            return;
        };
        if !self.hover.rail_open && plan.pill.is_some() {
            // The strip was reached: open the rail (the `pillEntered` port).
            self.hover.rail_open = true;
            self.dirty = true;
        }
        if !self.rail_open() {
            return;
        }
        let on_cap = plan.cap.rect.contains(x, y);
        if on_cap != self.hover.cap_hovered {
            self.hover.cap_hovered = on_cap;
            self.dirty = true;
        }
        let cell = win_paint::rail_hit(&plan, x, y);
        if cell != self.hovered_cell {
            self.hovered_cell = cell;
            if self.pinned.is_some() {
                // Hovering another cell while pinned moves the pin there at once.
                self.pinned = cell;
                self.hover.active = cell;
                self.dirty = true;
            } else {
                self.open_at = cell.map(|_| now + HOVER_OPEN_DELAY);
            }
        }
    }

    /// The pointer left the window's surface. State clears through the grace timer,
    /// not here: present() moves the window, which floods enter/leave pairs, so the
    /// pointer poll is the source of truth (the macOS controller's approach).
    pub fn cursor_left(&mut self, _now: Instant) {
        self.cursor_inside = false;
    }

    /// Puts the cap's pin out; the highlight only follows a pointer on the ear.
    fn clear_cap_highlight(&mut self) {
        if self.hover.cap_hovered {
            self.hover.cap_hovered = false;
            self.dirty = true;
        }
    }

    /// Advances the timers: hover-open delay, close grace, pinned-pointer watch.
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
        let inside_surfaces = self.pointer_on_surfaces();
        if inside_surfaces {
            self.last_inside = now;
        }
        let elapsed = now.saturating_duration_since(self.last_inside);
        if elapsed > HOVER_CLOSE_GRACE && self.pinned.is_none() {
            // A pinned popover stays open until an outside click (the low-level mouse
            // hook below) or a click on its cell; only hover-open popovers close here.
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

    fn pointer_on_surfaces(&self) -> bool {
        // The window thread feeds pointer samples; surfaces = the cursor is inside.
        self.cursor_inside
    }

    fn always_shown(&self) -> bool {
        self.settings()
            .is_some_and(|settings| settings.show == ShowMode::Always)
    }

    fn rail_open(&self) -> bool {
        self.always_shown() || self.hover.rail_open
    }
    /// Whether the loop should sample the pointer on its short cadence. tao reports no
    /// `CursorMoved` for this non-activating layered window, so the poll is the only
    /// pointer news there is: it has to run while the collapsed strip is showing too,
    /// or reaching the strip never opens the rail.
    pub fn pointer_poll_active(&self) -> bool {
        !self.suppressed && self.settings().is_some_and(|settings| settings.enabled)
    }

    /// A click in window-local points; returns the actions it asks for.
    pub fn clicked(&mut self, x: f64, y: f64) -> Vec<WinAction> {
        let Some(plan) = self.plan() else {
            return Vec::new();
        };
        if !self.rail_open() {
            return Vec::new();
        }
        self.last_inside = Instant::now();
        if plan.cap.rect.contains(x, y) {
            let next = if self.always_shown() {
                ShowMode::OnHover
            } else {
                ShowMode::Always
            };
            return vec![WinAction::SetShow(next)];
        }
        if let Some(cell) = win_paint::rail_hit(&plan, x, y) {
            if self.pinned == Some(cell) {
                self.pinned = None;
                self.hover.active = None;
            } else {
                self.pinned = Some(cell);
                self.hover.active = Some(cell);
            }
            self.dirty = true;
            return Vec::new();
        }
        if let Some((zone, _)) = plan
            .popover
            .as_ref()
            .and_then(|popover| popover.zones.iter().find(|(_, rect)| rect.contains(x, y)))
        {
            let cell = self.hover.active;
            return match zone {
                win_paint::Zone::OpenRow { url } => vec![WinAction::OpenUrl(url.clone())],
                win_paint::Zone::CopyRow { url, title } => vec![WinAction::CopyReviewRequest {
                    title: title.clone(),
                    url: url.clone(),
                }],
                win_paint::Zone::Footer => {
                    let footer = match cell {
                        Some(index) => {
                            match self.data.as_ref().and_then(|data| data.cells.get(index)) {
                                Some(super::win_paint::CellData::PullRequests(_)) => {
                                    WinAction::OpenPullRequests
                                }
                                _ => WinAction::OpenLimits,
                            }
                        }
                        None => WinAction::OpenLimits,
                    };
                    // The macOS helper dismisses after the action.
                    self.pinned = None;
                    self.hover.active = None;
                    self.dirty = true;
                    vec![footer, WinAction::Refresh]
                }
            };
        }
        Vec::new()
    }

    /// The next instant something can change, for the event loop's `WaitUntil`.
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

/// Spawns the window thread; the returned sender feeds it, and a dropped receiver on
/// our side tells the host the thread died.
pub fn spawn(action_tx: Sender<WinAction>) -> Sender<WindowMsg> {
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<WindowMsg>();
    thread::Builder::new()
        .name("side-notch-window".into())
        .spawn(move || window_main(msg_rx, action_tx))
        .expect("spawn the side notch window thread");
    msg_tx
}

/// The low-level mouse hook, installed once for the overlay's lifetime: while a
/// popover is pinned, an outside click dismisses it, the way the macOS helper's
/// global event monitor does. The procedure stays lock-free — it reads two atomics —
/// because Windows invokes it synchronously on the window thread for every system
/// mouse move; a slow hook starves input.
static OUTSIDE_CLICK_HOOK: Mutex<Option<SendHook>> = Mutex::new(None);
static OUTSIDE_CLICK_HWND: AtomicIsize = AtomicIsize::new(0);
static OUTSIDE_CLICK_PENDING: AtomicBool = AtomicBool::new(false);

struct SendHook(windows::Win32::UI::WindowsAndMessaging::HHOOK);
unsafe impl Send for SendHook {}

/// Installs the hook, if it is not already installed, and points it at `hwnd`.
///
/// Only a pinned popover needs it, and it costs the whole session: Windows calls the
/// procedure synchronously on this thread for every mouse move anywhere. So it is armed
/// on the pin and disarmed off it, by the thread that owns the window — a hook belongs to
/// the thread that installed it, and one left behind by a thread that has since died is
/// dead with it.
fn arm_outside_click_hook(hwnd: isize) {
    let mut guard = OUTSIDE_CLICK_HOOK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if guard.is_none() {
        // SAFETY: Windows invokes the procedure on this thread while it pumps messages.
        match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), None, 0) } {
            Ok(hook) => *guard = Some(SendHook(hook)),
            Err(_) => return,
        }
    }
    OUTSIDE_CLICK_HWND.store(hwnd, Ordering::SeqCst);
}

fn disarm_outside_click_hook() {
    OUTSIDE_CLICK_HWND.store(0, Ordering::SeqCst);
    let mut guard = OUTSIDE_CLICK_HOOK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(SendHook(hook)) = guard.take() {
        let _ = unsafe { UnhookWindowsHookEx(hook) };
    }
}

unsafe extern "system" fn low_level_mouse_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && wparam.0 as u32 == WM_LBUTTONDOWN {
        let hwnd = OUTSIDE_CLICK_HWND.load(Ordering::SeqCst);
        if hwnd != 0 {
            let info = unsafe { *(lparam.0 as *const MSLLHOOKSTRUCT) };
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(HWND(hwnd as *mut _), &mut rect) }.is_ok() {
                let inside = info.pt.x >= rect.left
                    && info.pt.x < rect.right
                    && info.pt.y >= rect.top
                    && info.pt.y < rect.bottom;
                if !inside {
                    OUTSIDE_CLICK_PENDING.store(true, Ordering::SeqCst);
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn window_main(msg_rx: Receiver<WindowMsg>, action_tx: Sender<WinAction>) {
    let _pm_v2 = super::win_displays::thread_pm_v2();
    let event_loop: EventLoop<WindowMsg> = EventLoopBuilder::<WindowMsg>::with_user_event()
        .with_any_thread(true)
        .build();
    let window = WindowBuilder::new()
        .with_title("on-n-off Side notch")
        .with_decorations(false)
        .with_transparent(false)
        .with_always_on_top(true)
        .with_resizable(false)
        .with_focusable(false)
        .with_skip_taskbar(true)
        .with_visible(false)
        .build(&event_loop)
        .expect("build the side notch window");

    let hwnd = window.hwnd();
    // tao rewrites GWL_EXSTYLE whenever its own flags diff, so layering goes last.
    window.set_visible(true);
    window.set_visible(false);
    unsafe { ensure_layered(hwnd) };
    disable_window_chrome(hwnd);
    // A previous window thread that panicked leaves its handle in the static; Windows has
    // already torn the hook down with that thread, so clear the record before arming.
    disarm_outside_click_hook();

    let mut machine = Machine::new();
    let mut was_visible = false;
    let mut last_poll = Instant::now();

    event_loop.run(move |event, _, control| {
        let mut exit = false;
        match event {
            // Fires on every wake (init, user events, timers) — the periodic slot.
            Event::NewEvents(_) => {
                while let Ok(msg) = msg_rx.try_recv() {
                    match msg {
                        WindowMsg::Data(data) => {
                            machine.accept(data);
                        }
                        WindowMsg::Shutdown => exit = true,

                        WindowMsg::Ping => {}
                    }
                }
                // Periodic screen work: displays re-enumeration and fullscreen check.
                if last_poll.elapsed() >= SCREEN_POLL {
                    last_poll = Instant::now();
                    match super::win_displays::read() {
                        Ok(displays) => {
                            machine.set_displays(displays);
                        }

                        Err(_) => {
                            machine.set_displays(Vec::new());
                        }
                    }
                    machine.set_suppressed(fullscreen_foreground(&machine, HWND(hwnd as *mut _)));
                }
                // Global pointer sampling while the rail shows: the cursor can sit on the

                // popover gap or the window may have just moved, so the poll - not the

                // enter/leave events - decides the hover state.

                if machine.pointer_poll_active() {
                    let scale = machine.scale();
                    let (origin_x, origin_y) = window_position(hwnd);
                    if let Some((x, y, inside)) = cursor_over(hwnd, scale, origin_x, origin_y) {
                        machine.cursor_at(x, y, inside, Instant::now());
                    }
                }
                machine.advance(Instant::now());
                // The hook exists only while a popover is pinned, which is the only state
                // an outside click has to reach; it sets a flag, and the dismissal happens
                // here, on this thread.
                if machine.pinned.is_some() {
                    arm_outside_click_hook(hwnd);
                } else {
                    disarm_outside_click_hook();
                }
                if OUTSIDE_CLICK_PENDING.swap(false, Ordering::SeqCst) {
                    machine.dismiss();
                }
            }
            Event::UserEvent(WindowMsg::Data(data)) => {
                machine.accept(data);
            }
            Event::UserEvent(WindowMsg::Shutdown) => exit = true,
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } if window_id == window.id() => {
                let scale = machine.scale();
                let x = position.x / scale;
                let y = position.y / scale;
                machine.cursor_at(x, y, true, Instant::now());
            }
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CursorLeft { .. },
                ..
            } if window_id == window.id() => {
                machine.cursor_left(Instant::now());
            }
            Event::WindowEvent {
                window_id,
                event:
                    WindowEvent::MouseInput {
                        button: tao::event::MouseButton::Left,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } if window_id == window.id() => {
                // The position of the press is the last known cursor position.
                let mut point = windows::Win32::Foundation::POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut point);
                }
                let scale = machine.scale();
                let (origin_x, origin_y) = window_position(hwnd);
                let x = (point.x - origin_x) as f64 / scale;
                let y = (point.y - origin_y) as f64 / scale;

                for action in machine.clicked(x, y) {
                    if action_tx.send(action).is_err() {
                        exit = true;
                    }
                }
            }
            _ => {}
        }

        if exit {
            disarm_outside_click_hook();
            *control = ControlFlow::Exit;
            return;
        }

        if machine.take_dirty() {
            match machine.plan() {
                Some(plan) if !plan.cells.is_empty() => {
                    // UpdateLayeredWindow carries the new origin and size atomically
                    // with the pixels; a separate SetWindowPos would flash a frame.
                    present(&window, &plan);
                    if !was_visible {
                        unsafe {
                            let _ = ShowWindow(HWND(hwnd as *mut _), SW_SHOWNOACTIVATE);
                        }
                    }
                    was_visible = true;
                }
                _ => {
                    if was_visible {
                        unsafe {
                            let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
                        }
                    }
                    was_visible = false;
                }
            }
        }

        *control = ControlFlow::WaitUntil(machine.deadline(Instant::now()));
        unsafe {
            if !IsWindowVisible(HWND(hwnd as *mut _)).as_bool() {
                // Keep the loop light while hidden.
                *control = ControlFlow::WaitUntil(Instant::now() + SCREEN_POLL);
            }
        }
    });
}

fn cursor_over(hwnd: isize, scale: f64, origin_x: i32, origin_y: i32) -> Option<(f64, f64, bool)> {
    let mut point = windows::Win32::Foundation::POINT::default();
    unsafe {
        GetCursorPos(&mut point).ok()?;
    }
    let inside = unsafe { window_under_cursor(hwnd) };
    Some((
        (point.x - origin_x) as f64 / scale,
        (point.y - origin_y) as f64 / scale,
        inside,
    ))
}

unsafe fn window_under_cursor(hwnd: isize) -> bool {
    let mut point = windows::Win32::Foundation::POINT::default();
    if GetCursorPos(&mut point).is_err() {
        return false;
    }
    let hovered = windows::Win32::UI::WindowsAndMessaging::WindowFromPoint(point);
    !hovered.is_invalid() && hovered.0 == HWND(hwnd as *mut _).0
}

fn window_position(hwnd: isize) -> (i32, i32) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(HWND(hwnd as *mut _), &mut rect);
    }
    (rect.left, rect.top)
}

/// The foreground window covering the notch's own display edge to edge (and not being
/// the shell) means a fullscreen app: the notch hides instead of floating over it.
/// Fullscreen activity on other monitors must not hide this rail, which is why the
/// check is scoped to the selected display.
fn fullscreen_foreground(machine: &Machine, own: HWND) -> bool {
    let Some(settings) = machine.settings() else {
        return false;
    };
    let Some(selected) = settings
        .display_id
        .as_deref()
        .and_then(|id| machine.displays.iter().find(|display| display.id == id))
    else {
        return false;
    };
    let (mx, my, mw, mh) = (
        (selected.x * selected.scale).round() as i32,
        (selected.y * selected.scale).round() as i32,
        (selected.width * selected.scale).round() as i32,
        (selected.height * selected.scale).round() as i32,
    );
    let mut rect = RECT::default();
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() || foreground == own {
            return false;
        }
        let mut class_name = [0u16; 64];
        let len = GetClassNameW(foreground, &mut class_name);
        let class = String::from_utf16_lossy(&class_name[..len.max(0) as usize]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd")
            || class.starts_with("XamlExplorerHost")
        {
            return false;
        }
        if GetWindowRect(foreground, &mut rect).is_err() {
            return false;
        }
    }
    rect.left <= mx && rect.top <= my && rect.right >= mx + mw && rect.bottom >= my + mh
}

/// The overlay's window styles. tao leaves an undecorated window with a caption and a
/// resize frame; Windows then keeps an invisible border on the left, right and bottom,
/// so the client area — and with it the layered surface — is inset by the border width
/// and the desktop compositor draws a shadow and a top hairline around the remainder.
/// A bare popup has no non-client frame, so the rail fills its window rect and reaches
/// the screen edge.
pub(super) fn overlay_style(current: u32) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
        WS_THICKFRAME,
    };
    let frame = WS_CAPTION.0
        | WS_BORDER.0
        | WS_DLGFRAME.0
        | WS_THICKFRAME.0
        | WS_SYSMENU.0
        | WS_MINIMIZEBOX.0
        | WS_MAXIMIZEBOX.0;
    (current & !frame) | WS_POPUP.0
}

/// Re-asserts the layered bit and the popup frame; tao rewrites the style words
/// whenever its own flags diff, so this runs before every present.
unsafe fn ensure_layered(hwnd: isize) {
    let window = HWND(hwnd as *mut _);
    let ex = GetWindowLongPtrW(window, GWL_EXSTYLE);
    SetWindowLongPtrW(window, GWL_EXSTYLE, ex | (WS_EX_LAYERED.0 as isize));
    let style = GetWindowLongPtrW(window, GWL_STYLE) as u32;
    let wanted = overlay_style(style);
    if wanted != style {
        SetWindowLongPtrW(window, GWL_STYLE, wanted as isize);
        let _ = SetWindowPos(
            window,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED
                | SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOZORDER
                | SWP_NOACTIVATE
                | SWP_NOOWNERZORDER,
        );
    }
}

/// Windows 11 rounds every top-level window's corners and draws a 1 px border — on a
/// transparent overlay that chrome reads as a rounded rectangle floating over the rail.
/// Square corners and no border, so the silhouette is the only visible shape.
fn disable_window_chrome(hwnd: isize) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWM_WINDOW_CORNER_PREFERENCE,
    };
    unsafe {
        let round = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
        let no_border: u32 = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_BORDER_COLOR,
            &no_border as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

/// Premultiplied RGBA (tiny-skia) -> layered window via UpdateLayeredWindow.
fn present(window: &Window, plan: &Plan) {
    let pixmap = win_paint::render(plan);
    present_pixmap(window.hwnd(), &pixmap, plan);
}

fn present_pixmap(hwnd: isize, pixmap: &Pixmap, plan: &Plan) {
    unsafe {
        ensure_layered(hwnd);
    }
    let scale = plan.display_scale;
    let x = (plan.window.x * scale).round() as i32;
    let y = (plan.window.y * scale).round() as i32;
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    let mut bgra = pixmap.data().to_vec();
    for px in bgra.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    unsafe {
        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        if let Ok(hbmp) = CreateDIBSection(Some(hdc_mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
            let old = SelectObject(hdc_mem, hbmp.into());
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                HWND(hwnd as *mut _),
                Some(hdc_screen),
                Some(&windows::Win32::Foundation::POINT { x, y }),
                Some(&windows::Win32::Foundation::SIZE { cx: w, cy: h }),
                Some(hdc_mem),
                Some(&windows::Win32::Foundation::POINT { x: 0, y: 0 }),
                windows::Win32::Foundation::COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(hdc_mem, old);
            let _ = DeleteObject(hbmp.into());
        }
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(None, hdc_screen);
    }
}

#[cfg(test)]
mod tests;
