use super::*;

#[test]
fn rendering_is_deterministic_and_lands_ink_where_planned() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![CellData::Provider(session_only(AgentId::Claude, 42.0))];
    let planned = plan(&displays, &data(cells), open_on(CLAUDE), now()).expect("fits");
    let first = render(&planned);
    let second = render(&planned);
    assert_eq!(first.data(), second.data(), "same plan, same bytes");
    assert_eq!(
        first.width(),
        (planned.window.w * planned.display_scale).round() as u32
    );

    let cell = &planned.cells[0];
    let scale = planned.display_scale as f32;
    let cx = (cell.rect.mid_x() * scale as f64) as u32;
    let cy = ((cell.rect.y + planned.metrics.cell.padding + planned.metrics.cell.icon_slot / 2.0)
        * scale as f64) as u32;
    let pixel = first.pixel(cx, cy).expect("inside the pixmap");
    assert!(pixel.alpha() > 200, "the glyph slot area has ink");

    let popover = planned.popover.as_ref().expect("the popover is open");
    let gap_from = popover.rect.x + popover.rect.w;
    assert!(
        planned.rail.x - gap_from >= 1.0,
        "the popover keeps its gap from the rail"
    );
    let column = ((gap_from + (planned.rail.x - gap_from) / 2.0) * scale as f64) as u32;
    let tail = cell.rect.mid_y() * scale as f64;
    let [upper, lower] = popover.tail.base;
    let tail_band = ((lower.1 - upper.1) / 2.0 + 1.0) * scale as f64;
    let mut probed = 0;
    for y in 0..first.height() {
        if (f64::from(y) - tail).abs() <= tail_band {
            continue;
        }
        let pixel = first.pixel(column, y).expect("inside the pixmap");
        assert_eq!(
            pixel.alpha(),
            0,
            "per-pixel alpha keeps the gap between the popover and the rail see-through at row {y}"
        );
        probed += 1;
    }
    assert!(
        probed > 100,
        "the gap was probed down the window: {probed} rows"
    );
}
#[test]
fn the_collapsed_pill_renders_only_the_strip() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![CellData::Provider(session_only(AgentId::Claude, 42.0))];
    let mut hover_settings = settings();
    hover_settings.show = ShowMode::OnHover;
    let planned = plan(
        &displays,
        &data_for(hover_settings, cells),
        Hover::default(),
        now(),
    )
    .expect("fits");
    let pill = planned.pill.expect("collapsed shows the strip");
    let pixmap = render(&planned);
    assert_eq!(
        pixmap.width(),
        (planned.window.w * planned.display_scale).round() as u32
    );
    let px_x = (pill.mid_x() * planned.display_scale) as u32;
    let px_y = (pill.mid_y() * planned.display_scale) as u32;
    let mid = pixmap.pixel(px_x.min(pixmap.width() - 1), px_y.min(pixmap.height() - 1));
    assert!(
        mid.is_some_and(|px| px.alpha() > 0),
        "the strip is visible at ({px_x},{px_y}) of {}x{}",
        pixmap.width(),
        pixmap.height()
    );
}
#[test]
fn the_quota_bar_is_a_capsule_the_whole_way_across() {
    let (planned, pixmap) = popover_render(claude_with(vec![window(
        "w",
        "Weekly - all models",
        LimitWindowKind::Weekly,
        100.0,
    )]));
    let popover = planned.popover.as_ref().expect("the popover is open");
    let bar = popover
        .entries
        .iter()
        .find_map(|(item, rect)| matches!(item, PopItem::Bar { .. }).then_some(*rect))
        .expect("the quota bar is planned");
    let top = bar.y.round() as u32;
    let bottom = (bar.y + bar.h).round() as u32;
    assert_eq!(bottom - top, 4, "the bar keeps its 4 pt height");
    let middle = column_ink(&pixmap, bar.mid_x().round() as u32, top, bottom);
    assert!(
        middle > 800,
        "the filled bar is solid in the middle: {middle}"
    );
    for fraction in [0.1_f64, 0.9] {
        let x = (bar.x + bar.w * fraction).round() as u32;
        let ink = column_ink(&pixmap, x, top, bottom);
        assert!(
            ink * 10 >= middle * 9,
            "the bar keeps its height at {fraction} of its length: {ink} of {middle}"
        );
    }
}
#[test]
fn the_hover_strip_is_a_capsule_along_its_length() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let mut hover_settings = settings();
    hover_settings.show = ShowMode::OnHover;
    let cells = vec![CellData::Provider(session_only(AgentId::Claude, 42.0))];
    let planned = plan(
        &displays,
        &data_for(hover_settings, cells),
        Hover::default(),
        now(),
    )
    .expect("fits");
    let pill = planned.pill.expect("collapsed shows the strip");
    let pixmap = render(&planned);
    let left = pill.x.round() as u32;
    let right = (pill.x + pill.w).round() as u32;
    let middle = row_ink(&pixmap, pill.mid_y().round() as u32, left, right);
    assert!(middle > 200, "the strip is a few points wide: {middle}");
    for fraction in [0.1_f64, 0.9] {
        let y = (pill.y + pill.h * fraction).round() as u32;
        let ink = row_ink(&pixmap, y, left, right);
        assert!(
            ink * 10 >= middle * 9,
            "the strip keeps its width at {fraction} of its length: {ink} of {middle}"
        );
    }
}
#[test]
fn hovering_the_cap_lightens_the_ear() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![CellData::Provider(session_only(AgentId::Claude, 42.0))];
    let dark = plan(&displays, &data(cells.clone()), Hover::default(), now()).expect("fits");
    let lit = plan(
        &displays,
        &data(cells),
        Hover {
            cap_hovered: true,
            ..Hover::default()
        },
        now(),
    )
    .expect("fits");
    let (x, y) = (10, 30);
    let before = render(&dark).pixel(x, y).expect("inside").red();
    let after = render(&lit).pixel(x, y).expect("inside").red();
    assert!(
        after > before + 8,
        "the hovered cap lightens the silhouette: {before} -> {after}"
    );
}
#[test]
fn the_cap_pin_says_which_show_mode_is_on() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![CellData::Provider(session_only(AgentId::Claude, 42.0))];
    let hover = Hover {
        cap_hovered: true,
        ..Hover::default()
    };
    let pinned = plan(&displays, &data(cells.clone()), hover, now()).expect("fits");
    let mut on_hover = settings();
    on_hover.show = ShowMode::OnHover;
    let loose = plan(
        &displays,
        &data_for(on_hover, cells),
        Hover {
            rail_open: true,
            ..hover
        },
        now(),
    )
    .expect("fits");
    assert!(
        pinned.cap.pinned && !loose.cap.pinned,
        "the two modes differ"
    );
    let (a, b) = (render(&pinned), render(&loose));
    let cap = pinned.cap.rect;
    let mut differing = 0;
    let mut ink = 0;
    for y in cap.y.round() as u32..(cap.y + cap.h).round() as u32 {
        for x in cap.x.round() as u32..(cap.x + cap.w).round() as u32 {
            let (one, two) = (a.pixel(x, y), b.pixel(x, y));
            if one != two {
                differing += 1;
            }
            if two.is_some_and(|px| px.red() > 120) {
                ink += 1;
            }
        }
    }
    assert!(
        differing > 10,
        "the pin looks different in each mode: {differing} pixels"
    );
    assert!(ink > 10, "and on-hover still shows a pin outline: {ink}");
}
#[test]
fn text_is_measured_at_the_size_it_is_drawn_on_a_scaled_display() {
    for scale in [1.0_f64, 2.0] {
        let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, scale)];
        let planned = plan(
            &displays,
            &data(vec![CellData::Provider(claude_with(vec![window(
                "w",
                "Weekly - all models",
                LimitWindowKind::Weekly,
                50.0,
            )]))]),
            open_on(CLAUDE),
            now(),
        )
        .expect("fits");
        let popover = planned.popover.as_ref().expect("the popover is open");
        let label = popover
            .entries
            .iter()
            .find_map(|(item, rect)| match item {
                PopItem::Text(Text { text, .. }) if text == "Open Limits" => Some(*rect),
                _ => None,
            })
            .expect("the footer label is planned");
        let pixmap = render(&planned);
        let (x0, x1) = (
            (label.x * scale).round() as u32,
            ((label.x + label.w) * scale).round() as u32,
        );
        let (y0, y1) = (
            (label.y * scale).round() as u32,
            ((label.y + label.h) * scale).round() as u32,
        );
        let inked: Vec<u32> = (x0..x1)
            .filter(|x| (y0..y1).any(|y| pixmap.pixel(*x, y).is_some_and(|px| px.red() > 60)))
            .collect();
        assert!(!inked.is_empty(), "the footer label draws at scale {scale}");
        let drawn = f64::from(inked[inked.len() - 1] - inked[0] + 1);
        let planned_width = label.w * scale;
        assert!(
            (drawn - planned_width).abs() <= 3.0,
            "at scale {scale} the label was planned {planned_width} wide and drew {drawn}"
        );
    }
}

#[test]
fn the_header_glyph_sits_on_the_cap_band_of_its_title() {
    let (planned, pixmap) = popover_render(claude_with(vec![window(
        "w",
        "Weekly - all models",
        LimitWindowKind::Weekly,
        50.0,
    )]));
    let popover = planned.popover.as_ref().expect("the popover is open");
    let mark = popover
        .entries
        .iter()
        .find_map(|(item, rect)| matches!(item, PopItem::Mark { .. }).then_some(*rect))
        .expect("the header mark is planned");
    let title = popover
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Text(Text { text, .. }) if text.ends_with("Usage") => Some(*rect),
            _ => None,
        })
        .expect("the header title is planned");
    let top = (title.y - 4.0).max(0.0).round() as u32;
    let bottom = (title.y + title.h + 4.0).round() as u32;
    let ink_centre = |x0: f64, x1: f64| -> f64 {
        let rows: Vec<u32> = (top..bottom)
            .filter(|y| {
                (x0.round() as u32..x1.round() as u32)
                    .any(|x| pixmap.pixel(x, *y).is_some_and(|px| px.red() > 90))
            })
            .collect();
        assert!(!rows.is_empty(), "ink between {x0} and {x1}");
        f64::from(rows[0] + rows[rows.len() - 1]) / 2.0
    };
    let glyph = ink_centre(mark.x, mark.x + mark.w);
    let caps = ink_centre(title.x, title.x + 8.0);
    assert!(
        (glyph - caps).abs() <= 1.0,
        "the glyph sits on the capitals: glyph {glyph}, caps {caps}"
    );
}

#[test]
fn conflict_blend_stays_in_the_ring_and_preserves_the_icon() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 2.0)];
    let mut planned = plan(
        &displays,
        &data(vec![CellData::Provider(session_only(
            AgentId::Claude,
            42.0,
        ))]),
        Hover::default(),
        now(),
    )
    .expect("fits");
    let track = planned.cells[0].arcs[0];
    let passing = Arc {
        from: -90.0,
        to: 270.0,
        ink: ArcInk::Solid(LIVE_GREEN),
        round: false,
        ..track
    };
    planned.cells[0].arcs = vec![track, passing];
    let plain = render(&planned);
    planned.cells[0].arcs[1].ink = ArcInk::Conflict;
    let conflict = render(&planned);
    let scale = planned.display_scale;
    let cell = &planned.cells[0];
    let cx = cell.rect.mid_x() * scale;
    let cy =
        (cell.rect.y + planned.metrics.cell.padding + planned.metrics.cell.icon_slot / 2.0) * scale;
    let stroke = planned.metrics.cell.ring_stroke * scale;
    let radius = (planned.metrics.cell.icon_slot * scale - stroke) / 2.0;
    let mut red = 0;
    for (index, (before, after)) in plain.pixels().iter().zip(conflict.pixels()).enumerate() {
        if before != after {
            let x = (index % plain.width() as usize) as f64;
            let y = (index / plain.width() as usize) as f64;
            assert!(
                ((x - cx).hypot(y - cy) - (radius + stroke / 3.0)).abs()
                    <= stroke / 6.0 + stroke * 0.18 + 1.5,
                "only the outer band and its transition may change; preserve the icon and label"
            );
            if after.red() > after.green() {
                red += 1;
            }
        }
    }
    assert!(red > 0, "the outer band conveys the conflict");
}

#[test]
fn the_popover_tail_points_at_its_own_cell() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells: Vec<CellData> = (0..5)
        .map(|index| {
            CellData::Provider(session_only(
                [
                    AgentId::Claude,
                    AgentId::Codex,
                    AgentId::Antigravity,
                    AgentId::Cursor,
                    AgentId::Muse,
                ][index],
                40.0,
            ))
        })
        .collect();
    let last = cells.len() - 1;
    let planned = plan(
        &displays,
        &data(cells),
        open_on(CellId::Provider(AgentId::Muse)),
        now(),
    )
    .expect("fits");
    let popover = planned.popover.as_ref().expect("the popover is open");
    assert!(
        popover.card.y > 1.0,
        "this case only bites once the card is pushed off the window's top edge: {}",
        popover.card.y
    );
    let pixmap = render(&planned);
    let x = (popover.card.x + popover.card.w + 2.0).round() as u32;
    let rows: Vec<u32> = (0..pixmap.height())
        .filter(|y| pixmap.pixel(x, *y).is_some_and(|px| px.alpha() > 0))
        .collect();
    assert!(!rows.is_empty(), "the tail draws beside the card");
    let centre = f64::from(rows[0] + rows[rows.len() - 1]) / 2.0;
    let cell = planned.cells[last].rect.mid_y();
    assert!(
        (centre - cell).abs() <= 2.0,
        "the tail's apex tracks the cell it belongs to: tail {centre}, cell {cell}"
    );
}

fn longest_run(pixmap: &tiny_skia::Pixmap, ink: Color) -> u32 {
    let mut longest = 0;
    for x in 0..pixmap.width() {
        let mut run = 0;
        for y in 0..pixmap.height() {
            let hit = pixmap
                .pixel(x, y)
                .is_some_and(|px| [px.red(), px.green(), px.blue(), px.alpha()] == ink);
            run = if hit { run + 1 } else { 0 };
            longest = longest.max(run);
        }
    }
    longest
}

fn bluish([r, g, b]: [f64; 3]) -> bool {
    b > 100.0 && b > r * 1.4 && b > g * 1.15
}

fn lavender([r, g, b]: [f64; 3]) -> bool {
    b > 100.0 && b > r && r > g * 1.05 && b > g * 1.2
}

fn hued_pixels(pixmap: &tiny_skia::Pixmap, rect: R, hue: fn([f64; 3]) -> bool) -> usize {
    let xs = rect.x.floor() as u32..(rect.x + rect.w).ceil() as u32;
    let ys = rect.y.floor() as u32..(rect.y + rect.h).ceil() as u32;
    ys.flat_map(|y| xs.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| {
            pixmap.pixel(x, y).is_some_and(|px| {
                hue([
                    f64::from(px.red()),
                    f64::from(px.green()),
                    f64::from(px.blue()),
                ])
            })
        })
        .count()
}

#[test]
fn each_row_paints_its_work_or_personal_ink_on_its_stripe_and_copy_icon() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &displays,
        &data(vec![tagged_rows()]),
        open_on(CellId::PullRequests),
        now(),
    )
    .expect("fits");
    let copies: Vec<R> = planned
        .popover
        .as_ref()
        .expect("the popover is open")
        .entries
        .iter()
        .filter_map(|(item, rect)| matches!(item, PopItem::CopyIcon { .. }).then_some(*rect))
        .collect();
    let pixmap = render(&planned);
    for ink in [WORK, PERSONAL] {
        assert!(
            longest_run(&pixmap, ink) >= 20,
            "the painted popover shows an unbroken {ink:?} stripe"
        );
    }
    let painted: Vec<(usize, usize)> = copies
        .iter()
        .map(|rect| {
            (
                hued_pixels(&pixmap, *rect, bluish),
                hued_pixels(&pixmap, *rect, lavender),
            )
        })
        .collect();
    assert!(
        painted[0].0 > 0 && painted[1].1 > 0 && painted[3].1 > 0,
        "the copy icons are painted in their row's ink: {painted:?}"
    );
    assert_eq!(painted[2], (0, 0), "an unread row's copy icon stays muted");
}

fn rgb(pixmap: &tiny_skia::Pixmap, x: u32, y: u32) -> [u8; 4] {
    let pixel = pixmap.pixel(x, y).expect("inside the pixmap");
    [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
}

fn within_three(pixel: [u8; 4], ink: [u8; 4]) -> bool {
    pixel
        .iter()
        .zip(ink)
        .all(|(value, ink)| value.abs_diff(ink) <= 3)
}

#[test]
fn the_cards_border_is_one_point_of_border_ink_then_card_ink() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 2.0)];
    let cells = vec![CellData::Provider(session_only(AgentId::Claude, 42.0))];
    let planned = plan(&displays, &data(cells), open_on(CLAUDE), now()).expect("fits");
    let popover = planned.popover.as_ref().expect("the popover is open");
    assert_eq!(planned.edge, crate::side_notch::model::Edge::Right);
    assert_eq!(popover.border.width, 1.0, "a standard border is 1 pt");
    let pixmap = render(&planned);
    let scale = planned.display_scale;
    let width = (popover.border.width * scale).round() as u32;
    let edge = (popover.card.x * scale).round() as u32;
    let row = (popover.card.mid_y() * scale).round() as u32;
    for inset in 0..width {
        let pixel = rgb(&pixmap, edge + inset, row);
        assert!(
            within_three(pixel, [40, 40, 43, 255]),
            "{inset} px in from the card's far edge is border ink, not {pixel:?}"
        );
    }
    let pixel = rgb(&pixmap, edge + width, row);
    assert!(
        within_three(pixel, POPOVER_INK),
        "{width} px in the card's own ink begins, not {pixel:?}"
    );
}

fn outside(x: f64, y: f64, rect: R, radius: f64) -> bool {
    let (left, top, right, bottom) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
    if x <= left - 0.5 || x >= right + 0.5 || y <= top - 0.5 || y >= bottom + 0.5 {
        return true;
    }
    let dx = (left + radius - x).max(x - (right - radius)).max(0.0);
    let dy = (top + radius - y).max(y - (bottom - radius)).max(0.0);
    dx > 0.0 && dy > 0.0 && dx.hypot(dy) > radius + 1.0
}

#[test]
fn a_popover_taller_than_its_maximum_paints_its_contents_only_inside_the_clip() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 400.0, 2.0)];
    let planned = plan(
        &displays,
        &data(vec![pulls(long_rows(25))]),
        open_on(CellId::PullRequests),
        now(),
    )
    .expect("fits");
    let popover = planned.popover.as_ref().expect("the popover is open");
    let clip = popover.clip.expect("the card cuts what overflows");
    let with = |entries: Vec<(PopItem, R)>| {
        let mut changed = planned.clone();
        changed.popover.as_mut().expect("open").entries = entries;
        render(&changed)
    };
    let empty = with(Vec::new());
    let contents = render(&planned);
    let covered = with(vec![(
        PopItem::Divider,
        R::new(0.0, 0.0, planned.window.w, planned.window.h),
    )]);
    let scale = planned.display_scale;
    let rect = R::new(
        clip.rect.x * scale,
        clip.rect.y * scale,
        clip.rect.w * scale,
        clip.rect.h * scale,
    );
    let radius = clip.radius * scale;
    let (mut outside_checked, mut contents_inside, mut covered_inside) = (0, 0, 0);
    for y in 0..empty.height() {
        for x in 0..empty.width() {
            let bare = rgb(&empty, x, y);
            if outside(f64::from(x) + 0.5, f64::from(y) + 0.5, rect, radius) {
                outside_checked += 1;
                assert_eq!(
                    rgb(&contents, x, y),
                    bare,
                    "contents painted outside the clip at {x},{y}"
                );
                assert_eq!(
                    rgb(&covered, x, y),
                    bare,
                    "a covering item painted outside the clip at {x},{y}"
                );
            } else {
                contents_inside += usize::from(rgb(&contents, x, y) != bare);
                covered_inside += usize::from(rgb(&covered, x, y) != bare);
            }
        }
    }
    assert!(
        outside_checked > 1000,
        "the card's rim and the rail were compared: {outside_checked}"
    );
    assert!(contents_inside > 0, "the contents paint inside the clip");
    assert!(
        covered_inside > 1000,
        "a covering item paints the clip: {covered_inside}"
    );
}

fn device(rect: R, scale: f64) -> R {
    R::new(
        rect.x * scale,
        rect.y * scale,
        rect.w * scale,
        rect.h * scale,
    )
}

fn white(px: tiny_skia::PremultipliedColorU8) -> bool {
    let channels = [px.red(), px.green(), px.blue()];
    let (low, high) = (
        channels.into_iter().min().unwrap_or(0),
        channels.into_iter().max().unwrap_or(0),
    );
    low > 70 && high - low < 30
}

fn near(px: tiny_skia::PremultipliedColorU8, ink: Color) -> bool {
    [px.red(), px.green(), px.blue()]
        .into_iter()
        .zip(ink)
        .all(|(drawn, wanted)| drawn.abs_diff(wanted) <= 40)
}

fn ink_bounds(
    pixmap: &tiny_skia::Pixmap,
    rect: R,
    band: f64,
    inked: fn(tiny_skia::PremultipliedColorU8) -> bool,
) -> Option<[f64; 4]> {
    let xs = (rect.x - band).floor().max(0.0) as u32..(rect.x + rect.w + band).ceil() as u32;
    let ys = (rect.y - band).floor().max(0.0) as u32..(rect.y + rect.h + band).ceil() as u32;
    let hits: Vec<(u32, u32)> = ys
        .flat_map(|y| xs.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| pixmap.pixel(x, y).is_some_and(inked))
        .collect();
    let left = hits.iter().map(|hit| hit.0).min()?;
    let right = hits.iter().map(|hit| hit.0).max()?;
    let top = hits.iter().map(|hit| hit.1).min()?;
    let bottom = hits.iter().map(|hit| hit.1).max()?;
    Some([
        f64::from(left),
        f64::from(top),
        f64::from(right + 1),
        f64::from(bottom + 1),
    ])
}

#[test]
fn at_twice_the_scale_each_mark_tail_fill_and_right_run_lands_in_its_planned_device_rect() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 2.0)];
    let cells = vec![CellData::Provider(claude_with(vec![window(
        "w",
        "Weekly - all models",
        LimitWindowKind::Weekly,
        50.0,
    )]))];
    let hover = Hover {
        cap_hovered: true,
        ..open_on(CLAUDE)
    };
    let planned = plan(&displays, &data(cells), hover, now()).expect("fits");
    let scale = planned.display_scale;
    assert_eq!((scale, planned.edge), (2.0, Edge::Right));
    let pixmap = render(&planned);

    let glyph = device(planned.cells[0].glyph, scale);
    let drawn = ink_bounds(&pixmap, glyph, 4.0, white).expect("the cell glyph draws");
    let wanted = [glyph.x, glyph.y, glyph.x + glyph.w, glyph.y + glyph.h];
    assert!(
        drawn
            .iter()
            .zip(wanted)
            .all(|(drawn, wanted)| (drawn - wanted).abs() <= 2.0),
        "the cell glyph fills its device rect {wanted:?} and nothing around it: {drawn:?}"
    );

    let pin = device(planned.cap.pin, scale);
    let [left, top, right, bottom] =
        ink_bounds(&pixmap, pin, 4.0, white).expect("the cap pin draws");
    assert!(
        left >= pin.x - 1.0
            && top >= pin.y - 1.0
            && right <= pin.x + pin.w + 1.0
            && bottom <= pin.y + pin.h + 1.0,
        "the pin's ink stays in its device rect {pin:?}: {:?}",
        [left, top, right, bottom]
    );
    assert!(
        ((left + right) / 2.0 - pin.mid_x()).abs() <= 1.5 && bottom - top >= pin.h * 0.75,
        "the pin is centred in and spans its device rect {pin:?}: {:?}",
        [left, top, right, bottom]
    );

    let popover = planned.popover.as_ref().expect("the popover is open");
    let (apex_x, apex_y) = (popover.tail.apex.0 * scale, popover.tail.apex.1 * scale);
    let base_x = popover.tail.base[0].0 * scale;
    let inside = (apex_x - (apex_x - base_x) / 4.0).floor() as u32;
    assert!(
        pixmap
            .pixel(inside, apex_y.floor() as u32)
            .is_some_and(|px| px.alpha() > 200),
        "the tail reaches towards its device apex ({apex_x}, {apex_y})"
    );
    let beyond = (apex_x + 1.0).ceil() as u32..(planned.rail.x * scale - 1.0).floor() as u32;
    assert!(!beyond.is_empty(), "the rail leaves a gap past the apex");
    let rows = apex_y.floor() as u32 - 3..=apex_y.floor() as u32 + 3;
    for x in beyond {
        for y in rows.clone() {
            assert_eq!(
                pixmap.pixel(x, y).map(|px| px.alpha()),
                Some(0),
                "nothing is drawn past the apex at ({x}, {y})"
            );
        }
    }

    let card_right = ((popover.card.x + popover.card.w) * scale).floor() as u32;
    let (fill, ink, bar) = popover
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Bar {
                fill: Some(fill),
                color,
            } => Some((*fill, *color, device(*rect, scale))),
            _ => None,
        })
        .expect("the quota bar is filled");
    let end = bar.x + fill * scale;
    assert!(
        end < bar.x + bar.w - 8.0,
        "the case leaves track past the fill"
    );
    let row = bar.mid_y().floor() as u32;
    let filled: Vec<u32> = (0..card_right)
        .filter(|x| pixmap.pixel(*x, row).is_some_and(|px| near(px, ink)))
        .collect();
    let (&first, &last) = filled
        .first()
        .zip(filled.last())
        .expect("the bar's fill draws");
    assert_eq!(filled.len() as u32, last - first + 1, "the fill is one run");
    assert!(
        f64::from(first) <= bar.x + 1.0 && (f64::from(last + 1) - end).abs() <= 1.5,
        "the fill runs from the bar's start {} to {end}, drawn {first}..={last}",
        bar.x
    );

    let note = popover
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Text(Text { right: true, .. }) => Some(device(*rect, scale)),
            _ => None,
        })
        .expect("the reset note is right-aligned");
    let rows = note.y.ceil() as u32..(note.y + note.h).floor() as u32;
    let rightmost = (0..card_right - 2)
        .filter(|x| {
            rows.clone()
                .any(|y| pixmap.pixel(*x, y).is_some_and(|px| px.red() > 60))
        })
        .max()
        .expect("the note's row draws");
    let planned_right = note.x + note.w;
    assert!(
        (planned_right - 5.0..=planned_right + 1.0).contains(&f64::from(rightmost + 1)),
        "the right-aligned run ends at its device rect's right edge {planned_right}: drawn to {}",
        rightmost + 1
    );
}
