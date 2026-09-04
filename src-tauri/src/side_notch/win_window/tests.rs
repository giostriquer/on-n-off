use super::*;
use crate::dto::AgentId;
use crate::dto::{LimitWindowDto, LimitWindowKind, LimitsStatus};
use crate::side_notch::model::{Display, NotchSettings};
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
    RailData {
        settings: settings(show),
        cells: vec![win_paint::CellData::Provider(win_paint::ProviderData {
            provider: AgentId::Claude,
            status: LimitsStatus::Ok,
            message: None,
            windows: vec![LimitWindowDto {
                id: "w".into(),
                label: "Current session".into(),
                kind: LimitWindowKind::Session,
                used_percent: 10.0,
                resets_at: None,
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
            }],
            sessions: Vec::new(),
        })],
        action_error: None,
    }
}

/// A rail of one cell, for hit-testing: the cell sits after the 40 pt ear.
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
fn the_hover_strip_opens_the_rail_and_the_grace_closes_it() {
    let mut machine = Machine::new();
    machine.accept(data(ShowMode::OnHover));
    machine.set_displays(vec![display("d1", false)]);
    assert!(
        machine.plan().expect("plan").pill.is_some(),
        "collapsed first"
    );

    // The pointer lands on the strip: the rail opens at once.
    let pill = machine.plan().unwrap().pill.unwrap();
    machine.cursor_at(pill.mid_x(), pill.mid_y(), true, Instant::now());
    assert!(machine.hover.rail_open);

    // Hovering the cell schedules the popover after the open delay.
    let cell = cell_rect(&machine);
    let entered = Instant::now();
    machine.cursor_at(cell.mid_x(), cell.mid_y(), true, entered);
    assert!(machine.hover.active.is_none(), "not before the delay");
    machine.advance(entered + HOVER_OPEN_DELAY + Duration::from_millis(5));
    assert_eq!(machine.hover.active, Some(0));

    // Leaving and waiting out the grace closes the popover and the rail.
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

    // A click off the rail pins nothing.
    let cell = cell_rect(&machine);
    assert!(machine
        .clicked(cell.mid_x() + 1000.0, cell.mid_y())
        .is_empty());
    assert_eq!(machine.pinned, None);

    // Pinning opens the popover, which widens the window and shifts every
    // window-local rect; the test re-reads the plan after each state change.
    let cell = cell_rect(&machine);
    machine.clicked(cell.mid_x(), cell.mid_y());
    assert_eq!(machine.pinned, Some(0));
    assert_eq!(machine.hover.active, Some(0));

    // The popover survives the pointer leaving (a fresh pin is not dropped by
    // the close grace).
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
    // What the low-level mouse hook drives. The hook is armed only while something is
    // pinned and is torn down with the window thread that installed it, so this is the
    // one path that has to keep working across a restart of that thread.
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

    // Nothing pinned and nothing hovered: no repaint to ask for.
    machine.dismiss();
    assert!(!machine.take_dirty(), "an idle dismissal is not a change");
}

#[test]
fn clicking_the_cap_asks_for_the_show_mode_to_flip() {
    // Always -> OnHover: the cap is visible on the open rail.
    let mut machine = Machine::new();
    machine.accept(data(ShowMode::Always));
    machine.set_displays(vec![display("d1", false)]);
    let cap = machine.plan().unwrap().cap.rect;
    assert_eq!(
        machine.clicked(cap.mid_x(), cap.mid_y()),
        vec![WinAction::SetShow(ShowMode::OnHover)]
    );

    // OnHover -> Always: the rail must be opened by the strip first.
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
    // An idle machine sleeps for the screen poll, not for a frame.
    let idle = Machine::new();
    assert!(idle.deadline(entered) >= entered + Duration::from_millis(100));

    // A hovered cell must wake the loop before the hover-open delay elapses.
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
    // What tao leaves on an undecorated window: a caption and a resize frame, whose
    // invisible border shrinks the client area the layered surface is clipped to.
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
    // tao reports no `CursorMoved` for this non-activating layered window, so the
    // poll is the only way the hover strip can notice the pointer reaching it.
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
