use super::*;

#[test]
fn the_rail_frame_at_every_size_edge_and_display_scale() {
    use Edge::{Bottom, Left, Right, Top};
    use NotchSize::{Compact, Large, Standard};
    let frames = [
        (1.0, Compact, Right, [1661.0, 366.0, 67.0, 418.0]),
        (1.0, Compact, Left, [0.0, 366.0, 67.0, 418.0]),
        (1.0, Compact, Top, [648.0, 33.0, 433.0, 64.0]),
        (1.0, Compact, Bottom, [648.0, 1053.0, 433.0, 64.0]),
        (1.0, Standard, Right, [1652.0, 337.0, 76.0, 477.0]),
        (1.0, Standard, Left, [0.0, 337.0, 76.0, 477.0]),
        (1.0, Standard, Top, [618.0, 33.0, 492.0, 73.0]),
        (1.0, Standard, Bottom, [618.0, 1044.0, 492.0, 73.0]),
        (1.0, Large, Right, [1642.0, 307.0, 86.0, 536.0]),
        (1.0, Large, Left, [0.0, 307.0, 86.0, 536.0]),
        (1.0, Large, Top, [586.0, 33.0, 556.0, 82.0]),
        (1.0, Large, Bottom, [586.0, 1035.0, 556.0, 82.0]),
        (
            1.25,
            Compact,
            Right,
            [1661.6, 367.2, 66.4, 415.20000000000005],
        ),
        (1.25, Compact, Left, [0.0, 367.2, 66.4, 415.20000000000005]),
        (1.25, Compact, Top, [648.8, 32.8, 431.20000000000005, 63.2]),
        (
            1.25,
            Compact,
            Bottom,
            [648.8, 1053.6, 431.20000000000005, 63.2],
        ),
        (1.25, Standard, Right, [1652.0, 335.2, 76.0, 480.0]),
        (1.25, Standard, Left, [0.0, 335.2, 76.0, 480.0]),
        (1.25, Standard, Top, [618.4, 32.8, 492.0, 73.6]),
        (1.25, Standard, Bottom, [618.4, 1043.2, 492.0, 73.6]),
        (1.25, Large, Right, [1642.4, 308.8, 85.6, 532.8]),
        (1.25, Large, Left, [0.0, 308.8, 85.6, 532.8]),
        (1.25, Large, Top, [588.0, 32.8, 552.8, 81.6]),
        (1.25, Large, Bottom, [588.0, 1035.2, 552.8, 81.6]),
        (
            1.5,
            Compact,
            Right,
            [
                1661.3333333333333,
                366.6666666666667,
                66.66666666666667,
                416.66666666666663,
            ],
        ),
        (
            1.5,
            Compact,
            Left,
            [
                0.0,
                366.6666666666667,
                66.66666666666667,
                416.66666666666663,
            ],
        ),
        (
            1.5,
            Compact,
            Top,
            [
                647.3333333333334,
                33.333333333333336,
                433.33333333333337,
                63.33333333333333,
            ],
        ),
        (
            1.5,
            Compact,
            Bottom,
            [
                647.3333333333334,
                1054.0,
                433.33333333333337,
                63.33333333333333,
            ],
        ),
        (1.5, Standard, Right, [1652.0, 334.0, 76.0, 482.0]),
        (1.5, Standard, Left, [0.0, 334.0, 76.0, 482.0]),
        (1.5, Standard, Top, [618.0, 33.333333333333336, 492.0, 74.0]),
        (
            1.5,
            Standard,
            Bottom,
            [618.0, 1043.3333333333333, 492.0, 74.0],
        ),
        (
            1.5,
            Large,
            Right,
            [
                1642.6666666666667,
                304.6666666666667,
                85.33333333333333,
                541.3333333333334,
            ],
        ),
        (
            1.5,
            Large,
            Left,
            [0.0, 304.6666666666667, 85.33333333333333, 541.3333333333334],
        ),
        (
            1.5,
            Large,
            Top,
            [
                586.6666666666666,
                33.333333333333336,
                554.6666666666666,
                82.66666666666667,
            ],
        ),
        (
            1.5,
            Large,
            Bottom,
            [
                586.6666666666666,
                1034.6666666666667,
                554.6666666666666,
                82.66666666666667,
            ],
        ),
        (2.0, Compact, Right, [1661.5, 365.0, 66.5, 420.5]),
        (2.0, Compact, Left, [0.0, 365.0, 66.5, 420.5]),
        (2.0, Compact, Top, [649.0, 33.0, 430.5, 64.5]),
        (2.0, Compact, Bottom, [649.0, 1052.5, 430.5, 64.5]),
        (2.0, Standard, Right, [1652.0, 336.5, 76.0, 477.0]),
        (2.0, Standard, Left, [0.0, 336.5, 76.0, 477.0]),
        (2.0, Standard, Top, [618.0, 33.0, 492.0, 73.0]),
        (2.0, Standard, Bottom, [618.0, 1044.0, 492.0, 73.0]),
        (2.0, Large, Right, [1642.5, 306.0, 85.5, 538.5]),
        (2.0, Large, Left, [0.0, 306.0, 85.5, 538.5]),
        (2.0, Large, Top, [587.5, 33.0, 553.5, 82.5]),
        (2.0, Large, Bottom, [587.5, 1034.5, 553.5, 82.5]),
    ];
    for (scale, size, edge, [x, y, width, height]) in frames {
        let mut current = settings("main", edge);
        current.size = size;
        assert_eq!(
            layout(&current, &[display("main", 0.0, scale)]),
            Some(Layout {
                x,
                y,
                width,
                height
            }),
            "{size:?} on the {edge:?} edge at {scale}x"
        );
    }
}

#[test]
fn a_compact_rail_needs_room_for_its_length() {
    let mut compact = settings("main", Edge::Right);
    compact.size = NotchSize::Compact;
    for (scale, work_height, fits) in [
        (1.0, 417.0, false),
        (1.0, 417.5, false),
        (1.0, 418.0, true),
        (2.0, 417.0, false),
        (2.0, 417.5, false),
        (2.0, 420.5, true),
    ] {
        let mut short = display("main", 0.0, scale);
        short.work_height = work_height;
        assert_eq!(
            layout(&compact, &[short]).is_some(),
            fits,
            "a {work_height} pt work area at {scale}x"
        );
    }
}

#[test]
fn the_rail_metrics_at_every_size_and_display_scale() {
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
        let m = rail_metrics(size, scale, Edge::Right);
        assert_eq!(
            [
                m.thickness,
                m.cell_length,
                m.cell_spacing,
                m.inset,
                m.ear(),
                m.cell.padding,
                m.cell.content_spacing,
                m.cell.icon_slot,
                m.cell.ring_stroke,
                m.cell.inner_ring_stroke,
                m.cell.inner_ring_inset,
                m.cell.glyph,
                m.cell.label_height,
            ],
            values,
            "{size:?} at {scale}x"
        );
        let across = rail_metrics(size, scale, Edge::Top);
        assert_eq!(
            (across.thickness, across.cell_length),
            (m.cell_length, m.thickness)
        );
    }
}

#[test]
fn the_pill_at_every_size_and_display_scale() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for (size, thickness, length) in [
            (NotchSize::Compact, 5.0, 105.0),
            (NotchSize::Standard, 6.0, 120.0),
            (NotchSize::Large, 7.0, 135.0),
        ] {
            let mut current = settings("main", Edge::Right);
            current.size = size;
            let displays = [display("main", 0.0, scale)];
            let rail = layout(&current, &displays).expect("fits");
            assert_eq!(
                pill_frame(&current, &displays),
                Some(Layout {
                    x: rail.x + rail.width - thickness,
                    y: rail.y + rail.height / 2.0 - length / 2.0,
                    width: thickness,
                    height: length,
                }),
                "{size:?} at {scale}x"
            );
        }
    }
}

#[test]
fn rail_cells_stack_along_the_axis_after_the_first_ear() {
    let metric_set = rail_metrics(NotchSize::Standard, 1.0, Edge::Right);
    let frames = rail_cell_frames(Edge::Right, &metric_set, 3);
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].y, metric_set.inset);
    assert_eq!(
        frames[1].y,
        metric_set.inset + metric_set.cell_length + metric_set.cell_spacing
    );
    assert_eq!(
        frames[2].y,
        metric_set.inset + 2.0 * (metric_set.cell_length + metric_set.cell_spacing)
    );
    assert_eq!(frames[2].x, 0.0);
    assert_eq!(frames[0].width, metric_set.thickness);

    let horizontal = rail_cell_frames(Edge::Top, &metric_set, 2);
    assert_eq!(
        horizontal[0].x, metric_set.inset,
        "cells run along x for horizontal edges"
    );
    assert_eq!(
        horizontal[1].x,
        metric_set.inset + metric_set.cell_length + metric_set.cell_spacing
    );
    assert_eq!(horizontal[0].y, 0.0);
}
