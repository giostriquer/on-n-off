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

#[test]
fn every_rail_frame_and_pill_matches_the_shared_cases() {
    let fixture = fixture();
    assert!(!fixture.rails.is_empty());
    let area = &fixture.work_area;
    let main = [Screen {
        id: "main".into(),
        ..Screen::default()
    }];
    for (index, case) in fixture.rails.iter().enumerate() {
        let displays: Vec<Display> = case
            .displays
            .as_deref()
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
                scale: screen.scale.unwrap_or(case.scale),
                mirrored: screen.mirrored.unwrap_or(false),
            })
            .collect();
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
        let across = rail_metrics(case.size, case.scale, Edge::Top);
        assert_eq!(
            (across.thickness, across.cell_length),
            (m.cell_length, m.thickness)
        );
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
