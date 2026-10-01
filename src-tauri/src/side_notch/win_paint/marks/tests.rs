use super::*;

#[test]
fn every_provider_mark_draws_inside_its_rect() {
    for id in crate::side_notch::model::RAIL_ORDER {
        let mut pixmap = Pixmap::new(48, 48).unwrap();
        provider(
            id,
            (8.0, 8.0, 32.0, 32.0),
            [255, 255, 255, 255],
            &mut pixmap,
        );
        let mut ink = 0;
        for (index, px) in pixmap.data().as_chunks::<4>().0.iter().enumerate() {
            if px[3] > 0 {
                ink += 1;
                let (x, y) = (index % 48, index / 48);
                assert!((7..41).contains(&x), "{id:?} leaks horizontally at {x}");
                assert!((7..41).contains(&y), "{id:?} leaks vertically at {y}");
            }
        }
        assert!(ink > 200, "{id:?} draws only {ink} pixels");
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

const SWIFT: &str = include_str!("../../../../macos/SideNotch/Sources/NotchApp/ProviderMark.swift");

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Move(Point),
    Line(Point),
    Curve { c1: Point, c2: Point, to: Point },
    Close,
}

fn swift_ops(name: &str) -> Vec<Op> {
    let start = SWIFT
        .find(&format!("static let {name}: Path = {{"))
        .unwrap_or_else(|| panic!("{name} is declared in ProviderMark.swift"));
    let block = &SWIFT[start..];
    let end = block
        .find("}()")
        .unwrap_or_else(|| panic!("{name}'s declaration closes"));
    let block = block[..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut ops = Vec::new();
    let mut rest = block.as_str();
    while let Some(at) = rest.find("path.") {
        rest = &rest[at + "path.".len()..];
        if let Some(tail) = rest.strip_prefix("move(") {
            let (points, next) = points(tail, 1);
            ops.push(Op::Move(points[0]));
            rest = next;
        } else if let Some(tail) = rest.strip_prefix("addLine(") {
            let (points, next) = points(tail, 1);
            ops.push(Op::Line(points[0]));
            rest = next;
        } else if let Some(tail) = rest.strip_prefix("addCurve(") {
            let (points, next) = points(tail, 3);
            ops.push(Op::Curve {
                c1: points[1],
                c2: points[2],
                to: points[0],
            });
            rest = next;
        } else if rest.starts_with("closeSubpath()") {
            ops.push(Op::Close);
        }
    }
    ops
}

fn points(text: &str, count: usize) -> (Vec<Point>, &str) {
    let mut out = Vec::new();
    let mut rest = text;
    for _ in 0..count {
        let mut pair = [0.0f32; 2];
        for (slot, label) in pair.iter_mut().zip(["x: ", "y: "]) {
            let at = rest
                .find(label)
                .unwrap_or_else(|| panic!("a `{label}` coordinate in {rest:.40}"));
            rest = &rest[at + label.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                .unwrap_or(rest.len());
            *slot = rest[..end].parse().expect("a number");
            rest = &rest[end..];
        }
        out.push((pair[0], pair[1]));
    }
    (out, rest)
}

fn rust_ops(shape: &Shape) -> Vec<Op> {
    let mut ops = Vec::new();
    for sub in shape.subs {
        ops.push(Op::Move(sub.start));
        for seg in sub.segs {
            ops.push(match seg {
                Seg::Line(to) => Op::Line(*to),
                Seg::Curve { c1, c2, to } => Op::Curve {
                    c1: *c1,
                    c2: *c2,
                    to: *to,
                },
            });
        }
        ops.push(Op::Close);
    }
    ops
}

fn same(left: Op, right: Op) -> bool {
    fn near(a: Point, b: Point) -> bool {
        (a.0 - b.0).abs() <= 0.02 && (a.1 - b.1).abs() <= 0.02
    }
    match (left, right) {
        (Op::Move(a), Op::Move(b)) | (Op::Line(a), Op::Line(b)) => near(a, b),
        (
            Op::Curve {
                c1: a1,
                c2: a2,
                to: a3,
            },
            Op::Curve {
                c1: b1,
                c2: b2,
                to: b3,
            },
        ) => near(a1, b1) && near(a2, b2) && near(a3, b3),
        (Op::Close, Op::Close) => true,
        _ => false,
    }
}

#[test]
fn provider_marks_match_the_swift_originals_op_for_op() {
    for (name, shape) in [
        ("claude", &CLAUDE),
        ("codex", &CODEX),
        ("cursor", &CURSOR),
        ("antigravity", &ANTIGRAVITY),
    ] {
        let swift = swift_ops(name);
        let rust = rust_ops(shape);
        assert!(
            !swift.is_empty(),
            "{name}: the Swift source was parsed, not skipped"
        );
        assert_eq!(
            swift.len(),
            rust.len(),
            "{name}: {} ops in Swift, {} in Rust",
            swift.len(),
            rust.len()
        );
        for (index, (want, have)) in swift.iter().zip(&rust).enumerate() {
            assert!(
                same(*want, *have),
                "{name} op {index}: Swift has {want:?}, Rust has {have:?}"
            );
        }
    }
}
