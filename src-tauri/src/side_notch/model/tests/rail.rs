use super::*;

#[test]
fn the_rail_frame_at_every_size_edge_and_display_scale() {
    use Edge::{Bottom, Left, Right, Top};
    use NotchSize::{Compact, Large, Standard};
    let frames = [
        (1.0, Compact, Right, [1662.0, 366.0, 66.5, 417.375]),
        (1.0, Compact, Left, [0.0, 366.0, 66.5, 417.375]),
        (1.0, Compact, Top, [649.0, 33.0, 430.5, 63.875]),
        (1.0, Compact, Bottom, [649.0, 1053.0, 430.5, 63.875]),
        (1.0, Standard, Right, [1652.0, 337.0, 76.0, 477.0]),
        (1.0, Standard, Left, [0.0, 337.0, 76.0, 477.0]),
        (1.0, Standard, Top, [618.0, 33.0, 492.0, 73.0]),
        (1.0, Standard, Bottom, [618.0, 1044.0, 492.0, 73.0]),
        (1.0, Large, Right, [1643.0, 307.0, 85.5, 536.625]),
        (1.0, Large, Left, [0.0, 307.0, 85.5, 536.625]),
        (1.0, Large, Top, [587.0, 33.0, 553.5, 82.125]),
        (1.0, Large, Bottom, [587.0, 1035.0, 553.5, 82.125]),
        (1.25, Compact, Right, [1661.6, 366.4, 66.5, 417.375]),
        (1.25, Compact, Left, [0.0, 366.4, 66.5, 417.375]),
        (1.25, Compact, Top, [648.8, 32.8, 430.5, 63.875]),
        (1.25, Compact, Bottom, [648.8, 1052.8, 430.5, 63.875]),
        (1.25, Standard, Right, [1652.0, 336.8, 76.0, 477.0]),
        (1.25, Standard, Left, [0.0, 336.8, 76.0, 477.0]),
        (1.25, Standard, Top, [618.4, 32.8, 492.0, 73.0]),
        (1.25, Standard, Bottom, [618.4, 1044.0, 492.0, 73.0]),
        (1.25, Large, Right, [1642.4, 306.4, 85.5, 536.625]),
        (1.25, Large, Left, [0.0, 306.4, 85.5, 536.625]),
        (1.25, Large, Top, [587.2, 32.8, 553.5, 82.125]),
        (1.25, Large, Bottom, [587.2, 1035.2, 553.5, 82.125]),
        (
            1.5,
            Compact,
            Right,
            [1661.3333333333333, 366.0, 66.5, 417.375],
        ),
        (1.5, Compact, Left, [0.0, 366.0, 66.5, 417.375]),
        (
            1.5,
            Compact,
            Top,
            [648.6666666666666, 33.333333333333336, 430.5, 63.875],
        ),
        (
            1.5,
            Compact,
            Bottom,
            [648.6666666666666, 1053.3333333333333, 430.5, 63.875],
        ),
        (
            1.5,
            Standard,
            Right,
            [1652.0, 336.6666666666667, 76.0, 477.0],
        ),
        (1.5, Standard, Left, [0.0, 336.6666666666667, 76.0, 477.0]),
        (1.5, Standard, Top, [618.0, 33.333333333333336, 492.0, 73.0]),
        (1.5, Standard, Bottom, [618.0, 1044.0, 492.0, 73.0]),
        (
            1.5,
            Large,
            Right,
            [1642.6666666666667, 306.6666666666667, 85.5, 536.625],
        ),
        (1.5, Large, Left, [0.0, 306.6666666666667, 85.5, 536.625]),
        (
            1.5,
            Large,
            Top,
            [587.3333333333334, 33.333333333333336, 553.5, 82.125],
        ),
        (
            1.5,
            Large,
            Bottom,
            [587.3333333333334, 1034.6666666666667, 553.5, 82.125],
        ),
        (2.0, Compact, Right, [1661.5, 366.5, 66.5, 417.375]),
        (2.0, Compact, Left, [0.0, 366.5, 66.5, 417.375]),
        (2.0, Compact, Top, [649.0, 33.0, 430.5, 63.875]),
        (2.0, Compact, Bottom, [649.0, 1053.0, 430.5, 63.875]),
        (2.0, Standard, Right, [1652.0, 336.5, 76.0, 477.0]),
        (2.0, Standard, Left, [0.0, 336.5, 76.0, 477.0]),
        (2.0, Standard, Top, [618.0, 33.0, 492.0, 73.0]),
        (2.0, Standard, Bottom, [618.0, 1044.0, 492.0, 73.0]),
        (2.0, Large, Right, [1642.5, 306.5, 85.5, 536.625]),
        (2.0, Large, Left, [0.0, 306.5, 85.5, 536.625]),
        (2.0, Large, Top, [587.5, 33.0, 553.5, 82.125]),
        (2.0, Large, Bottom, [587.5, 1035.0, 553.5, 82.125]),
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
        (1.0, 417.5, true),
        (1.0, 418.0, true),
        (2.0, 417.0, false),
        (2.0, 417.5, true),
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
