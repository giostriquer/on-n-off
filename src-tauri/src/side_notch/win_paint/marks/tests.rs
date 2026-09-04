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

// ---------------------------------------------------------------------------
// Parity with the Swift originals.
//
// The shapes above are hand-transcribed from `ProviderMark.swift`, which is exactly the
// kind of work a human eye signs off on and gets wrong: the Cursor cube shipped with the
// endpoint of one curve replaced by the *next* line's point, and every op after it shifted
// by one. The mark still drew pixels, so the coverage tests above passed. Comparing the two
// sources op for op is the only thing that makes "ported from the Swift" a claim rather
// than a hope.

const SWIFT: &str = include_str!("../../../../macos/SideNotch/Sources/NotchApp/ProviderMark.swift");

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Move(Point),
    Line(Point),
    Curve { c1: Point, c2: Point, to: Point },
    Close,
}

/// The declared ops of one `fileprivate static let <name>: Path` in the Swift source.
fn swift_ops(name: &str) -> Vec<Op> {
    let start = SWIFT
        .find(&format!("static let {name}: Path = {{"))
        .unwrap_or_else(|| panic!("{name} is declared in ProviderMark.swift"));
    let block = &SWIFT[start..];
    let end = block
        .find("}()")
        .unwrap_or_else(|| panic!("{name}'s declaration closes"));
    // Swift wraps `addCurve` over three lines; flattening makes one scanner enough.
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
            // Swift names the destination first, then the two controls.
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

/// The next `count` `CGPoint(x: … , y: …)` pairs, and the text after them.
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

/// The same ops, read off the Rust shape.
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

/// The Rust literals carry two decimals where the Swift carries four.
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
