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
