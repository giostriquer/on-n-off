use super::*;
use crate::side_notch::model::NotchSize;

#[test]
fn the_painted_rail_ends_one_ear_past_its_last_cell() {
    for size in [NotchSize::Compact, NotchSize::Standard, NotchSize::Large] {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for edge in [Edge::Right, Edge::Left, Edge::Top, Edge::Bottom] {
                let current = NotchSettings {
                    size,
                    edge,
                    ..settings()
                };
                let cells = (0..crate::side_notch::model::frame::cell_count(&current))
                    .map(|_| CellData::Provider(session_only(AgentId::Claude, 40.0)))
                    .collect();
                let rail_data = RailData {
                    settings: current.clone(),
                    cells,
                    action_error: None,
                };
                let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, scale)];
                let planned =
                    plan(&current, &displays, &rail_data, Hover::default()).expect("fits");
                let rail = planned.rail;
                let first = planned.cells[0].rect;
                let last = planned.cells[planned.cells.len() - 1].rect;
                let ear = planned.metrics.ear();
                let (thickness, cell_thickness, lead, tail) = if edge.is_vertical() {
                    (
                        rail.w,
                        first.w,
                        first.y - rail.y,
                        rail.y + rail.h - last.y - last.h,
                    )
                } else {
                    (
                        rail.h,
                        first.h,
                        first.x - rail.x,
                        rail.x + rail.w - last.x - last.w,
                    )
                };
                let case = format!("{size:?} on the {edge:?} edge at {scale}x");
                assert_eq!(thickness, cell_thickness, "{case}");
                assert!(
                    (lead - ear).abs() < 1e-9,
                    "{case}: {lead} before the first cell"
                );
                assert!(
                    (tail - ear).abs() < 1e-9,
                    "{case}: {tail} after the last cell"
                );
            }
        }
    }
}
