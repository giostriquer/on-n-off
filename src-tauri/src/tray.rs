#[cfg(target_os = "macos")]
use std::sync::Mutex;
#[cfg(any(target_os = "macos", test))]
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager};

#[cfg(target_os = "macos")]
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    window::{Effect, EffectState, EffectsBuilder},
    PhysicalPosition, PhysicalSize, Rect, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window,
    WindowEvent,
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

#[cfg(any(target_os = "macos", test))]
fn hides_on_close(label: &str) -> bool {
    matches!(label, MAIN_WINDOW_LABEL | POPOVER_LABEL)
}

#[cfg(any(target_os = "macos", test))]
fn hides_on_focus_loss(label: &str, focused: bool) -> bool {
    label == POPOVER_LABEL && !focused
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

    TrayIconBuilder::with_id("limits")
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

pub(crate) fn open_limits_window(app: &AppHandle) -> Result<(), String> {
    show_main_window(app)?;

    let main = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| "the main window is not available".to_string())?;
    main.emit("open-limits-window", ())
        .map_err(|error| error.to_string())?;
    hide_limits_popover(app)
}

/// Brings the main window forward on the Pull requests screen.
pub(crate) fn open_github_window(app: &AppHandle) -> Result<(), String> {
    show_main_window(app)?;
    let main = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| "the main window is not available".to_string())?;
    main.emit("open-github-window", ())
        .map_err(|error| error.to_string())?;
    hide_limits_popover(app)
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

#[cfg(target_os = "macos")]
pub(crate) fn handle_window_event(window: &Window, event: &WindowEvent) {
    match event {
        WindowEvent::CloseRequested { api, .. } if hides_on_close(window.label()) => {
            api.prevent_close();
            if let Err(error) = window.hide() {
                eprintln!("failed to hide {} window: {error}", window.label());
            }
        }
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
mod tests {
    use std::time::Duration;

    use super::{
        hides_on_close, hides_on_focus_loss, popover_position, tray_click_action, PixelPoint,
        PixelRect, PixelSize, TrayClickAction,
    };

    #[test]
    fn centers_the_popover_below_the_status_item() {
        let tray = PixelRect::new(490, 0, 20, 24);
        let monitor = PixelRect::new(0, 0, 1_000, 800);

        assert_eq!(
            popover_position(tray, PixelSize::new(420, 560), monitor),
            PixelPoint::new(290, 30)
        );
    }

    #[test]
    fn clamps_the_popover_inside_either_horizontal_screen_edge() {
        let monitor = PixelRect::new(0, 0, 1_000, 800);
        let popover = PixelSize::new(420, 560);

        assert_eq!(
            popover_position(PixelRect::new(0, 0, 20, 24), popover, monitor).x,
            8
        );
        assert_eq!(
            popover_position(PixelRect::new(980, 0, 20, 24), popover, monitor).x,
            572
        );
    }

    #[test]
    fn preserves_negative_monitor_coordinates() {
        let tray = PixelRect::new(-730, 0, 20, 24);
        let monitor = PixelRect::new(-1_440, 0, 1_440, 900);

        assert_eq!(
            popover_position(tray, PixelSize::new(420, 560), monitor),
            PixelPoint::new(-930, 30)
        );
    }

    #[test]
    fn ignores_a_tray_click_immediately_after_focus_loss() {
        assert_eq!(
            tray_click_action(false, Some(Duration::from_millis(80))),
            TrayClickAction::Ignore
        );
        assert_eq!(tray_click_action(true, None), TrayClickAction::Hide);
        assert_eq!(tray_click_action(false, None), TrayClickAction::Show);
        assert_eq!(
            tray_click_action(false, Some(Duration::from_millis(400))),
            TrayClickAction::Show
        );
    }

    #[test]
    fn hides_app_windows_instead_of_closing_them() {
        assert!(hides_on_close("main"));
        assert!(hides_on_close("limits-popover"));
        assert!(!hides_on_close("settings"));
    }

    #[test]
    fn dismisses_only_the_popover_when_it_loses_focus() {
        assert!(hides_on_focus_loss("limits-popover", false));
        assert!(!hides_on_focus_loss("limits-popover", true));
        assert!(!hides_on_focus_loss("main", false));
    }
}
