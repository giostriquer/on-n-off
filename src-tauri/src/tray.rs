//! The app's presence outside its main window.
//!
//! Both desktop platforms keep a status item, but they do different jobs.
//!
//! On macOS it is a Limits popover. Clicking the template icon opens a small always-on-top
//! window, and both app windows hide rather than close, because the status item — not the
//! Dock — is where the app lives.
//!
//! On Windows it is the app itself. Left click raises the main window, right click offers the
//! screens and a quit, and the icon is always present. Closing the main window quits, as it
//! always has, unless the user turned `closeToTray` on; then it hides, which is also what
//! removes the taskbar button. The live quota rail is `side_notch`'s job there, so this icon
//! stays static.

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "macos")]
use std::sync::Mutex;
#[cfg(any(target_os = "macos", test))]
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::{Window, WindowEvent};

#[cfg(target_os = "macos")]
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    window::{Effect, EffectState, EffectsBuilder},
    PhysicalPosition, PhysicalSize, Rect, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

#[cfg(target_os = "windows")]
use tauri::{
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

#[cfg(any(target_os = "macos", test))]
const SCREEN_EDGE_MARGIN: i32 = 8;
#[cfg(any(target_os = "macos", test))]
const TRAY_GAP: i32 = 6;
#[cfg(any(target_os = "macos", test))]
const FOCUS_LOSS_GUARD: Duration = Duration::from_millis(250);
const POPOVER_LABEL: &str = "limits-popover";
const MAIN_WINDOW_LABEL: &str = "main";
#[cfg(target_os = "macos")]
const TRAY_ICON_ID: &str = "limits";
#[cfg(target_os = "windows")]
const TRAY_ICON_ID: &str = "tray";
#[cfg(target_os = "macos")]
const POPOVER_WIDTH: f64 = 350.0;
#[cfg(target_os = "macos")]
const POPOVER_HEIGHT: f64 = 480.0;

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PixelPoint {
    x: i32,
    y: i32,
}

#[cfg(any(target_os = "macos", test))]
impl PixelPoint {
    const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug)]
struct PixelRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[cfg(any(target_os = "macos", test))]
impl PixelRect {
    const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug)]
struct PixelSize {
    width: u32,
    height: u32,
}

#[cfg(any(target_os = "macos", test))]
impl PixelSize {
    const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrayClickAction {
    Show,
    Hide,
    Ignore,
}

#[cfg(any(target_os = "macos", test))]
fn popover_position(tray: PixelRect, popover: PixelSize, monitor: PixelRect) -> PixelPoint {
    let ideal_x = tray.x + (tray.width as i32 - popover.width as i32) / 2;
    let minimum_x = monitor.x + SCREEN_EDGE_MARGIN;
    let maximum_x = monitor.x + monitor.width as i32 - popover.width as i32 - SCREEN_EDGE_MARGIN;
    let x = if maximum_x < minimum_x {
        monitor.x
    } else {
        ideal_x.clamp(minimum_x, maximum_x)
    };

    let ideal_y = tray.y + tray.height as i32 + TRAY_GAP;
    let minimum_y = monitor.y + SCREEN_EDGE_MARGIN;
    let maximum_y = monitor.y + monitor.height as i32 - popover.height as i32 - SCREEN_EDGE_MARGIN;
    let y = if maximum_y < minimum_y {
        monitor.y
    } else {
        ideal_y.clamp(minimum_y, maximum_y)
    };

    PixelPoint::new(x, y)
}

#[cfg(any(target_os = "macos", test))]
fn tray_click_action(visible: bool, since_focus_loss: Option<Duration>) -> TrayClickAction {
    if visible {
        TrayClickAction::Hide
    } else if since_focus_loss.is_some_and(|elapsed| elapsed < FOCUS_LOSS_GUARD) {
        TrayClickAction::Ignore
    } else {
        TrayClickAction::Show
    }
}

/// Whether this platform hides every app window on close. macOS does, because the status item
/// is the app's home there and the Dock icon brings the windows back; Windows has no
/// equivalent, so it hides only what the user asked it to.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
const HIDES_ALL_ON_CLOSE: bool = cfg!(target_os = "macos");

/// The close policy. The platform is a parameter rather than a constant read, so both CI legs
/// exercise both branches instead of each checking only its own.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn hides_on_close(label: &str, hides_all: bool, close_to_tray: bool) -> bool {
    match label {
        MAIN_WINDOW_LABEL => hides_all || close_to_tray,
        POPOVER_LABEL => hides_all,
        _ => false,
    }
}

#[cfg(any(target_os = "macos", test))]
fn hides_on_focus_loss(label: &str, focused: bool) -> bool {
    label == POPOVER_LABEL && !focused
}

/// The saved `closeToTray` flag. [`setup`] seeds it before the event loop starts, so it is
/// already resolved by the time any close can arrive, and `save_app_settings` refreshes it.
/// The close handler therefore only ever loads an atomic, never the settings file.
#[cfg(target_os = "windows")]
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
pub(crate) fn set_close_to_tray(enabled: bool) {
    CLOSE_TO_TRAY.store(enabled, Ordering::Relaxed);
}

/// macOS hides on close whatever the setting says, so it keeps no mirror to update.
#[cfg(not(target_os = "windows"))]
pub(crate) fn set_close_to_tray(_enabled: bool) {}

/// What [`hides_on_close`] should see for `close_to_tray` here. macOS never consults it.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn close_to_tray() -> bool {
    #[cfg(target_os = "windows")]
    {
        CLOSE_TO_TRAY.load(Ordering::Relaxed)
    }
    #[cfg(not(target_os = "windows"))]
    false
}

/// What a tray menu entry does. The ids live here rather than inline in [`setup`], so the
/// builder and the lookup cannot drift apart, and the mapping stays testable without a
/// running app.
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuAction {
    Open,
    Limits,
    PullRequests,
    Quit,
}

#[cfg(any(target_os = "windows", test))]
impl MenuAction {
    /// Every entry, in the order the menu shows them, with the label it carries.
    const ENTRIES: [(Self, &'static str); 4] = [
        (Self::Open, "Open on-n-off"),
        (Self::Limits, "Limits"),
        (Self::PullRequests, "Pull requests"),
        (Self::Quit, "Quit on-n-off"),
    ];

    const fn id(self) -> &'static str {
        match self {
            Self::Open => "tray-open",
            Self::Limits => "tray-limits",
            Self::PullRequests => "tray-pull-requests",
            Self::Quit => "tray-quit",
        }
    }
}

#[cfg(any(target_os = "windows", test))]
fn menu_action(id: &str) -> Option<MenuAction> {
    MenuAction::ENTRIES
        .into_iter()
        .find(|(action, _)| action.id() == id)
        .map(|(action, _)| action)
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct MacTrayState {
    last_focus_loss: Mutex<Option<Instant>>,
}

#[cfg(target_os = "macos")]
impl MacTrayState {
    fn record_focus_loss(&self) {
        if let Ok(mut last_focus_loss) = self.last_focus_loss.lock() {
            *last_focus_loss = Some(Instant::now());
        }
    }

    fn take_focus_loss_elapsed(&self) -> Option<Duration> {
        self.last_focus_loss
            .lock()
            .ok()
            .and_then(|mut last_focus_loss| last_focus_loss.take())
            .map(|lost_at| lost_at.elapsed())
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn setup(app: &mut tauri::App) -> tauri::Result<()> {
    app.manage(MacTrayState::default());

    TrayIconBuilder::with_id(TRAY_ICON_ID)
        .icon(tauri::include_image!("icons/tray-template.png"))
        .icon_as_template(true)
        .tooltip("on-n-off Limits")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Err(error) = toggle_popover(tray.app_handle(), rect) {
                    eprintln!("failed to toggle Limits popover: {error}");
                }
            }
        })
        .build(app)?;

    Ok(())
}

/// The Windows notification-area icon: the app's home while the main window is hidden.
#[cfg(target_os = "windows")]
pub(crate) fn setup(app: &mut tauri::App) -> tauri::Result<()> {
    // Seeded here, before the event loop starts, so a close never has to read the disk.
    set_close_to_tray(crate::settings::load_settings().close_to_tray);

    let mut items = MenuBuilder::new(app);
    for (action, label) in MenuAction::ENTRIES {
        if matches!(action, MenuAction::Quit) {
            items = items.separator();
        }
        items = items.text(action.id(), label);
    }
    let menu = items.build()?;

    let tray = TrayIconBuilder::with_id(TRAY_ICON_ID)
        // The colour app icon, not macOS's monochrome template: Windows draws the bitmap
        // as-is, so a template image would arrive as a black square. Embedded rather than
        // read from `default_window_icon()`, which is an `Option` — an iconless tray entry
        // must be unreachable, because it is the only way back to a hidden window.
        .icon(tauri::include_image!("icons/32x32.png"))
        .tooltip("on-n-off")
        .menu(&menu)
        // Left click belongs to the window; the menu is the right-click affordance.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let Some(action) = menu_action(event.id.as_ref()) else {
                return;
            };
            if let Err(error) = run_menu_action(app, action) {
                eprintln!("tray menu action failed: {error}");
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Err(error) = show_main_window(tray.app_handle()) {
                    eprintln!("failed to open the main window from the tray: {error}");
                }
            }
        });
    tray.build(app)?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn run_menu_action(app: &AppHandle, action: MenuAction) -> Result<(), String> {
    match action {
        MenuAction::Open => show_main_window(app),
        MenuAction::Limits => open_limits_window(app),
        MenuAction::PullRequests => open_github_window(app),
        MenuAction::Quit => {
            quit_app(app);
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
fn toggle_popover(app: &AppHandle, tray_rect: Rect) -> Result<(), String> {
    let visible = app
        .get_webview_window(POPOVER_LABEL)
        .map(|window| window.is_visible().unwrap_or(false))
        .unwrap_or(false);
    let since_focus_loss = app.state::<MacTrayState>().take_focus_loss_elapsed();

    match tray_click_action(visible, since_focus_loss) {
        TrayClickAction::Hide => hide_limits_popover(app),
        TrayClickAction::Ignore => Ok(()),
        TrayClickAction::Show => show_popover(app, tray_rect),
    }
}

#[cfg(target_os = "macos")]
fn show_popover(app: &AppHandle, tray_rect: Rect) -> Result<(), String> {
    let tray = physical_tray_rect(tray_rect);
    let monitor = app
        .monitor_from_point(
            f64::from(tray.x) + f64::from(tray.width) / 2.0,
            f64::from(tray.y) + f64::from(tray.height) / 2.0,
        )
        .map_err(|error| error.to_string())?
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or_else(|| "no active monitor is available".to_string())?;

    let scale_factor = monitor.scale_factor();
    let work_area = monitor.work_area();
    let monitor_rect = PixelRect::new(
        work_area.position.x,
        work_area.position.y,
        work_area.size.width,
        work_area.size.height,
    );
    let desired_width = logical_pixels(POPOVER_WIDTH, scale_factor);
    let desired_height = logical_pixels(POPOVER_HEIGHT, scale_factor);
    let available_height = (monitor_rect.y + monitor_rect.height as i32
        - (tray.y + tray.height as i32 + TRAY_GAP)
        - SCREEN_EDGE_MARGIN)
        .max(1) as u32;
    let size = PixelSize::new(
        desired_width.min(monitor_rect.width),
        desired_height.min(available_height),
    );
    let position = popover_position(tray, size, monitor_rect);

    let window = popover_window(app)?;
    window
        .set_size(PhysicalSize::new(size.width, size.height))
        .map_err(|error| error.to_string())?;
    window
        .set_position(PhysicalPosition::new(position.x, position.y))
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    window
        .emit("limits-popover-opened", ())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn physical_tray_rect(rect: Rect) -> PixelRect {
    // Tauri documents tray geometry as physical pixels.
    let position = rect.position.to_physical::<i32>(1.0);
    let size = rect.size.to_physical::<u32>(1.0);
    PixelRect::new(position.x, position.y, size.width, size.height)
}

#[cfg(target_os = "macos")]
fn logical_pixels(value: f64, scale_factor: f64) -> u32 {
    (value * scale_factor).round().max(1.0) as u32
}

#[cfg(target_os = "macos")]
fn popover_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        POPOVER_LABEL,
        WebviewUrl::App("index.html?surface=limits-popover".into()),
    )
    .title("Limits")
    .inner_size(POPOVER_WIDTH, POPOVER_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .decorations(false)
    .transparent(true)
    .effects(
        EffectsBuilder::new()
            .effect(Effect::Popover)
            .state(EffectState::Active)
            .radius(16.0)
            .build(),
    )
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .shadow(true)
    .focused(false)
    .visible(false)
    .build()
    .map_err(|error| error.to_string())
}

pub(crate) fn hide_limits_popover(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Brings the main window forward and tells the shell which screen to show.
fn open_main_window_on(app: &AppHandle, event: &str) -> Result<(), String> {
    show_main_window(app)?;
    let main = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| "the main window is not available".to_string())?;
    main.emit(event, ()).map_err(|error| error.to_string())?;
    hide_limits_popover(app)
}

pub(crate) fn open_limits_window(app: &AppHandle) -> Result<(), String> {
    open_main_window_on(app, "open-limits-window")
}

/// Brings the main window forward on the Pull requests screen (the side notch's popover).
pub(crate) fn open_github_window(app: &AppHandle) -> Result<(), String> {
    open_main_window_on(app, "open-github-window")
}

pub(crate) fn quit_app(app: &AppHandle) {
    app.exit(0);
}

pub(crate) fn show_main_window(app: &AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| "the main window is not available".to_string())?;
    main.unminimize().map_err(|error| error.to_string())?;
    main.show().map_err(|error| error.to_string())?;
    main.set_focus().map_err(|error| error.to_string())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn handle_window_event(window: &Window, event: &WindowEvent) {
    match event {
        WindowEvent::CloseRequested { api, .. }
            if hides_on_close(window.label(), HIDES_ALL_ON_CLOSE, close_to_tray()) =>
        {
            api.prevent_close();
            if let Err(error) = window.hide() {
                eprintln!("failed to hide {} window: {error}", window.label());
            }
        }
        #[cfg(target_os = "macos")]
        WindowEvent::Focused(focused) if hides_on_focus_loss(window.label(), *focused) => {
            window.state::<MacTrayState>().record_focus_loss();
            if let Err(error) = window.hide() {
                eprintln!("failed to dismiss Limits popover: {error}");
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
