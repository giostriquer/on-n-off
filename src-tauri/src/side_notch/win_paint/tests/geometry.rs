use super::*;

#[test]
fn the_painters_metrics_at_every_size_and_display_scale() {
    use NotchSize::{Compact, Large, Standard};
    let expected = [
        (
            1.0,
            Compact,
            [
                67.0, 64.0, 7.0, 35.0, 35.0, 1.0, 3.0, 40.0, 4.0, 3.0, 5.0, 21.0, 19.0,
            ],
        ),
        (
            1.0,
            Standard,
            [
                76.0, 73.0, 8.0, 40.0, 40.0, 1.0, 3.0, 46.0, 4.0, 3.0, 6.0, 24.0, 22.0,
            ],
        ),
        (
            1.0,
            Large,
            [
                86.0, 82.0, 9.0, 45.0, 45.0, 1.0, 3.0, 52.0, 5.0, 3.0, 7.0, 27.0, 25.0,
            ],
        ),
        (
            1.25,
            Compact,
            [
                66.4, 63.2, 7.2, 35.2, 35.2, 0.8, 2.4, 40.0, 3.2, 2.4, 5.6, 20.8, 19.2,
            ],
        ),
        (
            1.25,
            Standard,
            [
                76.0, 73.6, 8.0, 40.0, 40.0, 0.8, 3.2, 46.4, 4.0, 3.2, 6.4, 24.0, 22.4,
            ],
        ),
        (
            1.25,
            Large,
            [
                85.6, 81.6, 8.8, 44.8, 44.8, 0.8, 3.2, 52.0, 4.8, 3.2, 6.4, 27.2, 24.8,
            ],
        ),
        (
            1.5,
            Compact,
            [
                66.66666666666667,
                63.33333333333333,
                7.333333333333333,
                35.333333333333336,
                35.333333333333336,
                0.6666666666666666,
                2.6666666666666665,
                40.0,
                3.3333333333333335,
                2.6666666666666665,
                5.333333333333333,
                21.333333333333332,
                19.333333333333332,
            ],
        ),
        (
            1.5,
            Standard,
            [
                76.0,
                74.0,
                8.0,
                40.0,
                40.0,
                1.3333333333333333,
                3.3333333333333335,
                46.0,
                4.0,
                3.3333333333333335,
                6.0,
                24.0,
                22.0,
            ],
        ),
        (
            1.5,
            Large,
            [
                85.33333333333333,
                82.66666666666667,
                9.333333333333334,
                45.333333333333336,
                45.333333333333336,
                1.3333333333333333,
                3.3333333333333335,
                52.0,
                4.666666666666667,
                3.3333333333333335,
                6.666666666666667,
                27.333333333333332,
                24.666666666666668,
            ],
        ),
        (
            2.0,
            Compact,
            [
                66.5, 64.5, 7.0, 35.0, 35.0, 1.0, 2.5, 40.5, 3.5, 2.5, 5.5, 21.0, 19.5,
            ],
        ),
        (
            2.0,
            Standard,
            [
                76.0, 73.0, 8.0, 40.0, 40.0, 1.0, 3.0, 46.0, 4.0, 3.0, 6.0, 24.0, 22.0,
            ],
        ),
        (
            2.0,
            Large,
            [
                85.5, 82.5, 9.0, 45.0, 45.0, 1.0, 3.5, 52.0, 4.5, 3.5, 7.0, 27.0, 25.0,
            ],
        ),
    ];
    for (scale, size, values) in expected {
        let m = metrics(size, scale, Edge::Right);
        assert_eq!(
            [
                m.thickness,
                m.cell_length,
                m.cell_spacing,
                m.inset,
                m.ear,
                m.cell_padding,
                m.content_spacing,
                m.icon_slot,
                m.ring_stroke,
                m.inner_ring_stroke,
                m.inner_ring_inset,
                m.glyph,
                m.label_height,
            ],
            values,
            "{size:?} at {scale}x"
        );
        let across = metrics(size, scale, Edge::Top);
        assert_eq!(
            (across.thickness, across.cell_length),
            (m.cell_length, m.thickness)
        );
    }
}

#[test]
fn the_painters_pill_at_every_size() {
    let rail = R::new(1000.0, 200.0, 90.0, 600.0);
    for (size, thickness, length) in [
        (NotchSize::Compact, 5.0, 105.0),
        (NotchSize::Standard, 6.0, 120.0),
        (NotchSize::Large, 7.0, 135.0),
    ] {
        let current = NotchSettings { size, ..settings() };
        assert_eq!(
            pill_frame(&current, &rail),
            R::new(1090.0 - thickness, 500.0 - length / 2.0, thickness, length),
            "{size:?}"
        );
    }
}

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
                let cells = (0..current.cell_count())
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
                let ear = planned.metrics.ear;
                let (thickness, cell_thickness, lead, tail) = if vertical(edge) {
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
