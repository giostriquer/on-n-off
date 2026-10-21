use super::*;
use crate::dto::AgentId;
use crate::dto::{LimitWindowDto, LimitWindowKind, Reading};
use crate::side_notch::model::frame;
use crate::side_notch::model::{Display, NotchProvider, NotchSettings};
use crate::side_notch::win_paint::R;
use std::time::Duration;

fn display(id: &str, mirrored: bool) -> Display {
    Display {
        id: id.into(),
        name: id.into(),
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
        work_y: 0.0,
        work_height: 1040.0,
        scale: 1.0,
        mirrored,
    }
}

fn settings(show: ShowMode) -> NotchSettings {
    NotchSettings {
        enabled: true,
        display_id: Some("d1".into()),
        show,
        ..NotchSettings::default()
    }
}

fn data(show: ShowMode) -> RailData {
    let claude = NotchProvider::current(vec![crate::limits::signed_in_card(
        AgentId::Claude,
        "acct",
        Reading {
            windows: vec![LimitWindowDto {
                id: "w".into(),
                label: "Current session".into(),
                kind: LimitWindowKind::Session,
                used_percent: 10.0,
                resets_at: None,
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
                role: None,
                starts_on_first_message: false,
            }],
            ..Reading::default()
        },
    )])
    .expect("a signed-in account");
    frame::rail(
        &settings(show),
        [(AgentId::Claude, Some(claude), Vec::new())],
        None,
        None,
    )
}

fn cell_rect(machine: &Machine) -> R {
    machine.plan().expect("plan").cells[0].rect
}

#[test]
fn the_rail_is_hidden_until_enabled_data_and_displays_agree() {
    let now = Instant::now();
    let mut machine = Machine::new();
    assert!(machine.plan().is_none());
    machine.accept(data(ShowMode::Always));
    assert!(machine.plan().is_none(), "no displays yet");
    machine.set_displays(vec![display("d1", false)]);
    assert!(machine.plan().is_some());
    let _ = now;
}

#[test]
fn the_pointer_scale_is_the_selected_displays_and_1x_for_a_mirrored_or_duplicated_selection() {
    let mut machine = Machine::new();
    assert_eq!(machine.scale(), 1.0, "no settings yet");
    machine.accept(data(ShowMode::Always));
    let mut scaled = display("d1", false);
    scaled.scale = 1.5;
    machine.set_displays(vec![scaled.clone()]);
    assert_eq!(machine.scale(), 1.5);
    let mut mirrored = scaled.clone();
    mirrored.mirrored = true;
    machine.set_displays(vec![mirrored]);
    assert!(machine.plan().is_none());
    assert_eq!(machine.scale(), 1.0, "a mirrored selection");
    machine.set_displays(vec![scaled.clone(), scaled]);
    assert!(machine.plan().is_none());
    assert_eq!(machine.scale(), 1.0, "two displays with the selected id");
}

#[test]
fn the_hover_strip_opens_the_rail_and_the_grace_closes_it() {
    let mut machine = Machine::new();
    machine.accept(data(ShowMode::OnHover));
    machine.set_displays(vec![display("d1", false)]);
    assert!(
        machine.plan().expect("plan").pill.is_some(),
        "collapsed first"
    );

    let pill = machine.plan().unwrap().pill.unwrap();
    machine.cursor_at(pill.mid_x(), pill.mid_y(), true, Instant::now());
    assert!(machine.hover.rail_open);

    let cell = cell_rect(&machine);
    let entered = Instant::now();
    machine.cursor_at(cell.mid_x(), cell.mid_y(), true, entered);
    assert!(machine.hover.active.is_none(), "not before the delay");
    machine.advance(entered + HOVER_OPEN_DELAY + Duration::from_millis(5));
    assert_eq!(machine.hover.active, Some(0));

    machine.cursor_left(entered + Duration::from_millis(200));
    machine.advance(entered + Duration::from_millis(200) + HOVER_CLOSE_GRACE);
    assert_eq!(machine.hover.active, None);
    assert!(!machine.hover.rail_open);
}

#[test]
fn a_click_pins_the_popover_until_the_same_cell_is_clicked_again() {
    let mut machine = Machine::new();
    machine.accept(data(ShowMode::Always));
    machine.set_displays(vec![display("d1", false)]);
    let now = Instant::now();

    let cell = cell_rect(&machine);
    assert!(machine
        .clicked(cell.mid_x() + 1000.0, cell.mid_y())
        .is_empty());
    assert_eq!(machine.pinned, None);

    let cell = cell_rect(&machine);
    machine.clicked(cell.mid_x(), cell.mid_y());
    assert_eq!(machine.pinned, Some(0));
    assert_eq!(machine.hover.active, Some(0));

    machine.cursor_left(now);
    machine.advance(now + HOVER_CLOSE_GRACE);
    assert_eq!(
        machine.hover.active,
        Some(0),
        "a fresh pin is not dropped by the grace"
    );

    let cell = cell_rect(&machine);
    machine.clicked(cell.mid_x(), cell.mid_y());
    assert_eq!(machine.pinned, None);
    assert_eq!(machine.hover.active, None);
}

#[test]
fn a_click_outside_the_overlay_dismisses_a_pinned_popover() {
    let mut machine = Machine::new();
    machine.accept(data(ShowMode::Always));
    machine.set_displays(vec![display("d1", false)]);

    let cell = cell_rect(&machine);
    machine.clicked(cell.mid_x(), cell.mid_y());
    assert_eq!(machine.pinned, Some(0));
    machine.take_dirty();

    machine.dismiss();
    assert_eq!(machine.pinned, None);
    assert_eq!(machine.hover.active, None);
    assert!(machine.take_dirty(), "the dismissal asks for a repaint");

    machine.dismiss();
    assert!(!machine.take_dirty(), "an idle dismissal is not a change");
}

#[test]
fn clicking_the_cap_asks_for_the_show_mode_to_flip() {
    let mut machine = Machine::new();
    machine.accept(data(ShowMode::Always));
    machine.set_displays(vec![display("d1", false)]);
    let cap = machine.plan().unwrap().cap.rect;
    assert_eq!(
        machine.clicked(cap.mid_x(), cap.mid_y()),
        vec![WinAction::SetShow(ShowMode::OnHover)]
    );

    let mut machine = Machine::new();
    machine.accept(data(ShowMode::OnHover));
    machine.set_displays(vec![display("d1", false)]);
    let pill = machine.plan().unwrap().pill.unwrap();
    machine.cursor_at(pill.mid_x(), pill.mid_y(), true, Instant::now());
    let cap = machine.plan().unwrap().cap.rect;
    assert_eq!(
        machine.clicked(cap.mid_x(), cap.mid_y()),
        vec![WinAction::SetShow(ShowMode::Always)]
    );
}

#[test]
fn the_deadline_never_sleeps_past_the_scheduled_open() {
    let entered = Instant::now();
    let idle = Machine::new();
    assert!(idle.deadline(entered) >= entered + Duration::from_millis(100));

    let mut machine = Machine::new();
    machine.accept(data(ShowMode::Always));
    machine.set_displays(vec![display("d1", false)]);
    let cell = cell_rect(&machine);
    machine.cursor_at(cell.mid_x(), cell.mid_y(), true, entered);
    let deadline = machine.deadline(entered);
    assert!(
        deadline <= entered + HOVER_OPEN_DELAY,
        "the loop must wake for the hover-open delay"
    );
}

#[test]
fn a_layout_changing_settings_update_drops_hover_state() {
    let mut machine = Machine::new();
    machine.accept(data(ShowMode::Always));
    machine.set_displays(vec![display("d1", false)]);
    let cell = cell_rect(&machine);
    machine.clicked(cell.mid_x(), cell.mid_y());
    assert_eq!(machine.pinned, Some(0));

    let mut next = data(ShowMode::Always);
    next.settings.edge = crate::side_notch::model::Edge::Left;
    machine.accept(next);
    assert_eq!(machine.pinned, None, "panels moved; hover state resets");
}

#[test]
fn fullscreen_suppression_hides_the_whole_window() {
    let mut machine = Machine::new();
    machine.accept(data(ShowMode::Always));
    machine.set_displays(vec![display("d1", false)]);
    assert!(machine.plan().is_some());
    machine.set_suppressed(true);
    assert!(machine.plan().is_none(), "a fullscreen app owns the screen");
    machine.set_suppressed(false);
    assert!(machine.plan().is_some());
}

#[test]
fn mirrored_or_missing_displays_hide_the_rail() {
    let mut machine = Machine::new();
    machine.accept(data(ShowMode::Always));
    machine.set_displays(vec![display("d1", true)]);
    assert!(machine.plan().is_none());
    machine.set_displays(vec![display("other", false)]);
    assert!(machine.plan().is_none());
    machine.set_displays(vec![display("d1", false)]);
    assert!(machine.plan().is_some());
}

#[test]
fn leaving_the_window_clears_the_cap_highlight() {
    let mut machine = Machine::new();
    machine.set_displays(vec![display("d1", false)]);
    machine.accept(data(ShowMode::Always));
    let cap = machine.plan().expect("plan").cap.rect;
    let now = Instant::now();
    machine.cursor_at(cap.mid_x(), cap.mid_y(), true, now);
    assert!(
        machine.hover.cap_hovered,
        "the pin lights under the pointer"
    );
    machine.cursor_at(-40.0, -40.0, false, now + Duration::from_millis(100));
    assert!(
        !machine.hover.cap_hovered,
        "and fades again once the pointer is gone"
    );
}

#[test]
fn the_overlay_style_drops_the_caption_and_resize_frame() {
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_CAPTION, WS_CLIPSIBLINGS, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
    };
    let tao = WS_VISIBLE.0 | WS_CLIPSIBLINGS.0 | WS_CAPTION.0 | WS_THICKFRAME.0;
    let overlay = overlay_style(tao);
    assert_eq!(overlay & WS_CAPTION.0, 0, "no caption");
    assert_eq!(overlay & WS_THICKFRAME.0, 0, "no resize frame");
    assert_eq!(overlay & WS_POPUP.0, WS_POPUP.0, "a bare popup");
    assert_eq!(
        overlay & (WS_VISIBLE.0 | WS_CLIPSIBLINGS.0),
        WS_VISIBLE.0 | WS_CLIPSIBLINGS.0,
        "visibility and clipping are left alone"
    );
    assert_eq!(overlay_style(overlay), overlay, "and it is idempotent");
}

#[test]
fn the_collapsed_strip_still_gets_pointer_samples() {
    let mut machine = Machine::new();
    machine.set_displays(vec![display("d1", false)]);
    machine.accept(data(ShowMode::OnHover));
    assert!(machine.plan().expect("plan").pill.is_some(), "collapsed");
    assert!(
        machine.pointer_poll_active(),
        "the strip is watched while the rail is closed"
    );
    let now = Instant::now();
    assert!(
        machine.deadline(now) <= now + POINTER_POLL,
        "and the loop wakes on the short cadence to do it"
    );
}

const ORDINARY: WindowAbove = WindowAbove {
    visible: true,
    cloaked: false,
    topmost: false,
};
const ALWAYS_ON_TOP: WindowAbove = WindowAbove {
    visible: true,
    cloaked: false,
    topmost: true,
};
const HIDDEN: WindowAbove = WindowAbove {
    visible: false,
    cloaked: false,
    topmost: false,
};
const CLOAKED: WindowAbove = WindowAbove {
    visible: true,
    cloaked: true,
    topmost: false,
};

#[test]
fn an_ordinary_window_stacked_above_the_notch_means_it_sank() {
    assert!(sunk_below_ordinary_windows([ALWAYS_ON_TOP, ORDINARY]));
    assert!(sunk_below_ordinary_windows([HIDDEN, CLOAKED, ORDINARY]));
}

#[test]
fn always_on_top_hidden_or_cloaked_windows_above_do_not_count_as_sinking() {
    assert!(!sunk_below_ordinary_windows([]));
    assert!(!sunk_below_ordinary_windows([ALWAYS_ON_TOP, ALWAYS_ON_TOP]));
    assert!(!sunk_below_ordinary_windows([HIDDEN]));
    assert!(!sunk_below_ordinary_windows([CLOAKED]));
}

static STACKING: Mutex<()> = Mutex::new(());

struct TestWindow(HWND);

impl Drop for TestWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.0);
        }
    }
}

fn shown_window(topmost: bool) -> TestWindow {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
    };
    let mut style = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    if topmost {
        style |= WS_EX_TOPMOST;
    }
    let hwnd = unsafe {
        CreateWindowExW(
            style,
            windows::core::w!("STATIC"),
            windows::core::w!("on-n-off stacking test"),
            WS_POPUP,
            37,
            41,
            1,
            1,
            None,
            None,
            None,
            None,
        )
    }
    .expect("create a test window");
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    TestWindow(hwnd)
}

fn place(window: &TestWindow, after: HWND) {
    unsafe {
        SetWindowPos(
            window.0,
            Some(after),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    }
    .expect("restack a test window");
}

fn is_above(upper: &TestWindow, lower: &TestWindow) -> bool {
    let next = |window: HWND| unsafe { GetWindow(window, GW_HWNDPREV) }.ok();
    std::iter::successors(next(lower.0), |window| next(*window)).any(|window| window == upper.0)
}

fn has_topmost_style(window: &TestWindow) -> bool {
    let style = unsafe { GetWindowLongPtrW(window.0, GWL_EXSTYLE) };
    style & WS_EX_TOPMOST.0 as isize != 0
}

fn bounds(window: &TestWindow) -> RECT {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(window.0, &mut rect) }.expect("read a test window's bounds");
    rect
}

fn sunk_overlay() -> (TestWindow, TestWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{HWND_NOTOPMOST, HWND_TOP};
    let overlay = shown_window(true);
    let ordinary = shown_window(false);
    place(&overlay, HWND_NOTOPMOST);
    place(&ordinary, HWND_TOP);
    assert!(
        is_above(&ordinary, &overlay),
        "the setup did not sink the overlay"
    );
    (overlay, ordinary)
}

#[test]
fn a_notch_sunk_below_an_ordinary_window_goes_back_on_top_where_it_was() {
    let _stacking = STACKING.lock().unwrap_or_else(|e| e.into_inner());
    let (overlay, ordinary) = sunk_overlay();
    let before = bounds(&overlay);

    keep_on_top(overlay.0, false);

    assert!(is_above(&overlay, &ordinary));
    assert!(has_topmost_style(&overlay));
    assert_eq!(bounds(&overlay), before);
    let active = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow() };
    assert_ne!(active, overlay.0, "the notch was activated");
}

#[test]
fn a_notch_under_only_always_on_top_windows_stays_where_it_is() {
    let _stacking = STACKING.lock().unwrap_or_else(|e| e.into_inner());
    let overlay = shown_window(true);
    let other = shown_window(true);
    place(&other, HWND_TOPMOST);
    assert!(is_above(&other, &overlay));

    keep_on_top(overlay.0, false);

    assert!(is_above(&other, &overlay));
}

#[test]
fn a_notch_hidden_for_a_full_screen_app_or_by_itself_is_not_raised() {
    let _stacking = STACKING.lock().unwrap_or_else(|e| e.into_inner());
    let (overlay, ordinary) = sunk_overlay();
    keep_on_top(overlay.0, true);
    assert!(
        is_above(&ordinary, &overlay),
        "raised over a full-screen app"
    );

    unsafe {
        let _ = ShowWindow(overlay.0, SW_HIDE);
    }
    keep_on_top(overlay.0, false);
    assert!(is_above(&ordinary, &overlay), "raised while hidden");
}

#[test]
fn the_screens_change_only_after_the_first_read() {
    let mut machine = Machine::new();
    assert!(
        !machine.set_displays(vec![display("d1", false)]),
        "the first read is what the host started with"
    );
    assert!(!machine.set_displays(vec![display("d1", false)]));
    assert!(machine.set_displays(vec![display("d1", false), display("d2", false)]));
    assert!(machine.set_displays(Vec::new()), "a read that failed");
    assert!(machine.set_displays(vec![display("d1", true)]));
}

#[test]
fn window_messages_queue_the_pointer_in_client_pixels() {
    use windows::Win32::UI::WindowsAndMessaging::{WM_MOUSEMOVE, WM_NULL};
    let at = |x: i16, y: i16| LPARAM(((y as u16 as isize) << 16) | x as u16 as isize);
    INPUT.with_borrow_mut(Vec::clear);
    for (message, lparam) in [
        (WM_MOUSEMOVE, at(-3, 7)),
        (WM_LBUTTONDOWN, at(40, -2)),
        (WM_MOUSELEAVE, LPARAM(0)),
        (WM_NULL, at(1, 1)),
    ] {
        unsafe { overlay_proc(HWND::default(), message, WPARAM(0), lparam) };
    }
    assert_eq!(
        INPUT.with_borrow_mut(std::mem::take),
        vec![
            Input::Moved { x: -3, y: 7 },
            Input::Pressed { x: 40, y: -2 },
            Input::Left,
        ]
    );
}

#[test]
fn window_input_reaches_the_machine_in_points_of_the_selected_display() {
    let mut machine = Machine::new();
    machine.accept(data(ShowMode::Always));
    let mut scaled = display("d1", false);
    scaled.scale = 1.5;
    machine.set_displays(vec![scaled]);
    let device = |points: f64| (points * 1.5).round() as i32;

    let cap = machine.plan().unwrap().cap.rect;
    assert_eq!(
        machine.input(
            Input::Pressed {
                x: device(cap.mid_x()),
                y: device(cap.mid_y()),
            },
            Instant::now(),
        ),
        vec![WinAction::SetShow(ShowMode::OnHover)]
    );

    let cell = cell_rect(&machine);
    let entered = Instant::now();
    let moved = Input::Moved {
        x: device(cell.mid_x()),
        y: device(cell.mid_y()),
    };
    assert!(machine.input(moved, entered).is_empty());
    machine.advance(entered + HOVER_OPEN_DELAY);
    assert_eq!(machine.hover.active, Some(0));

    assert!(machine.input(Input::Left, entered).is_empty());
    assert!(!machine.cursor_inside);
}

fn notch_windows() -> Vec<HWND> {
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};
    unsafe extern "system" fn collect(window: HWND, found: LPARAM) -> windows::core::BOOL {
        let mut process = 0;
        let mut class = [0u16; 64];
        let len = unsafe {
            GetWindowThreadProcessId(window, Some(&mut process));
            GetClassNameW(window, &mut class)
        };
        let class = String::from_utf16_lossy(&class[..len.max(0) as usize]);
        if process == std::process::id() && class == unsafe { CLASS.to_string() }.unwrap() {
            unsafe { &mut *(found.0 as *mut Vec<HWND>) }.push(window);
        }
        true.into()
    }
    let mut found = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut found as *mut Vec<HWND> as isize));
    }
    found
}

fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "{what}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn open_notch() -> (Sender<WindowMsg>, HWND) {
    let (action_tx, _actions) = std::sync::mpsc::channel();
    let messages = spawn(action_tx);
    eventually("the notch window never opened", || {
        notch_windows().len() == 1
    });
    (messages, notch_windows()[0])
}

fn close_notch(messages: &Sender<WindowMsg>) {
    messages
        .send(WindowMsg::Shutdown)
        .expect("the notch thread takes the shutdown");
    eventually("the notch thread kept running after Shutdown", || {
        messages.send(WindowMsg::Ping).is_err()
    });
    assert!(
        notch_windows().is_empty(),
        "the notch window outlived its thread"
    );
}

const BESIDE_TAO_DONE: &str = "the notch ran beside a tao event loop and stopped";

#[test]
#[ignore = "runs in its own process, started by the_notch_runs_beside_the_apps_own_tao_event_loop_and_stops_without_ending_the_process"]
fn notch_beside_a_tao_event_loop() {
    use tao::platform::windows::EventLoopBuilderExtWindows;
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, GWL_STYLE, SMTO_ABORTIFHUNG, WM_MOUSEMOVE, WS_CAPTION, WS_THICKFRAME,
    };
    let app_loop = tao::event_loop::EventLoopBuilder::<String>::with_user_event()
        .with_any_thread(true)
        .build();
    let _app_window = tao::window::WindowBuilder::new()
        .with_visible(false)
        .build(&app_loop)
        .expect("the app's own window");

    let (messages, notch) = open_notch();
    let ex_style = unsafe { GetWindowLongPtrW(notch, GWL_EXSTYLE) } as u32;
    let wanted = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
    assert_eq!(
        ex_style & wanted.0,
        wanted.0,
        "extended style {ex_style:#x}"
    );
    let style = unsafe { GetWindowLongPtrW(notch, GWL_STYLE) } as u32;
    assert_ne!(style & WS_POPUP.0, 0, "style {style:#x}");
    assert_eq!(
        style & (WS_CAPTION.0 | WS_THICKFRAME.0),
        0,
        "style {style:#x}"
    );

    messages
        .send(WindowMsg::Data(data(ShowMode::Always)))
        .expect("the notch thread takes data");
    for message in [WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_MOUSELEAVE] {
        let answered = unsafe {
            SendMessageTimeoutW(
                notch,
                message,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                5_000,
                None,
            )
        };
        assert_ne!(
            answered.0, 0,
            "the notch did not answer message {message:#x}"
        );
    }
    assert!(
        messages.send(WindowMsg::Ping).is_ok(),
        "the notch thread died on its own input"
    );
    close_notch(&messages);

    let (messages, _) = open_notch();
    close_notch(&messages);
    println!("{BESIDE_TAO_DONE}");
}

#[test]
fn the_notch_runs_beside_the_apps_own_tao_event_loop_and_stops_without_ending_the_process() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "side_notch::win_window::tests::notch_beside_a_tao_event_loop",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .expect("run the notch in its own test process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains(BESIDE_TAO_DONE),
        "{}
{stdout}
{stderr}",
        output.status
    );
}
