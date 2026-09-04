use std::time::Duration;

use super::{
    hides_on_close, hides_on_focus_loss, menu_action, popover_position, tray_click_action,
    MenuAction, PixelPoint, PixelRect, PixelSize, TrayClickAction, HIDES_ALL_ON_CLOSE,
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

/// Both policies, asserted on both CI legs — `hides_all` is a parameter precisely so that
/// neither leg has to take the other's branch on trust.
#[test]
fn hides_a_window_on_close_only_where_something_will_bring_it_back() {
    // A platform that hides everything (macOS): the status item is the app's home, so the
    // setting cannot make it quit instead.
    assert!(hides_on_close("main", true, false));
    assert!(hides_on_close("main", true, true));
    assert!(hides_on_close("limits-popover", true, false));

    // A platform that does not (Windows): the main window only, and only when asked.
    assert!(hides_on_close("main", false, true));
    assert!(!hides_on_close("main", false, false));
    assert!(!hides_on_close("limits-popover", false, true));

    // Every other window closes for real on either platform.
    assert!(!hides_on_close("settings", true, true));
    assert!(!hides_on_close("settings", false, true));
}

#[test]
fn this_platform_is_wired_to_the_policy_it_should_have() {
    assert_eq!(HIDES_ALL_ON_CLOSE, cfg!(target_os = "macos"));
}

/// The id the menu is built with must be the id a click is looked up by. One list owns both;
/// this is what holds them together.
#[test]
fn every_tray_menu_id_round_trips_to_its_action() {
    for (action, label) in MenuAction::ENTRIES {
        assert_eq!(menu_action(action.id()), Some(action), "{label}");
    }
    assert_eq!(menu_action("tray-nonsense"), None);
}

/// The only test that touches the process-global mirror, so it cannot race another.
#[cfg(target_os = "windows")]
#[test]
fn a_saved_flag_reaches_the_close_handler() {
    use super::{close_to_tray, set_close_to_tray};

    set_close_to_tray(true);
    assert!(close_to_tray());
    set_close_to_tray(false);
    assert!(!close_to_tray());
}

#[test]
fn dismisses_only_the_popover_when_it_loses_focus() {
    assert!(hides_on_focus_loss("limits-popover", false));
    assert!(!hides_on_focus_loss("limits-popover", true));
    assert!(!hides_on_focus_loss("main", false));
}
