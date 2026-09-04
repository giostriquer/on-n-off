use super::*;

#[test]
fn rendering_is_deterministic_and_lands_ink_where_planned() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![CellData::Provider(provider_data(AgentId::Claude, 42.0))];
    let planned = plan(
        &settings(),
        &displays,
        &data(cells),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
    )
    .expect("fits");
    let first = render(&planned);
    let second = render(&planned);
    assert_eq!(first.data(), second.data(), "same plan, same bytes");
    assert_eq!(
        first.width(),
        (planned.window.w * planned.display_scale).round() as u32
    );

    // The pill body colour sits at the icon slot's centre; the ring track surrounds it.
    let cell = &planned.cells[0];
    let scale = planned.display_scale as f32;
    let cx = (cell.rect.mid_x() * scale as f64) as u32;
    let cy = ((cell.rect.y + planned.metrics.cell_padding + planned.metrics.icon_slot / 2.0)
        * scale as f64) as u32;
    let pixel = first.pixel(cx, cy).expect("inside the pixmap");
    assert!(pixel.alpha() > 200, "the glyph slot area has ink");

    // The window is transparent well outside the rail (the popover is inward of it).
    let left_edge = first.pixel(0, first.height() / 2).unwrap();
    assert_eq!(
        left_edge.alpha(),
        0,
        "per-pixel alpha keeps the rest see-through"
    );
}
#[test]
fn the_collapsed_pill_renders_only_the_strip() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![CellData::Provider(provider_data(AgentId::Claude, 42.0))];
    let mut hover_settings = settings();
    hover_settings.show = ShowMode::OnHover;
    let planned = plan(&hover_settings, &displays, &data(cells), Hover::default()).expect("fits");
    let pill = planned.pill.expect("collapsed shows the strip");
    let pixmap = render(&planned);
    assert_eq!(
        pixmap.width(),
        (planned.window.w * planned.display_scale).round() as u32
    );
    // The pill rect is window-local; the strip is drawn exactly there.
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
    // A capsule is full height everywhere past its rounded caps; a flat ellipse
    // tapers away from the middle and reads as a hairline.
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
    let cells = vec![CellData::Provider(provider_data(AgentId::Claude, 42.0))];
    let planned = plan(&hover_settings, &displays, &data(cells), Hover::default()).expect("fits");
    let pill = planned.pill.expect("collapsed shows the strip");
    let pixmap = render(&planned);
    let left = pill.x.round() as u32;
    let right = (pill.x + pill.w).round() as u32;
    // A capsule keeps its width along the strip; an ellipse pinches to a pixel.
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
    let cells = vec![CellData::Provider(provider_data(AgentId::Claude, 42.0))];
    let dark = plan(
        &settings(),
        &displays,
        &data(cells.clone()),
        Hover::default(),
    )
    .expect("fits");
    let lit = plan(
        &settings(),
        &displays,
        &data(cells),
        Hover {
            cap_hovered: true,
            ..Hover::default()
        },
    )
    .expect("fits");
    // A point inside the ear, clear of the pin glyph at 62 % of the cap.
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
    let cells = vec![CellData::Provider(provider_data(AgentId::Claude, 42.0))];
    let hover = Hover {
        cap_hovered: true,
        ..Hover::default()
    };
    let pinned = plan(&settings(), &displays, &data(cells.clone()), hover).expect("fits");
    let mut on_hover = settings();
    on_hover.show = ShowMode::OnHover;
    let loose = plan(
        &on_hover,
        &displays,
        &data(cells),
        Hover {
            rail_open: true,
            ..hover
        },
    )
    .expect("fits");
    assert!(
        pinned.cap.pinned && !loose.cap.pinned,
        "the two modes differ"
    );
    // The two plans differ in nothing but the cap, so the pin has to carry the
    // difference: the mac cap swaps `pin.fill` for `pin` to say which mode is on.
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
    // The planner lays out in points and the renderer rasterises at
    // point x display scale. Measuring at the point size instead of the device size
    // drifts on a 150 % or 200 % display: right-aligned rows and ellipses land wrong.
    for scale in [1.0_f64, 2.0] {
        let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, scale)];
        let planned = plan(
            &settings(),
            &displays,
            &data(vec![CellData::Provider(claude_with(vec![window(
                "w",
                "Weekly - all models",
                LimitWindowKind::Weekly,
                50.0,
            )]))]),
            Hover {
                active: Some(0),
                ..Hover::default()
            },
        )
        .expect("fits");
        let popover = planned.popover.as_ref().expect("the popover is open");
        let label = popover
            .entries
            .iter()
            .find_map(|(item, rect)| match item {
                PopItem::Text { text, .. } if text == "Open Limits" => Some(*rect),
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
    // The mac header is an `HStack` and the app's rows are `flex items-center`; both
    // land a mark beside a title on the middle of the capitals. Centring on the whole
    // ink extent instead drops the mark about a pixel and a half, because Segoe UI's
    // descender runs much deeper below the baseline than its cap line runs above it.
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
            PopItem::Text { text, .. } if text.ends_with("Usage") => Some(*rect),
            _ => None,
        })
        .expect("the header title is planned");
    // Only the header band, so the quota bar below it cannot join the measurement.
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
    // The leading capital on its own: cap line to baseline, with no ascender or
    // descender in the window to widen the band.
    let caps = ink_centre(title.x, title.x + 8.0);
    assert!(
        (glyph - caps).abs() <= 1.0,
        "the glyph sits on the capitals: glyph {glyph}, caps {caps}"
    );
}
