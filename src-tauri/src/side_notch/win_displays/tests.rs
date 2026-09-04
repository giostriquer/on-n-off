use super::*;
use crate::side_notch::model::Display;

fn raw(device: &str, x: i32, y: i32, w: i32, h: i32, dpi: u32) -> RawMonitor {
    RawMonitor {
        device: device.into(),
        x,
        y,
        width: w,
        height: h,
        work_y: y,
        work_height: h,
        dpi,
    }
}

#[test]
fn physical_pixels_become_points_at_the_monitor_scale() {
    let monitor = raw("\\\\.\\DISPLAY1", 1920, 0, 3840, 2160, 192);
    let display = to_display(&monitor, "edid:unit", "Dell U4323QE");
    assert_eq!(display.id, "edid:unit");
    assert_eq!(display.name, "Dell U4323QE");
    assert_eq!(display.scale, 2.0);
    assert_eq!(
        display.x, 960.0,
        "the monitor sits at 1920 physical, 960 points"
    );
    assert_eq!(display.y, 0.0);
    assert_eq!(display.width, 1920.0);
    assert_eq!(display.height, 1080.0);
    assert_eq!(display.work_height, 1080.0);
    assert!(!display.mirrored);
}

#[test]
fn a_work_area_that_excludes_the_taskbar_survives_the_point_conversion() {
    let mut monitor = raw("\\\\.\\DISPLAY1", 0, 0, 2560, 1440, 120);
    monitor.work_y = 0;
    monitor.work_height = 1392; // taskbar takes 48 physical pixels
    let display = to_display(&monitor, "id", "name");
    assert_eq!(display.scale, 1.25);
    assert_eq!(display.work_height, 1113.6);
    assert_eq!(display.height, 1152.0);
}

#[test]
fn two_active_monitors_sharing_a_desktop_rect_are_mirrored() {
    let mut displays = vec![
        to_display(&raw("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96), "a", "A"),
        to_display(&raw("\\\\.\\DISPLAY2", 0, 0, 1920, 1080, 96), "b", "B"),
        to_display(&raw("\\\\.\\DISPLAY3", 1920, 0, 1920, 1080, 96), "c", "C"),
    ];
    apply_mirroring(&mut displays, false);
    assert!(displays[0].mirrored && displays[1].mirrored);
    assert!(!displays[2].mirrored);
}

#[test]
fn a_cloned_gdi_source_marks_every_reported_display_mirrored() {
    // Duplicate mode enumerates ONE GDI monitor; the topology shows two active paths
    // sharing the same source.
    let cloned = [raw("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96)];
    let mut displays = vec![to_display(&cloned[0], "a", "A")];
    let duplicated = paths_duplicated(&[(1, 2, 0), (1, 2, 0)]);
    apply_mirroring(&mut displays, duplicated);
    assert!(
        displays[0].mirrored,
        "duplicate mode reports one GDI monitor and the topology says cloned"
    );
    assert!(!paths_duplicated(&[(1, 2, 0), (1, 3, 0)]));
    assert!(!paths_duplicated(&[]));
}

#[test]
fn incomplete_display_information_is_detected_before_caching() {
    let display = to_display(&raw("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96), "", "A");
    assert!(display.id.is_empty());
    let bad = Display {
        scale: 0.0,
        ..to_display(&raw("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96), "a", "A")
    };
    assert!(bad.scale <= 0.0);
}

#[test]
fn topology_changes_whenever_any_monitor_input_changes() {
    let base = vec![raw("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96)];
    let moved = vec![raw("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 144)];
    let extra = vec![
        raw("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96),
        raw("\\\\.\\DISPLAY2", 1920, 0, 1920, 1080, 96),
    ];
    let same = vec![raw("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96)];
    assert_ne!(
        topology(&base),
        topology(&moved),
        "DPI change busts the cache"
    );
    assert_ne!(
        topology(&base),
        topology(&extra),
        "a new monitor busts the cache"
    );
    assert_eq!(
        topology(&base),
        topology(&same),
        "unchanged inputs reuse the cache"
    );
}

#[test]
fn the_shared_rect_fallback_signals_mirroring_without_the_topology() {
    let pair = [
        raw("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96),
        raw("\\\\.\\DISPLAY2", 0, 0, 1920, 1080, 96),
    ];
    assert!(shared_rects(&pair));
    let side_by_side = [
        raw("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96),
        raw("\\\\.\\DISPLAY2", 1920, 0, 1920, 1080, 96),
    ];
    assert!(!shared_rects(&side_by_side));
}
