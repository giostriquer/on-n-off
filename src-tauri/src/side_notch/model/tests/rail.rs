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
