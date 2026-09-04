//! Provider marks as vector paths, ported from
//! `src-tauri/macos/SideNotch/Sources/NotchApp/ProviderMark.swift` (converted offline from
//! their SVG sources). Every mark fits into a rect the same way the SwiftUI shape does:
//! uniform scale, centred, `xMidYMid meet`.

// The path constants are transcribed from the Swift source; their full f64 precision is
// intentional, so the precision lint is off for this module.
#![allow(clippy::excessive_precision)]

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

type Point = (f32, f32);

/// One path segment after the previous point (or the subpath's start).
enum Seg {
    Line(Point),
    Curve { c1: Point, c2: Point, to: Point },
}

struct Sub {
    start: Point,
    segs: &'static [Seg],
}

struct Shape {
    even_odd: bool,
    subs: &'static [Sub],
}

fn append(pb: &mut PathBuilder, sub: &Sub) {
    pb.move_to(sub.start.0, sub.start.1);
    for seg in sub.segs {
        match seg {
            Seg::Line((x, y)) => pb.line_to(*x, *y),
            Seg::Curve {
                c1: (c1x, c1y),
                c2: (c2x, c2y),
                to: (x, y),
            } => {
                pb.cubic_to(*c1x, *c1y, *c2x, *c2y, *x, *y);
            }
        }
    }
    pb.close();
}

/// `path` scaled uniformly into `rect` and centred, like an SVG with `xMidYMid meet`.
fn fitted(shape: &Shape, box_: (f32, f32, f32, f32), rect: (f32, f32, f32, f32)) -> Option<Path> {
    let (bx, by, bw, bh) = box_;
    let (rx, ry, rw, rh) = rect;
    let scale = (rw / bw).min(rh / bh);
    let offset_x = rx + (rw - bw * scale) / 2.0 - bx * scale;
    let offset_y = ry + (rh - bh * scale) / 2.0 - by * scale;
    let transform = Transform::from_row(scale, 0.0, 0.0, scale, offset_x, offset_y);
    let mut pb = PathBuilder::new();
    for sub in shape.subs {
        append(&mut pb, sub);
    }
    pb.finish().and_then(|path| path.transform(transform))
}

// MARK: Claude (Simple Icons, 24 x 24) — one continuous starburst.

const CLAUDE: Shape = Shape {
    even_odd: true,
    subs: &[Sub {
        start: (4.7144, 15.9555),
        segs: &[
            Seg::Line((9.4318, 13.3084)),
            Seg::Line((9.5108, 13.0777)),
            Seg::Line((9.4318, 12.9502)),
            Seg::Line((9.2011, 12.9502)),
            Seg::Line((8.4118, 12.9016)),
            Seg::Line((5.7162, 12.8287)),
            Seg::Line((3.3787, 12.7316)),
            Seg::Line((1.1141, 12.6102)),
            Seg::Line((0.5434, 12.4887)),
            Seg::Line((0.0091, 11.7845)),
            Seg::Line((0.0637, 11.4323)),
            Seg::Line((0.5434, 11.1105)),
            Seg::Line((1.2294, 11.1713)),
            Seg::Line((2.7473, 11.2745)),
            Seg::Line((5.024, 11.4323)),
            Seg::Line((6.6754, 11.5295)),
            Seg::Line((9.1222, 11.7845)),
            Seg::Line((9.5108, 11.7845)),
            Seg::Line((9.5654, 11.6266)),
            Seg::Line((9.4318, 11.5295)),
            Seg::Line((9.3286, 11.4323)),
            Seg::Line((6.973, 9.8356)),
            Seg::Line((4.423, 8.1477)),
            Seg::Line((3.0874, 7.1763)),
            Seg::Line((2.3649, 6.6845)),
            Seg::Line((2.0006, 6.2231)),
            Seg::Line((1.8428, 5.2153)),
            Seg::Line((2.4985, 4.4928)),
            Seg::Line((3.3788, 4.5535)),
            Seg::Line((3.6034, 4.6142)),
            Seg::Line((4.4959, 5.3002)),
            Seg::Line((6.4023, 6.7756)),
            Seg::Line((8.8916, 8.6092)),
            Seg::Line((9.2559, 8.9127)),
            Seg::Line((9.4016, 8.8095)),
            Seg::Line((9.4198, 8.7367)),
            Seg::Line((9.2558, 8.4634)),
            Seg::Line((7.9019, 6.0167)),
            Seg::Line((6.4569, 3.5274)),
            Seg::Line((5.8134, 2.4954)),
            Seg::Line((5.6434, 1.876)),
            Seg::Curve {
                c1: (5.5827, 1.621),
                c2: (5.5402, 1.4086),
                to: (5.5402, 1.1475),
            },
            Seg::Line((6.287, 0.1335)),
            Seg::Line((6.6997, 0.0)),
            Seg::Line((7.6954, 0.1336)),
            Seg::Line((8.1144, 0.4978)),
            Seg::Line((8.7336, 1.9125)),
            Seg::Line((9.7354, 4.1407)),
            Seg::Line((11.2897, 7.1703)),
            Seg::Line((11.745, 8.0688)),
            Seg::Line((11.9879, 8.9006)),
            Seg::Line((12.0789, 9.1556)),
            Seg::Line((12.2368, 9.1556)),
            Seg::Line((12.2368, 9.0099)),
            Seg::Line((12.3643, 7.3039)),
            Seg::Line((12.6011, 5.2092)),
            Seg::Line((12.8318, 2.5135)),
            Seg::Line((12.9107, 1.7546)),
            Seg::Line((13.2871, 0.8439)),
            Seg::Line((14.0339, 0.3521)),
            Seg::Line((14.6167, 0.6314)),
            Seg::Line((15.0964, 1.3174)),
            Seg::Line((15.0296, 1.7607)),
            Seg::Line((14.7443, 3.6124)),
            Seg::Line((14.1857, 6.5145)),
            Seg::Line((13.8214, 8.4574)),
            Seg::Line((14.0339, 8.4574)),
            Seg::Line((14.2768, 8.2145)),
            Seg::Line((15.2603, 6.9092)),
            Seg::Line((16.9117, 4.8449)),
            Seg::Line((17.6403, 4.0253)),
            Seg::Line((18.4903, 3.1207)),
            Seg::Line((19.0367, 2.6896)),
            Seg::Line((20.0688, 2.6896)),
            Seg::Line((20.8278, 3.8189)),
            Seg::Line((20.4878, 4.9846)),
            Seg::Line((19.4253, 6.3324)),
            Seg::Line((18.5449, 7.4738)),
            Seg::Line((17.2821, 9.1738)),
            Seg::Line((16.4928, 10.5338)),
            Seg::Line((16.5657, 10.6431)),
            Seg::Line((16.7539, 10.6248)),
            Seg::Line((19.6074, 10.0178)),
            Seg::Line((21.1495, 9.7384)),
            Seg::Line((22.9891, 9.4227)),
            Seg::Line((23.8209, 9.8113)),
            Seg::Line((23.9119, 10.2059)),
            Seg::Line((23.5841, 11.0134)),
            Seg::Line((21.6171, 11.4991)),
            Seg::Line((19.3099, 11.9605)),
            Seg::Line((15.8735, 12.7741)),
            Seg::Line((15.831, 12.8045)),
            Seg::Line((15.8796, 12.8652)),
            Seg::Line((17.4278, 13.0109)),
            Seg::Line((18.0896, 13.0473)),
            Seg::Line((19.7106, 13.0473)),
            Seg::Line((22.7281, 13.272)),
            Seg::Line((23.5173, 13.794)),
            Seg::Line((23.9909, 14.4316)),
            Seg::Line((23.9119, 14.9173)),
            Seg::Line((22.6977, 15.5366)),
            Seg::Line((21.0584, 15.148)),
            Seg::Line((17.2334, 14.2373)),
            Seg::Line((15.9221, 13.9094)),
            Seg::Line((15.7399, 13.9094)),
            Seg::Line((15.7399, 14.0187)),
            Seg::Line((16.8328, 15.0873)),
            Seg::Line((18.8363, 16.8965)),
            Seg::Line((21.3438, 19.2279)),
            Seg::Line((21.4713, 19.8047)),
            Seg::Line((21.1495, 20.2601)),
            Seg::Line((20.8095, 20.2115)),
            Seg::Line((18.6056, 18.554)),
            Seg::Line((17.7556, 17.8072)),
            Seg::Line((15.831, 16.1862)),
            Seg::Line((15.7035, 16.1862)),
            Seg::Line((15.7035, 16.3562)),
            Seg::Line((16.1467, 17.0058)),
            Seg::Line((18.4903, 20.5272)),
            Seg::Line((18.6117, 21.6079)),
            Seg::Line((18.4417, 21.96)),
            Seg::Line((17.8346, 22.1725)),
            Seg::Line((17.1667, 22.0511)),
            Seg::Line((15.7946, 20.1265)),
            Seg::Line((14.38, 17.959)),
            Seg::Line((13.2386, 16.0162)),
            Seg::Line((13.0989, 16.0952)),
            Seg::Line((12.4249, 23.3504)),
            Seg::Line((12.1093, 23.7207)),
            Seg::Line((11.3807, 24.0)),
            Seg::Line((10.7736, 23.5386)),
            Seg::Line((10.4518, 22.7918)),
            Seg::Line((10.7736, 21.3165)),
            Seg::Line((11.1622, 19.3919)),
            Seg::Line((11.4779, 17.8619)),
            Seg::Line((11.7632, 15.9615)),
            Seg::Line((11.9332, 15.3301)),
            Seg::Line((11.9211, 15.2876)),
            Seg::Line((11.7814, 15.3058)),
            Seg::Line((10.3486, 17.273)),
            Seg::Line((8.169, 20.2176)),
            Seg::Line((6.4447, 22.0632)),
            Seg::Line((6.0319, 22.2272)),
            Seg::Line((5.3155, 21.8568)),
            Seg::Line((5.3822, 21.195)),
            Seg::Line((5.783, 20.6061)),
            Seg::Line((8.169, 17.5704)),
            Seg::Line((9.6079, 15.6884)),
            Seg::Line((10.5369, 14.6016)),
            Seg::Line((10.5307, 14.4437)),
            Seg::Line((10.4761, 14.4437)),
            Seg::Line((4.1376, 18.5601)),
            Seg::Line((3.0083, 18.7058)),
            Seg::Line((2.5226, 18.2504)),
            Seg::Line((2.5834, 17.5037)),
            Seg::Line((2.8141, 17.2608)),
            Seg::Line((4.7205, 15.9494)),
            Seg::Line((4.7144, 15.9555)),
        ],
    }],
};

// MARK: Codex (Simple Icons, 24 x 24) — the knot plus its six inner windows.

const CODEX: Shape = Shape {
    even_odd: true,
    subs: &[
        Sub {
            start: (22.2819, 9.8211),
            segs: &[
                Seg::Curve {
                    c1: (22.824776, 8.186235),
                    c2: (22.636854, 6.396725),
                    to: (21.7662, 4.9103),
                },
                Seg::Curve {
                    c1: (20.457089, 2.631633),
                    c2: (17.825979, 1.459521),
                    to: (15.2564, 2.0103),
                },
                Seg::Curve {
                    c1: (13.808329, 0.399528),
                    c2: (11.611165, -0.316756),
                    to: (9.491981, 0.131078),
                },
                Seg::Curve {
                    c1: (7.372797, 0.578912),
                    c2: (5.653279, 2.122884),
                    to: (4.9807, 4.1818),
                },
                Seg::Curve {
                    c1: (3.292803, 4.527919),
                    c2: (1.835975, 5.584727),
                    to: (0.983, 7.0818),
                },
                Seg::Curve {
                    c1: (-0.340434, 9.356841),
                    c2: (-0.040091, 12.226664),
                    to: (1.7257, 14.1784),
                },
                Seg::Curve {
                    c1: (1.180815, 15.812499),
                    c2: (1.367049, 17.602196),
                    to: (2.2367, 19.0891),
                },
                Seg::Curve {
                    c1: (3.547453, 21.368581),
                    c2: (6.180305, 22.540646),
                    to: (8.7513, 21.9892),
                },
                Seg::Curve {
                    c1: (9.894838, 23.276963),
                    c2: (11.537716, 24.009673),
                    to: (13.2599, 24.0),
                },
                Seg::Curve {
                    c1: (15.893738, 24.002424),
                    c2: (18.227114, 22.302138),
                    to: (19.0317, 19.7942),
                },
                Seg::Curve {
                    c1: (20.719362, 19.447484),
                    c2: (22.17598, 18.390793),
                    to: (23.0294, 16.8941),
                },
                Seg::Curve {
                    c1: (24.336803, 14.623065),
                    c2: (24.035146, 11.76877),
                    to: (22.2819, 9.8212),
                },
                Seg::Line((22.2819, 9.8211)),
            ],
        },
        Sub {
            start: (13.2599, 22.4292),
            segs: &[
                Seg::Curve {
                    c1: (12.208618, 22.430864),
                    c2: (11.190301, 22.062395),
                    to: (10.3835, 21.3884),
                },
                Seg::Line((10.5254, 21.308)),
                Seg::Line((15.3037, 18.5498)),
                Seg::Curve {
                    c1: (15.545646, 18.407902),
                    c2: (15.694886, 18.148983),
                    to: (15.6964, 17.8685),
                },
                Seg::Line((15.6964, 11.1316)),
                Seg::Line((17.7164, 12.3002)),
                Seg::Curve {
                    c1: (17.736652, 12.310461),
                    c2: (17.750776, 12.329788),
                    to: (17.7544, 12.3522),
                },
                Seg::Line((17.7544, 17.9348)),
                Seg::Curve {
                    c1: (17.749119, 20.414837),
                    c2: (15.739937, 22.423975),
                    to: (13.2599, 22.4292),
                },
                Seg::Line((13.2599, 22.4292)),
            ],
        },
        Sub {
            start: (3.5992, 18.3038),
            segs: &[
                Seg::Curve {
                    c1: (3.071972, 17.39342),
                    c2: (2.882672, 16.326277),
                    to: (3.0646, 15.2901),
                },
                Seg::Line((3.2066, 15.3753)),
                Seg::Line((7.9896, 18.1335)),
                Seg::Curve {
                    c1: (8.230587, 18.274909),
                    c2: (8.529213, 18.274909),
                    to: (8.7702, 18.1335),
                },
                Seg::Line((14.613, 14.765)),
                Seg::Line((14.613, 17.0974)),
                Seg::Curve {
                    c1: (14.611888, 17.121889),
                    c2: (14.599664, 17.144534),
                    to: (14.5798, 17.1589),
                },
                Seg::Line((9.74, 19.9502)),
                Seg::Curve {
                    c1: (7.589341, 21.189138),
                    c2: (4.841618, 20.45245),
                    to: (3.5992, 18.3038),
                },
                Seg::Line((3.5992, 18.3038)),
            ],
        },
        Sub {
            start: (2.3408, 7.8956),
            segs: &[
                Seg::Curve {
                    c1: (2.871683, 6.979369),
                    c2: (3.709632, 6.280529),
                    to: (4.7063, 5.9228),
                },
                Seg::Line((4.7063, 11.6)),
                Seg::Curve {
                    c1: (4.702637, 11.879344),
                    c2: (4.851265, 12.138553),
                    to: (5.0942, 12.2765),
                },
                Seg::Line((10.9086, 15.6308)),
                Seg::Line((8.8885, 16.7993)),
                Seg::Curve {
                    c1: (8.866301, 16.811087),
                    c2: (8.839699, 16.811087),
                    to: (8.8175, 16.7993),
                },
                Seg::Line((3.9872, 14.0128)),
                Seg::Curve {
                    c1: (1.840816, 12.768645),
                    c2: (1.104693, 10.023029),
                    to: (2.3408, 7.872),
                },
                Seg::Line((2.3408, 7.8956)),
            ],
        },
        Sub {
            start: (18.9371, 11.7514),
            segs: &[
                Seg::Line((13.1038, 8.364)),
                Seg::Line((15.1192, 7.2)),
                Seg::Curve {
                    c1: (15.141399, 7.188213),
                    c2: (15.168001, 7.188213),
                    to: (15.1902, 7.2),
                },
                Seg::Line((20.0205, 9.9913)),
                Seg::Curve {
                    c1: (21.528124, 10.861219),
                    c2: (22.397899, 12.523435),
                    to: (22.253106, 14.258003),
                },
                Seg::Curve {
                    c1: (22.108312, 15.99257),
                    c2: (20.974987, 17.487577),
                    to: (19.344, 18.0955),
                },
                Seg::Line((19.344, 12.4183)),
                Seg::Curve {
                    c1: (19.335479, 12.139742),
                    c2: (19.180819, 11.886281),
                    to: (18.937, 11.7513),
                },
                Seg::Line((18.9371, 11.7514)),
            ],
        },
        Sub {
            start: (20.9478, 8.7283),
            segs: &[
                Seg::Line((20.8058, 8.6431)),
                Seg::Line((16.0323, 5.8613)),
                Seg::Curve {
                    c1: (15.789833, 5.719012),
                    c2: (15.489367, 5.719012),
                    to: (15.2469, 5.8613),
                },
                Seg::Line((9.409, 9.2297)),
                Seg::Line((9.409, 6.8974)),
                Seg::Curve {
                    c1: (9.406467, 6.873242),
                    c2: (9.417367, 6.849637),
                    to: (9.4374, 6.8359),
                },
                Seg::Line((14.2677, 4.0493)),
                Seg::Curve {
                    c1: (15.778978, 3.178674),
                    c2: (17.657277, 3.259917),
                    to: (19.087737, 4.257782),
                },
                Seg::Curve {
                    c1: (20.518196, 5.255648),
                    c2: (21.243075, 6.990341),
                    to: (20.9479, 8.7093),
                },
                Seg::Line((20.9478, 8.7283)),
            ],
        },
        Sub {
            start: (8.3065, 12.863),
            segs: &[
                Seg::Line((6.2865, 11.6992)),
                Seg::Curve {
                    c1: (6.266041, 11.686882),
                    c2: (6.252117, 11.666106),
                    to: (6.2485, 11.6425),
                },
                Seg::Line((6.2485, 6.0742)),
                Seg::Curve {
                    c1: (6.25077, 4.330388),
                    c2: (7.260488, 2.74493),
                    to: (8.839741, 2.005438),
                },
                Seg::Curve {
                    c1: (10.418993, 1.265947),
                    c2: (12.283335, 1.505616),
                    to: (13.6242, 2.6205),
                },
                Seg::Line((13.4822, 2.701)),
                Seg::Line((8.704, 5.459)),
                Seg::Curve {
                    c1: (8.462054, 5.600898),
                    c2: (8.312814, 5.859817),
                    to: (8.3113, 6.1403),
                },
                Seg::Line((8.3065, 12.863)),
            ],
        },
        Sub {
            start: (9.4041, 10.4976),
            segs: &[
                Seg::Line((12.0061, 8.9978)),
                Seg::Line((14.613, 10.4976)),
                Seg::Line((14.613, 13.497)),
                Seg::Line((12.0156, 14.9967)),
                Seg::Line((9.4089, 13.497)),
                Seg::Line((9.4041, 10.4976)),
            ],
        },
    ],
};

// MARK: Cursor (official 2D cube, 466.73 x 532.09)

const CURSOR: Shape = Shape {
    even_odd: true,
    subs: &[
        Sub {
            start: (457.43, 125.94),
            segs: &[
                Seg::Line((244.42, 2.96)),
                Seg::Curve {
                    c1: (237.58, -0.99),
                    c2: (229.14, -0.99),
                    to: (222.3, 2.96),
                },
                Seg::Line((9.3, 125.94)),
                Seg::Curve {
                    c1: (3.55, 129.26),
                    c2: (0.0, 135.4),
                    to: (0.0, 142.05),
                },
                Seg::Line((0.0, 390.04)),
                Seg::Curve {
                    c1: (0.0, 396.69),
                    c2: (3.55, 402.83),
                    to: (9.3, 406.15),
                },
                Seg::Line((222.31, 529.13)),
                Seg::Curve {
                    c1: (229.15, 533.08),
                    c2: (237.59, 533.08),
                    to: (244.43, 529.13),
                },
                Seg::Line((457.44, 406.15)),
                Seg::Curve {
                    c1: (463.19, 402.83),
                    c2: (466.74, 396.69),
                    to: (466.74, 390.04),
                },
                Seg::Line((466.74, 142.05)),
                Seg::Curve {
                    c1: (466.74, 135.4),
                    c2: (463.19, 129.26),
                    to: (457.43, 125.94),
                },
                Seg::Line((457.43, 125.94)),
            ],
        },
        Sub {
            start: (444.05, 151.99),
            segs: &[
                Seg::Line((238.42, 508.15)),
                Seg::Curve {
                    c1: (237.03, 510.55),
                    c2: (233.36, 509.57),
                    to: (233.36, 506.79),
                },
                Seg::Line((233.36, 273.58)),
                Seg::Curve {
                    c1: (233.36, 268.92),
                    c2: (230.87, 264.61),
                    to: (24.87, 145.67),
                },
                Seg::Line((26.23, 140.61)),
                Seg::Curve {
                    c1: (22.47, 144.28),
                    c2: (23.45, 140.61),
                    to: (437.49, 140.61),
                },
                Seg::Line((444.06, 152.0)),
                Seg::Curve {
                    c1: (443.33, 140.61),
                    c2: (446.98, 146.94),
                    to: (444.05, 152.0),
                },
                Seg::Line((444.05, 152.0)),
            ],
        },
    ],
};

// MARK: Antigravity (arch silhouette, viewBox 13 14.5 85 85)

const ANTIGRAVITY: Shape = Shape {
    even_odd: true,
    subs: &[Sub {
        start: (89.6992, 93.695),
        segs: &[
            Seg::Curve {
                c1: (94.3659, 97.195),
                c2: (101.366, 94.8617),
                to: (94.9492, 88.445),
            },
            Seg::Curve {
                c1: (75.6992, 69.7783),
                c2: (79.7825, 18.445),
                to: (55.8659, 18.445),
            },
            Seg::Curve {
                c1: (31.9492, 18.445),
                c2: (36.0325, 69.7783),
                to: (16.7825, 88.445),
            },
            Seg::Curve {
                c1: (9.7825, 95.445),
                c2: (17.3658, 97.195),
                to: (22.0325, 93.695),
            },
            Seg::Curve {
                c1: (40.1159, 81.445),
                c2: (38.9492, 59.8617),
                to: (55.8659, 59.8617),
            },
            Seg::Curve {
                c1: (72.7825, 59.8617),
                c2: (71.6159, 81.445),
                to: (89.6992, 93.695),
            },
        ],
    }],
};

fn fill(
    shape: &Shape,
    box_: (f32, f32, f32, f32),
    rect: (f32, f32, f32, f32),
    color: [u8; 4],
    pixmap: &mut Pixmap,
) {
    let Some(path) = fitted(shape, box_, rect) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        if shape.even_odd {
            FillRule::EvenOdd
        } else {
            FillRule::Winding
        },
        Transform::identity(),
        None,
    );
}

/// The provider glyph centred in `rect` (x, y, w, h device pixels), white like the rail.
pub(super) fn provider(
    provider: crate::dto::AgentId,
    rect: (f32, f32, f32, f32),
    color: [u8; 4],
    pixmap: &mut Pixmap,
) {
    use crate::dto::AgentId as Id;
    match provider {
        Id::Claude => fill(&CLAUDE, (0.0, 0.0, 24.0, 24.0), rect, color, pixmap),
        Id::Codex => fill(&CODEX, (0.0, 0.0, 24.0, 24.0), rect, color, pixmap),
        Id::Cursor => fill(&CURSOR, (0.0, 0.0, 466.73, 532.09), rect, color, pixmap),
        Id::Antigravity => fill(&ANTIGRAVITY, (13.0, 14.5, 85.0, 85.0), rect, color, pixmap),
    }
}

/// GitHub's pull-request glyph: a branch dot joined to a base dot and a merge dot on the
/// right, stroked like the SwiftUI `PullRequestMark` (unit 16, radius 2.2).
pub(super) fn pull_request(
    rect: (f32, f32, f32, f32),
    stroke: f32,
    color: [u8; 4],
    pixmap: &mut Pixmap,
) {
    let (rx, ry, rw, rh) = rect;
    let unit = rw.min(rh) / 16.0;
    let point = |x: f32, y: f32| (rx + x * unit, ry + y * unit);
    let radius = 2.2 * unit;
    for center in [point(4.0, 4.0), point(4.0, 12.0), point(12.0, 12.0)] {
        let mut circle = PathBuilder::new();
        circle.push_circle(center.0, center.1, radius);
        if let Some(path) = circle.finish() {
            stroked(pixmap, &path, stroke, color);
        }
    }
    let mut pb = PathBuilder::new();
    let (ax, ay) = point(4.0, 6.2);
    let (bx, by) = point(4.0, 9.8);
    let (cx, cy) = point(7.0, 4.0);
    let (dx, dy) = point(9.5, 4.0);
    let (e1x, e1y) = point(11.4, 4.0);
    let (e2x, e2y) = point(12.0, 4.6);
    let (fx, fy) = point(12.0, 6.5);
    let (gx, gy) = point(12.0, 9.8);
    pb.move_to(ax, ay);
    pb.line_to(bx, by);
    pb.move_to(cx, cy);
    pb.line_to(dx, dy);
    pb.cubic_to(e1x, e1y, e2x, e2y, fx, fy);
    pb.line_to(gx, gy);
    if let Some(path) = pb.finish() {
        stroked(pixmap, &path, stroke, color);
    }
}

/// A pushpin glyph for the show-mode cap, centred in `rect`. The mac cap swaps
/// `pin.fill` for `pin` between the two show modes; a 12 pt outline turns to mush in
/// this rasteriser, so the caller dims the same silhouette instead.
pub(super) fn pin(rect: (f32, f32, f32, f32), color: [u8; 4], pixmap: &mut Pixmap) {
    let (rx, ry, rw, rh) = rect;
    let u = rw.min(rh);
    let cx = rx + rw / 2.0;
    let cy = ry + rh / 2.0;
    // A pushpin seen head-on: a cap bar, a barrel, a flange and the needle.
    let at = |x: f32, y: f32| (cx + x * u, cy + y * u);
    let outline = [
        (-0.30, -0.40),
        (0.30, -0.40),
        (0.30, -0.24),
        (0.19, -0.24),
        (0.13, 0.06),
        (0.30, 0.06),
        (0.30, 0.18),
        (0.05, 0.18),
        (0.00, 0.46),
        (-0.05, 0.18),
        (-0.30, 0.18),
        (-0.30, 0.06),
        (-0.13, 0.06),
        (-0.19, -0.24),
        (-0.30, -0.24),
    ];
    let mut pb = PathBuilder::new();
    for (index, (x, y)) in outline.iter().enumerate() {
        let (px, py) = at(*x, *y);
        if index == 0 {
            pb.move_to(px, py);
        } else {
            pb.line_to(px, py);
        }
    }
    pb.close();
    if let Some(path) = pb.finish() {
        filled(pixmap, &path, color);
    }
}

/// The "open externally" arrow used by the popover footer.
pub(super) fn open_arrow(
    rect: (f32, f32, f32, f32),
    stroke: f32,
    color: [u8; 4],
    pixmap: &mut Pixmap,
) {
    let (rx, ry, rw, rh) = rect;
    let unit = rw.min(rh);
    let cx = rx + rw / 2.0;
    let cy = ry + rh / 2.0;
    let mut pb = PathBuilder::new();
    pb.move_to(cx - unit * 0.32, cy + unit * 0.32);
    pb.line_to(cx + unit * 0.28, cy - unit * 0.28);
    pb.move_to(cx - unit * 0.06, cy - unit * 0.3);
    pb.line_to(cx + unit * 0.3, cy - unit * 0.3);
    pb.line_to(cx + unit * 0.3, cy + unit * 0.06);
    if let Some(path) = pb.finish() {
        stroked(pixmap, &path, stroke, color);
    }
}

/// A "copy" affordance: two overlapping rounded sheets.
pub(super) fn copy_icon(
    rect: (f32, f32, f32, f32),
    stroke: f32,
    color: [u8; 4],
    pixmap: &mut Pixmap,
) {
    let (rx, ry, rw, rh) = rect;
    let unit = rw.min(rh);
    let mut pb = PathBuilder::new();
    for (x, y) in [
        (rx + rw * 0.18, ry + rh * 0.32),
        (rx + rw * 0.32, ry + rh * 0.14),
    ] {
        let (w, h) = (unit * 0.5, unit * 0.58);
        pb.move_to(x + w * 0.25, y);
        pb.line_to(x + w * 0.75, y);
        pb.cubic_to(x + w, y, x + w, y + h * 0.05, x + w, y + h * 0.25);
        pb.line_to(x + w, y + h * 0.75);
        pb.cubic_to(x + w, y + h, x + w * 0.75, y + h, x + w * 0.75, y + h);
        pb.line_to(x + w * 0.25, y + h);
        pb.cubic_to(x, y + h, x, y + h, x, y + h * 0.75);
        pb.line_to(x, y + h * 0.25);
        pb.cubic_to(x, y, x, y, x + w * 0.25, y);
    }
    if let Some(path) = pb.finish() {
        stroked(pixmap, &path, stroke, color);
    }
}

fn style(stroke: f32) -> Stroke {
    Stroke {
        width: stroke,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Default::default()
    }
}

fn stroked(pixmap: &mut Pixmap, path: &Path, stroke: f32, color: [u8; 4]) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    if let Some(stroked) = path.stroke(&style(stroke), 1.0) {
        pixmap.fill_path(
            &stroked,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn filled(pixmap: &mut Pixmap, path: &Path, color: [u8; 4]) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

pub(super) fn stroke(pixmap: &mut Pixmap, path: &Path, stroke: f32, color: [u8; 4]) {
    stroked(pixmap, path, stroke, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage(shape: &Shape, box_: (f32, f32, f32, f32)) -> usize {
        let mut pixmap = Pixmap::new(48, 48).unwrap();
        fill(
            shape,
            box_,
            (8.0, 8.0, 32.0, 32.0),
            [255, 0, 0, 255],
            &mut pixmap,
        );
        pixmap
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] > 0)
            .count()
    }

    #[test]
    fn every_provider_mark_draws_pixels() {
        assert!(
            coverage(&CLAUDE, (0.0, 0.0, 24.0, 24.0)) > 200,
            "claude starburst"
        );
        assert!(coverage(&CODEX, (0.0, 0.0, 24.0, 24.0)) > 200, "codex knot");
        assert!(
            coverage(&CURSOR, (0.0, 0.0, 466.73, 532.09)) > 200,
            "cursor cube"
        );
        assert!(
            coverage(&ANTIGRAVITY, (13.0, 14.5, 85.0, 85.0)) > 100,
            "antigravity arch"
        );
    }

    #[test]
    fn marks_fit_inside_their_rect() {
        let mut pixmap = Pixmap::new(32, 32).unwrap();
        provider(
            crate::dto::AgentId::Claude,
            (4.0, 4.0, 24.0, 24.0),
            [255, 255, 255, 255],
            &mut pixmap,
        );
        for (index, px) in pixmap.data().as_chunks::<4>().0.iter().enumerate() {
            if px[3] > 0 {
                let x = index % 32;
                let y = index / 32;
                assert!((1..31).contains(&x), "glyph leaks horizontally at {x}");
                assert!((1..31).contains(&y), "glyph leaks vertically at {y}");
            }
        }
    }

    #[test]
    fn stroke_only_marks_draw() {
        let mut pixmap = Pixmap::new(24, 24).unwrap();
        pull_request(
            (2.0, 2.0, 20.0, 20.0),
            1.6,
            [255, 255, 255, 255],
            &mut pixmap,
        );
        assert!(
            pixmap
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] > 0)
                .count()
                > 50
        );
        pin((2.0, 2.0, 20.0, 20.0), [255, 255, 255, 255], &mut pixmap);
        assert!(
            pixmap
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] > 0)
                .count()
                > 50
        );
    }
}
