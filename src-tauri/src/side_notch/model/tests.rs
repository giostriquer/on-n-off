use super::*;

fn display(id: &str, x: f64, scale: f64) -> Display {
    Display {
        id: id.into(),
        name: "Same monitor name".into(),
        x,
        y: 0.0,
        width: 1728.0,
        height: 1117.0,
        work_y: 33.0,
        work_height: 1084.0,
        scale,
        mirrored: false,
    }
}

fn settings(id: &str, edge: Edge) -> NotchSettings {
    NotchSettings {
        enabled: true,
        display_id: Some(id.into()),
        edge,
        // The placement tests measure a four-cell rail; which providers a fresh
        // install rails is a separate question.
        providers: RAIL_ORDER.to_vec(),
        pull_requests: NotchPullRequests {
            enabled: false,
            lists: vec![GithubList::Mine],
        },
        ..NotchSettings::default()
    }
}

#[test]
fn follows_the_selected_uuid_across_reordering_and_mixed_scales() {
    let displays = vec![
        display("external", 0.0, 1.0),
        display("retina", -1728.0, 2.0),
    ];
    // Four cells: 4 × 73 + 3 × 8 + 2 × 40 = 396.
    assert_eq!(
        layout(&settings("retina", Edge::Right), &displays),
        Some(Layout {
            x: -76.0,
            y: 377.0,
            width: 76.0,
            height: 396.0
        })
    );
    let reversed: Vec<_> = displays.into_iter().rev().collect();
    assert_eq!(
        layout(&settings("retina", Edge::Left), &reversed),
        Some(Layout {
            x: -1728.0,
            y: 377.0,
            width: 76.0,
            height: 396.0
        })
    );
}

#[test]
fn never_falls_back_to_another_display_when_the_selection_is_missing_or_mirrored() {
    let mut current = settings("missing", Edge::Left);
    let mut displays = vec![display("other", 0.0, 1.0)];
    assert_eq!(layout(&current, &displays), None);
    current.display_id = Some("other".into());
    displays[0].mirrored = true;
    assert_eq!(layout(&current, &displays), None);
}

#[test]
fn top_and_bottom_edges_center_a_horizontal_rail_inside_the_work_area() {
    let displays = [display("main", 100.0, 2.0)];
    // Four cells side by side: 4 × 76 + 3 × 8 + 2 × 40 = 408, as tall as a cell.
    let top = layout(&settings("main", Edge::Top), &displays).unwrap();
    assert_eq!(
        top,
        Layout {
            x: 100.0 + (1728.0 - 408.0) / 2.0,
            y: 33.0,
            width: 408.0,
            height: 73.0
        }
    );
    let bottom = layout(&settings("main", Edge::Bottom), &displays).unwrap();
    assert_eq!(bottom.y, 33.0 + 1084.0 - 73.0);
    assert_eq!(bottom.x, top.x);
}

#[test]
fn the_rail_shrinks_with_fewer_providers_and_hides_without_any() {
    let displays = [display("main", 0.0, 1.0)];
    let mut current = settings("main", Edge::Right);
    current.providers = vec![AgentId::Cursor, AgentId::Claude, AgentId::Claude];
    assert_eq!(current.rail_providers(), [AgentId::Claude, AgentId::Cursor]);
    assert_eq!(layout(&current, &displays).unwrap().height, 234.0);
    current.providers.clear();
    assert_eq!(layout(&current, &displays), None);
}

#[test]
fn a_rail_that_does_not_fit_stays_hidden_instead_of_overflowing() {
    let mut small = display("small", 0.0, 1.0);
    small.work_height = 300.0;
    assert_eq!(
        layout(&settings("small", Edge::Right), &[small.clone()]),
        None
    );
    small.width = 300.0;
    small.work_height = 1000.0;
    assert_eq!(layout(&settings("small", Edge::Top), &[small]), None);
}

#[test]
fn the_pull_request_cell_is_on_by_default_with_only_the_users_own_list() {
    let defaults = NotchSettings::default();
    assert!(defaults.pull_requests.enabled);
    assert_eq!(defaults.pull_requests.lists, [GithubList::Mine]);
    assert_eq!(defaults.cell_count(), 3, "two providers plus the cell");
    let legacy: NotchSettings =
        serde_json::from_str(r#"{"enabled":true,"displayId":"main","edge":"right"}"#).unwrap();
    assert_eq!(
        legacy.cell_count(),
        5,
        "older documents gain the cell and keep their providers"
    );
    let mut current = settings("main", Edge::Right);
    current.pull_requests = NotchPullRequests {
        enabled: true,
        lists: vec![GithubList::Assigned, GithubList::Mine, GithubList::Assigned],
    };
    assert_eq!(
        current.pull_requests.selected_lists(),
        [GithubList::Mine, GithubList::Assigned]
    );
    let displays = [display("main", 0.0, 1.0)];
    assert_eq!(
        layout(&current, &displays).unwrap().height,
        rail_length(5, CELL_HEIGHT)
    );
    let json = serde_json::to_value(&current).unwrap();
    assert_eq!(json["pullRequests"]["enabled"], true);
    assert_eq!(json["pullRequests"]["lists"][0], "assigned");
}

#[test]
fn remains_hidden_until_explicitly_enabled() {
    assert_eq!(
        layout(&NotchSettings::default(), &[display("main", 0.0, 1.0)]),
        None
    );
}

#[test]
fn legacy_documents_default_to_always_show_all_providers_and_the_standard_size() {
    let legacy: NotchSettings =
        serde_json::from_str(r#"{"enabled":true,"displayId":"main","edge":"right"}"#).unwrap();
    assert_eq!(legacy.size, NotchSize::Standard);
    assert_eq!(legacy.show, ShowMode::Always);
    assert_eq!(legacy.providers, RAIL_ORDER.to_vec());
    let hover: NotchSettings = serde_json::from_str(
        r#"{"enabled":true,"displayId":"main","edge":"top","show":"onHover","providers":["codex"]}"#,
    )
    .unwrap();
    assert_eq!(hover.show, ShowMode::OnHover);
    assert_eq!(hover.edge, Edge::Top);
    assert_eq!(hover.providers, vec![AgentId::Codex]);
    let json = serde_json::to_value(&hover).unwrap();
    assert_eq!(json["show"], "onHover");
    assert_eq!(json["edge"], "top");
}

#[test]
fn presets_scale_the_whole_rail() {
    let displays = [display("main", 0.0, 1.0)];
    for (size, scale) in [
        (NotchSize::Compact, 0.875),
        (NotchSize::Standard, 1.0),
        (NotchSize::Large, 1.125),
    ] {
        let mut current = settings("main", Edge::Right);
        current.size = size;
        let rail = layout(&current, &displays).unwrap();
        assert_eq!(rail.width, CELL_WIDTH * scale);
        assert_eq!(rail.height, rail_length(4, CELL_HEIGHT) * scale);
    }
}

#[test]
fn the_rail_lands_on_whole_device_pixels() {
    // A work area that does not divide evenly leaves the rail on a half pixel. The
    // window origin is the union with the popover, so opening a popover above the rail
    // shifts that half pixel into the rail's own drawing and the cells visibly jump.
    let displays = vec![Display {
        id: "d1".into(),
        name: "d1".into(),
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
        work_y: 0.0,
        work_height: 1032.0,
        scale: 1.0,
        mirrored: false,
    }];
    let settings = NotchSettings {
        enabled: true,
        display_id: Some("d1".into()),
        ..NotchSettings::default()
    };
    let frame = layout(&settings, &displays).expect("fits");
    assert_eq!(
        frame.y.fract(),
        0.0,
        "the rail starts on a pixel: {}",
        frame.y
    );
    assert_eq!(frame.x.fract(), 0.0, "and so does its edge: {}", frame.x);

    // The same on a 150 % display, where a pixel is two thirds of a point.
    let mut scaled = displays.clone();
    scaled[0].scale = 1.5;
    let frame = layout(&settings, &scaled).expect("fits");
    assert_eq!(
        (frame.y * 1.5).fract(),
        0.0,
        "aligned to the display grid, not to points: {}",
        frame.y
    );
}

#[test]
fn a_fresh_notch_only_rails_the_providers_with_limits_to_show() {
    // Antigravity publishes no subscription limits and Cursor only does on some
    // setups, so a rail full of dashes is not a good first run; both are one toggle
    // away in the settings card.
    let fresh = NotchSettings::default();
    assert_eq!(
        fresh.providers,
        vec![AgentId::Claude, AgentId::Codex],
        "the two with quotas to draw"
    );
    // A settings file that names them keeps them.
    let chosen = NotchSettings {
        providers: RAIL_ORDER.to_vec(),
        ..NotchSettings::default()
    };
    assert_eq!(
        chosen.rail_providers().len(),
        4,
        "an explicit choice stands"
    );
}

/// The twin of `NotchCoreChecks`' ramp group. The meter used to step to a light amber at 70 %, so
/// a window that was filling up went paler and yellower exactly as it ran out. Whatever shape the
/// ramp takes, a fuller window must never sit further from the trip red than a less full one, and
/// inside the band each step must actually move.
#[test]
fn meter_ramp_only_ever_moves_toward_the_trip_red() {
    fn distance_to_trip(color: Color) -> f64 {
        let square = |a: u8, b: u8| (f64::from(a) - f64::from(b)).powi(2);
        (square(color[0], TRIP_RED[0])
            + square(color[1], TRIP_RED[1])
            + square(color[2], TRIP_RED[2]))
        .sqrt()
    }
    // Claude, Fable, Codex, Cursor and Antigravity: every accent the meter can be handed.
    let accents: [Color; 5] = [
        [217, 119, 87, 255],
        [204, 98, 64, 255],
        [238, 240, 242, 255],
        [122, 162, 255, 255],
        [140, 147, 157, 255],
    ];
    for base in accents {
        let mut previous = f64::INFINITY;
        for step in 0..=100 {
            let distance = distance_to_trip(meter_color(Some(f64::from(step)), base));
            assert!(
                distance <= previous + f64::EPSILON,
                "ramp backtracks at {step} %: {distance} > {previous}"
            );
            previous = distance;
        }
        // Sampled rather than per-point: the blend quantises to 8 bits, so two adjacent percentages
        // can legitimately round to the same colour near the top of the band. Across these spans it
        // must still move, which is what a ramp that stopped interpolating would fail.
        for pair in [(71, 75), (75, 80), (80, 85), (85, 89)] {
            let (low, high) = pair;
            let nearer = distance_to_trip(meter_color(Some(f64::from(high)), base));
            let farther = distance_to_trip(meter_color(Some(f64::from(low)), base));
            assert!(
                nearer < farther,
                "ramp stalls between {low} % and {high} %: {nearer} is no closer than {farther}"
            );
        }
        assert_eq!(
            meter_color(Some(70.0), base),
            base,
            "70 % is still the accent"
        );
        assert_eq!(
            meter_color(Some(90.0), base),
            TRIP_RED,
            "90 % is the trip red"
        );
        // Pins the easing: a quarter of the way through the band is half the way to red. A linear
        // blend would put a quarter here, and nothing else in this test would notice.
        assert_eq!(
            meter_color(Some(75.0), base),
            mix(base, TRIP_RED, 0.5),
            "the blend is eased, not linear"
        );
    }
    assert_eq!(meter_color(None, accents[0]), UNREADABLE_INK);
}
