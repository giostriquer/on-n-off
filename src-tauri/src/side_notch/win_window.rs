#![allow(unsafe_code)]

use super::model::frame::RailData;
use super::plan::Plan;
use super::presenter::WinAction;
use super::presenter::{Input, Presenter, SCREEN_POLL};
use super::win_paint::{self, DirectWrite};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::{
    sync::mpsc::{Receiver, Sender},
    sync::Mutex,
    thread,
    time::Instant,
};
use tiny_skia::Pixmap;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindow, GetWindowRect,
    IsWindowVisible, SetWindowsHookExW, ShowWindow, UnhookWindowsHookEx, GW_HWNDPREV, HWND_TOPMOST,
    MSLLHOOKSTRUCT, SW_HIDE, SW_SHOWNOACTIVATE, WH_MOUSE_LL, WM_LBUTTONDOWN, WS_EX_LAYERED,
    WS_EX_TOPMOST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, LoadCursorW,
    MsgWaitForMultipleObjectsEx, PeekMessageW, RegisterClassExW, TranslateMessage, IDC_ARROW, MSG,
    MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_CLOSE, WM_MOUSEMOVE, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, UpdateLayeredWindow, GWL_EXSTYLE,
    GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_NOZORDER, ULW_ALPHA,
};

#[derive(Debug)]
pub enum WindowMsg {
    Data(RailData),
    Ping,
    Shutdown,
}

const ABOVE_WALK_LIMIT: usize = 512;

pub fn spawn(action_tx: Sender<WinAction>) -> Sender<WindowMsg> {
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<WindowMsg>();
    thread::Builder::new()
        .name("side-notch-window".into())
        .spawn(move || window_main(msg_rx, action_tx))
        .expect("spawn the side notch window thread");
    msg_tx
}

static OUTSIDE_CLICK_HOOK: Mutex<Option<SendHook>> = Mutex::new(None);
static OUTSIDE_CLICK_HWND: AtomicIsize = AtomicIsize::new(0);
static OUTSIDE_CLICK_PENDING: AtomicBool = AtomicBool::new(false);

struct SendHook(windows::Win32::UI::WindowsAndMessaging::HHOOK);
unsafe impl Send for SendHook {}

fn arm_outside_click_hook(hwnd: isize) {
    let mut guard = OUTSIDE_CLICK_HOOK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if guard.is_none() {
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

const CLASS: PCWSTR = w!("on-n-off Side notch");
const WM_MOUSELEAVE: u32 = 0x02A3;
fn client_point(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam.0 & 0xFFFF) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16;
    (i32::from(x), i32::from(y))
}

thread_local! {
    static INPUT: RefCell<Vec<Input>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let input = match msg {
        WM_MOUSEMOVE => {
            let mut track = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = unsafe { TrackMouseEvent(&mut track) };
            let (x, y) = client_point(lparam);
            Input::Moved { x, y }
        }
        WM_MOUSELEAVE => Input::Left,
        WM_LBUTTONDOWN => {
            let (x, y) = client_point(lparam);
            Input::Pressed { x, y }
        }
        WM_CLOSE => return LRESULT(0),
        _ => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    };
    INPUT.with_borrow_mut(|queue| queue.push(input));
    LRESULT(0)
}

fn create_overlay() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_proc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            CLASS,
            w!("on-n-off Side notch"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()
    }
}

fn pump_messages() {
    let mut msg = MSG::default();
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn wait_until(deadline: Instant) {
    let timeout = deadline.saturating_duration_since(Instant::now());
    let millis = timeout
        .as_nanos()
        .div_ceil(1_000_000)
        .min(u128::from(u32::MAX - 1)) as u32;
    unsafe {
        MsgWaitForMultipleObjectsEx(None, millis, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
    }
}

fn window_main(msg_rx: Receiver<WindowMsg>, action_tx: Sender<WinAction>) {
    let _pm_v2 = super::win_displays::thread_pm_v2();
    let Some(own) = create_overlay() else {
        return;
    };
    let hwnd = own.0 as isize;
    unsafe { ensure_layered(hwnd) };
    disable_window_chrome(hwnd);
    disarm_outside_click_hook();

    let mut presenter = Presenter::new(&DirectWrite);
    let mut was_visible = false;
    let mut last_poll = Instant::now();
    let mut deadline = Instant::now();

    loop {
        wait_until(deadline);
        pump_messages();
        let mut exit = false;
        while let Ok(msg) = msg_rx.try_recv() {
            match msg {
                WindowMsg::Data(data) => {
                    presenter.accept(data);
                }
                WindowMsg::Shutdown => exit = true,

                WindowMsg::Ping => {}
            }
        }
        if last_poll.elapsed() >= SCREEN_POLL {
            last_poll = Instant::now();
            let displays = super::win_displays::read().unwrap_or_default();
            if let Some(changed) = presenter.set_displays(displays) {
                if action_tx.send(changed).is_err() {
                    exit = true;
                }
            }
            let fullscreen = fullscreen_foreground(&presenter, own);
            presenter.set_suppressed(fullscreen);
            keep_on_top(own, fullscreen);
        }

        if presenter.pointer_poll_active() {
            let scale = presenter.scale();
            let (origin_x, origin_y) = window_position(hwnd);
            if let Some((x, y, inside)) = cursor_over(hwnd, scale, origin_x, origin_y) {
                presenter.cursor_at(x, y, inside, Instant::now());
            }
        }
        presenter.advance(Instant::now());
        if presenter.pinned().is_some() {
            arm_outside_click_hook(hwnd);
        } else {
            disarm_outside_click_hook();
        }
        if OUTSIDE_CLICK_PENDING.swap(false, Ordering::SeqCst) {
            presenter.dismiss();
        }

        for input in INPUT.with_borrow_mut(std::mem::take) {
            for action in presenter.input(input, Instant::now()) {
                if action_tx.send(action).is_err() {
                    exit = true;
                }
            }
        }

        if exit {
            break;
        }

        if presenter.take_dirty() {
            match presenter.present(chrono::Utc::now()) {
                Some(plan) => {
                    present(hwnd, plan);
                    if !was_visible {
                        unsafe {
                            let _ = ShowWindow(own, SW_SHOWNOACTIVATE);
                        }
                    }
                    was_visible = true;
                }
                _ => {
                    if was_visible {
                        unsafe {
                            let _ = ShowWindow(own, SW_HIDE);
                        }
                    }
                    was_visible = false;
                }
            }
        }

        deadline = presenter.deadline(Instant::now());
        unsafe {
            if !IsWindowVisible(own).as_bool() {
                deadline = Instant::now() + SCREEN_POLL;
            }
        }
    }
    disarm_outside_click_hook();
    unsafe {
        let _ = DestroyWindow(own);
    }
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

fn fullscreen_foreground(presenter: &Presenter, own: HWND) -> bool {
    let Some(selected) = presenter.selected() else {
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

#[derive(Clone, Copy)]
struct WindowAbove {
    visible: bool,
    cloaked: bool,
    topmost: bool,
}

fn sunk_below_ordinary_windows(above: impl IntoIterator<Item = WindowAbove>) -> bool {
    above
        .into_iter()
        .any(|window| window.visible && !window.cloaked && !window.topmost)
}

fn windows_above(own: HWND) -> impl Iterator<Item = WindowAbove> {
    let next = |window: HWND| unsafe { GetWindow(window, GW_HWNDPREV) }.ok();
    std::iter::successors(next(own), move |window| next(*window))
        .take(ABOVE_WALK_LIMIT)
        .map(|window| {
            let mut cloaked: u32 = 0;
            unsafe {
                let _ = DwmGetWindowAttribute(
                    window,
                    DWMWA_CLOAKED,
                    &mut cloaked as *mut u32 as *mut core::ffi::c_void,
                    std::mem::size_of::<u32>() as u32,
                );
                WindowAbove {
                    visible: IsWindowVisible(window).as_bool(),
                    cloaked: cloaked != 0,
                    topmost: GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0,
                }
            }
        })
}

fn keep_on_top(own: HWND, fullscreen: bool) {
    if fullscreen
        || !unsafe { IsWindowVisible(own) }.as_bool()
        || !sunk_below_ordinary_windows(windows_above(own))
    {
        return;
    }
    unsafe {
        let _ = SetWindowPos(
            own,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

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

fn present(hwnd: isize, plan: &Plan) {
    let pixmap = win_paint::render(plan);
    present_pixmap(hwnd, &pixmap, plan);
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
