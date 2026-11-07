use super::*;
use serde::Deserialize;

const CASES: &str =
    include_str!("../../../../macos/SideNotch/Tests/NotchCoreTests/rail-cases.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fixture {
    work_area: WorkArea,
    rails: Vec<RailCase>,
    metrics: Vec<MetricsCase>,
    popovers: Vec<PopoverCase>,
    placements: Vec<PlacementCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkArea {
    width: f64,
    height: f64,
    work_y: f64,
    work_height: f64,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Screen {
    id: String,
    x: Option<f64>,
    width: Option<f64>,
    work_y: Option<f64>,
    work_height: Option<f64>,
    scale: Option<f64>,
    mirrored: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RailCase {
    displays: Option<Vec<Screen>>,
    scale: f64,
    size: NotchSize,
    edge: Edge,
    cells: Option<usize>,
    enabled: Option<bool>,
    rail: Option<[f64; 4]>,
    pill: Option<[f64; 4]>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetricsCase {
    scale: f64,
    size: NotchSize,
    thickness: f64,
    cell_length: f64,
    cell_spacing: f64,
    inset: f64,
    cell_padding: f64,
    content_spacing: f64,
    icon_slot: f64,
    label_height: f64,
    ring_stroke: f64,
    inner_ring_stroke: f64,
    inner_ring_inset: f64,
    glyph_size: f64,
    label_size: f64,
    label_nudge: f64,
    ring_radius: f64,
    inner_ring_radius: f64,
    pin_size: f64,
    cap_pins: CapPins,
    right_cells: Vec<[f64; 4]>,
    top_cells: Vec<[f64; 4]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapPins {
    right: [f64; 2],
    left: [f64; 2],
    top: [f64; 2],
    bottom: [f64; 2],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PopoverCase {
    displays: Option<Vec<Screen>>,
    scale: f64,
    size: NotchSize,
    width: f64,
    padding: f64,
    tail_length: f64,
    tail_half_width: f64,
    corner_radius: f64,
    border: f64,
    max_height: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlacementCase {
    displays: Option<Vec<Screen>>,
    scale: f64,
    size: NotchSize,
    edge: Edge,
    cell: [f64; 4],
    fitting: [f64; 2],
    frame: [f64; 4],
    card: [f64; 4],
}

fn fixture() -> Fixture {
    serde_json::from_str(CASES).expect("the shared rail cases parse")
}

fn pixels(frame: Option<Layout>, scale: f64) -> Option<[f64; 4]> {
    frame.map(|frame| [frame.x, frame.y, frame.width, frame.height].map(|points| points * scale))
}

fn assert_pixels<const N: usize>(actual: Option<[f64; N]>, expected: Option<[f64; N]>, what: &str) {
    let matches = match (actual, expected) {
        (None, None) => true,
        (Some(actual), Some(expected)) => actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() < 1e-6),
        _ => false,
    };
    assert!(matches, "{what}: {actual:?} != {expected:?}");
}

fn displays(area: &WorkArea, screens: Option<&[Screen]>, scale: f64) -> Vec<Display> {
    let main = [Screen {
        id: "main".into(),
        ..Screen::default()
    }];
    screens
        .unwrap_or(&main)
        .iter()
        .map(|screen| Display {
            id: screen.id.clone(),
            name: screen.id.clone(),
            x: screen.x.unwrap_or(0.0),
            y: 0.0,
            width: screen.width.unwrap_or(area.width),
            height: area.height,
            work_y: screen.work_y.unwrap_or(area.work_y),
            work_height: screen.work_height.unwrap_or(area.work_height),
            scale: screen.scale.unwrap_or(scale),
            mirrored: screen.mirrored.unwrap_or(false),
        })
        .collect()
}

#[test]
fn every_rail_frame_and_pill_matches_the_shared_cases() {
    let fixture = fixture();
    assert!(!fixture.rails.is_empty());
    for (index, case) in fixture.rails.iter().enumerate() {
        let displays = displays(&fixture.work_area, case.displays.as_deref(), case.scale);
        let current = NotchSettings {
            enabled: case.enabled.unwrap_or(true),
            size: case.size,
            ..settings("main", case.edge)
        };
        let cells = case.cells.unwrap_or(RAIL_ORDER.len());
        assert_pixels(
            pixels(layout(&current, &displays, cells), case.scale),
            case.rail,
            &format!("rail case {index}"),
        );
        assert_pixels(
            pixels(pill_frame(&current, &displays, cells), case.scale),
            case.pill,
            &format!("pill case {index}"),
        );
    }
}

#[test]
fn every_rail_metric_matches_the_shared_cases() {
    let fixture = fixture();
    assert!(!fixture.metrics.is_empty());
    for case in fixture.metrics {
        let m = rail_metrics(case.size, case.scale, Edge::Right);
        assert_pixels(
            Some(
                [
                    m.thickness,
                    m.cell_length,
                    m.cell_spacing,
                    m.inset,
                    m.cell.padding,
                    m.cell.content_spacing,
                    m.cell.icon_slot,
                    m.cell.label_height,
                    m.cell.ring_stroke,
                    m.cell.inner_ring_stroke,
                    m.cell.inner_ring_inset,
                    m.cell.glyph,
                ]
                .map(|points| points * case.scale),
            ),
            Some([
                case.thickness,
                case.cell_length,
                case.cell_spacing,
                case.inset,
                case.cell_padding,
                case.content_spacing,
                case.icon_slot,
                case.label_height,
                case.ring_stroke,
                case.inner_ring_stroke,
                case.inner_ring_inset,
                case.glyph_size,
            ]),
            &format!("{:?} at {}x", case.size, case.scale),
        );
        assert_pixels(
            Some(
                [
                    m.cell.label_size,
                    m.cell.label_nudge,
                    m.cell.ring_radius(),
                    m.cell.inner_ring_radius(),
                    m.pin,
                ]
                .map(|points| points * case.scale),
            ),
            Some([
                case.label_size,
                case.label_nudge,
                case.ring_radius,
                case.inner_ring_radius,
                case.pin_size,
            ]),
            &format!("{:?} chrome at {}x", case.size, case.scale),
        );
        for (edge, pin) in [
            (Edge::Right, case.cap_pins.right),
            (Edge::Left, case.cap_pins.left),
            (Edge::Top, case.cap_pins.top),
            (Edge::Bottom, case.cap_pins.bottom),
        ] {
            let (x, y) = rail_metrics(case.size, case.scale, edge).cap_pin(edge);
            assert_pixels(
                Some([x * case.scale, y * case.scale]),
                Some(pin),
                &format!("{edge:?} cap pin of {:?} at {}x", case.size, case.scale),
            );
        }
        let across = rail_metrics(case.size, case.scale, Edge::Top);
        assert_eq!(
            (across.thickness, across.cell_length),
            (m.cell_length, m.thickness)
        );
        for (edge, cells) in [
            (Edge::Right, &case.right_cells),
            (Edge::Top, &case.top_cells),
        ] {
            assert!(!cells.is_empty());
            let metrics = rail_metrics(case.size, case.scale, edge);
            let frames = rail_cell_frames(edge, &metrics, cells.len());
            for (index, (frame, cell)) in frames.into_iter().zip(cells).enumerate() {
                assert_pixels(
                    pixels(Some(frame), case.scale),
                    Some(*cell),
                    &format!(
                        "{edge:?} cell {index} of {:?} at {}x",
                        case.size, case.scale
                    ),
                );
            }
        }
    }
}

#[test]
fn cells_tile_the_rail() {
    let metrics = rail_metrics(NotchSize::Standard, 1.0, Edge::Right);
    let cells = rail_cell_frames(Edge::Right, &metrics, 4);
    assert_eq!(cells.len(), 4);
    assert_eq!(cells[0].y, metrics.ear());
    assert_eq!(
        cells[3].y + cells[3].height,
        metrics.length(4) - metrics.ear()
    );
    let bar = rail_metrics(NotchSize::Standard, 1.0, Edge::Top);
    assert_eq!(bar.thickness, metrics.cell_length);
    assert_eq!(
        rail_cell_frames(Edge::Top, &bar, 2)[1].x,
        bar.inset + bar.cell_length + bar.cell_spacing
    );
    assert_eq!(
        cells[0].y,
        metrics.length(4) - (cells[3].y + cells[3].height)
    );
}

#[test]
fn every_popover_measure_matches_the_shared_cases() {
    let fixture = fixture();
    assert!(!fixture.popovers.is_empty());
    for case in fixture.popovers {
        let display = &displays(&fixture.work_area, case.displays.as_deref(), case.scale)[0];
        let chrome = popover_metrics(case.size, display);
        assert_pixels(
            Some(
                [
                    chrome.width,
                    chrome.padding,
                    chrome.tail_length,
                    chrome.tail_half_width,
                    chrome.corner_radius,
                    chrome.border,
                    chrome.max_height,
                ]
                .map(|points| points * case.scale),
            ),
            Some([
                case.width,
                case.padding,
                case.tail_length,
                case.tail_half_width,
                case.corner_radius,
                case.border,
                case.max_height,
            ]),
            &format!(
                "popover of {:?} at {}x on {}",
                case.size, case.scale, display.work_height
            ),
        );
    }
}

#[test]
fn every_popover_placement_matches_the_shared_cases() {
    let fixture = fixture();
    assert!(!fixture.placements.is_empty());
    for (index, case) in fixture.placements.iter().enumerate() {
        let display = &displays(&fixture.work_area, case.displays.as_deref(), case.scale)[0];
        let chrome = popover_metrics(case.size, display);
        let [x, y, width, height] = case.cell;
        let cell = Layout {
            x,
            y,
            width,
            height,
        };
        let frame = chrome.frame(&cell, case.edge, (case.fitting[0], case.fitting[1]));
        assert_pixels(
            pixels(Some(frame), case.scale),
            Some(case.frame),
            &format!("placement {index} frame"),
        );
        assert_pixels(
            pixels(Some(chrome.card(&frame, case.edge)), case.scale),
            Some(case.card),
            &format!("placement {index} card"),
        );
    }
}
