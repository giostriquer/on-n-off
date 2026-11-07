use super::*;

const PURE_RED: Color = [255, 0, 0, 255];

fn at_twice_the_scale(cell: CellData, hover: Hover) -> Plan {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 2.0)];
    let planned = plan(&displays, &data(vec![cell]), hover, now()).expect("fits");
    assert_eq!((planned.display_scale, planned.edge), (2.0, Edge::Right));
    planned
}

struct Ring {
    pixmap: tiny_skia::Pixmap,
    centre: (f64, f64),
    radius: f64,
    stroke: f64,
}

impl Ring {
    fn alone(arc: impl Fn(Arc) -> Arc) -> Self {
        let mut planned = at_twice_the_scale(
            CellData::Provider(session_only(AgentId::Claude, 42.0)),
            Hover::default(),
        );
        let drawn = arc(planned.cells[0].arcs[0]);
        planned.cells[0].arcs = vec![drawn];
        let scale = planned.display_scale;
        Self {
            pixmap: render(&planned),
            centre: (drawn.centre.0 * scale, drawn.centre.1 * scale),
            radius: drawn.radius * scale,
            stroke: drawn.stroke * scale,
        }
    }

    fn at(&self, angle: f64, distance: f64) -> (f64, f64) {
        let angle = angle.to_radians();
        (
            self.centre.0 + distance * angle.cos(),
            self.centre.1 + distance * angle.sin(),
        )
    }

    fn red(&self, (x, y): (f64, f64)) -> f64 {
        self.pixmap
            .pixel(x.floor() as u32, y.floor() as u32)
            .map_or(0.0, |px| {
                (f64::from(px.red()) - f64::from(px.green())) / 255.0
            })
    }
}

#[test]
fn at_twice_the_scale_a_meter_arc_covers_its_span_at_its_stroke_and_a_round_end_caps_it() {
    let meter = |round| {
        Ring::alone(|track| Arc {
            from: -90.0,
            to: 90.0,
            ink: ArcInk::Solid(PURE_RED),
            round,
            ..track
        })
    };
    let ring = meter(true);
    let (radius, stroke) = (ring.radius, ring.stroke);

    let row = ring.centre.1.floor();
    let across: f64 = ((ring.centre.0 + radius - stroke).floor() as u32
        ..(ring.centre.0 + radius + stroke).ceil() as u32)
        .map(|x| ring.red((f64::from(x), row)))
        .sum();
    assert!(
        (across - stroke).abs() <= 0.6,
        "the arc is {stroke} px thick across its middle: {across}"
    );

    for angle in [-45.0, 0.0, 45.0] {
        assert!(
            ring.red(ring.at(angle, radius)) > 0.9,
            "the span is inked at {angle} degrees"
        );
        for side in [-1.0, 1.0] {
            let clear = radius + side * (stroke / 2.0 + 2.0);
            assert!(
                ring.red(ring.at(angle, clear)) < 0.1,
                "no ink {clear} px from the centre at {angle} degrees"
            );
        }
    }
    for angle in [135.0, 180.0, -135.0] {
        assert!(
            ring.red(ring.at(angle, radius)) < 0.1,
            "no ink outside the span at {angle} degrees"
        );
    }

    let end = ring.at(90.0, radius);
    let tangent = (-(90.0_f64.to_radians().sin()), 90.0_f64.to_radians().cos());
    let along = |distance: f64| (end.0 + tangent.0 * distance, end.1 + tangent.1 * distance);
    let (before, past) = (along(-0.3 * stroke), along(0.3 * stroke));
    assert!(
        ring.red(before) > 0.9 && ring.red(past) > 0.9,
        "a round end carries the ink stroke/2 past the end angle"
    );
    let butt = meter(false);
    assert!(
        butt.red(before) > 0.9 && butt.red(past) < 0.1,
        "a butt end stops at the end angle"
    );
}

#[test]
fn at_twice_the_scale_a_conflict_ring_turns_from_green_to_red_across_its_boundary() {
    let ring = Ring::alone(|track| Arc {
        from: -90.0,
        to: 270.0,
        ink: ArcInk::Conflict,
        round: false,
        ..track
    });
    let (radius, stroke) = (ring.radius, ring.stroke);
    let (boundary, blend) = (radius + stroke / 6.0, stroke * 0.18);
    let row = ring.centre.1.floor() as u32;
    let mut sweeps = Vec::new();
    for x in (ring.centre.0 + radius - stroke) as u32..(ring.centre.0 + radius + stroke) as u32 {
        let distance =
            (f64::from(x) + 0.5 - ring.centre.0).hypot(f64::from(row) + 0.5 - ring.centre.1);
        if (distance - radius).abs() > stroke / 2.0 - 1.0 {
            continue;
        }
        let sweep = ((distance - (boundary - blend)) / (2.0 * blend)).clamp(0.0, 1.0);
        let wanted: Vec<f64> = (0..3)
            .map(|channel| {
                let (green, red) = (f64::from(LIVE_GREEN[channel]), f64::from(TRIP_RED[channel]));
                green + (red - green) * sweep
            })
            .collect();
        let px = ring.pixmap.pixel(x, row).expect("inside the pixmap");
        let drawn = [px.red(), px.green(), px.blue()];
        assert!(
            drawn
                .iter()
                .zip(&wanted)
                .all(|(drawn, wanted)| (f64::from(*drawn) - wanted).abs() <= 10.0),
            "at {distance:.2} px from the centre the ring is {drawn:?}, its bands give {wanted:?}"
        );
        sweeps.push(sweep);
    }
    assert!(
        sweeps.contains(&0.0) && sweeps.iter().any(|sweep| *sweep > 0.8),
        "the probe crossed the green inner band and the red outer band: {sweeps:?}"
    );
}

fn assert_stroke_weight(
    pixmap: &tiny_skia::Pixmap,
    rect: (f32, f32, f32, f32),
    stroke: f32,
    background: Color,
    what: &str,
) {
    let mut reference =
        tiny_skia::Pixmap::new(pixmap.width(), pixmap.height()).expect("a reference pixmap");
    marks::pull_request(rect, stroke, WHITE, &mut reference);
    let xs = (rect.0 - 1.0).floor() as u32..(rect.0 + rect.2 + 1.0).ceil() as u32;
    let ys = (rect.1 - 1.0).floor() as u32..(rect.1 + rect.3 + 1.0).ceil() as u32;
    let ground = f64::from(background[0]);
    let (mut drawn, mut wanted) = (0.0, 0.0);
    for y in ys {
        for x in xs.clone() {
            if let (Some(px), Some(want)) = (pixmap.pixel(x, y), reference.pixel(x, y)) {
                drawn += (f64::from(px.red()) - ground).max(0.0) / (255.0 - ground);
                wanted += f64::from(want.alpha()) / 255.0;
            }
        }
    }
    assert!(wanted > 50.0, "the reference {what} draws: {wanted}");
    assert!(
        (drawn / wanted - 1.0).abs() <= 0.04,
        "{what} is stroked {stroke} px: it drew {drawn:.1} px of ink, a {stroke} px stroke draws {wanted:.1}"
    );
}

#[test]
fn at_twice_the_scale_the_pull_request_marks_and_a_hollow_dot_keep_their_stroke_weights() {
    let mut unchecked = pr(7, "Fix the resize test", "acme/webapp");
    unchecked["ci"] = "none".into();
    let planned = at_twice_the_scale(pulls(vec![unchecked]), open_on(CellId::PullRequests));
    let pixmap = render(&planned);
    let scale = planned.display_scale as f32;

    let glyph = planned.cells[0].glyph;
    assert_stroke_weight(
        &pixmap,
        (
            glyph.x as f32 * scale,
            glyph.y as f32 * scale,
            glyph.w as f32 * scale,
            glyph.h as f32 * scale,
        ),
        1.6 * scale,
        RAIL_INK,
        "the cell's pull-request glyph",
    );

    let popover = planned.popover.as_ref().expect("the popover is open");
    let header = popover
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Mark {
                kind: MarkKind::PullRequests,
                size,
            } => Some((
                (rect.x * f64::from(scale)) as f32,
                (rect.y * f64::from(scale)) as f32,
                *size as f32 * scale,
                *size as f32 * scale,
            )),
            _ => None,
        })
        .expect("the header mark is planned");
    assert_stroke_weight(
        &pixmap,
        header,
        1.4 * scale,
        POPOVER_INK,
        "the popover's pull-request mark",
    );

    let (dot, ink) = popover
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Dot { ink, hollow: true } => Some((
                R::new(
                    rect.x * f64::from(scale),
                    rect.y * f64::from(scale),
                    rect.w * f64::from(scale),
                    rect.h * f64::from(scale),
                ),
                *ink,
            )),
            _ => None,
        })
        .expect("a pull request without checks plans a hollow dot");
    let (ground, full) = (f64::from(POPOVER_INK[0]), f64::from(ink[0]));
    let row = dot.mid_y().floor() as u32;
    let across: f64 = ((dot.mid_x() + 3.0) as u32..(dot.mid_x() + 12.0) as u32)
        .filter_map(|x| pixmap.pixel(x, row))
        .map(|px| ((f64::from(px.red()) - ground) / (full - ground)).max(0.0))
        .sum();
    assert!(
        (across - f64::from(scale)).abs() <= 0.5,
        "the hollow dot's ring is 1 pt, {scale} px, thick: {across}"
    );
}
