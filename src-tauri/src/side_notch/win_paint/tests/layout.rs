use super::*;

#[test]
fn metrics_mirror_the_notchcore_constants_at_standard_size() {
    let standard = metrics(NotchSize::Standard, 1.0, Edge::Right);
    assert_eq!(standard.thickness, 76.0);
    assert_eq!(
        standard.cell_length, 73.0,
        "icon slot 46 + label 22 + spacing 3 + padding 2"
    );
    assert_eq!(standard.cell_spacing, 8.0);
    assert_eq!(standard.inset, 40.0);
    assert_eq!(standard.ear, 40.0);
    assert_eq!(standard.ring_stroke, 4.0);
    // Rail length for two cells: cells + spacing + two ears.
    assert_eq!(
        2.0 * standard.cell_length + standard.cell_spacing + 2.0 * standard.inset,
        2.0 * 73.0 + 8.0 + 80.0
    );

    let vertical_m = metrics(NotchSize::Standard, 1.0, Edge::Right);
    let horizontal_m = metrics(NotchSize::Standard, 1.0, Edge::Top);
    assert_eq!(vertical_m.thickness, horizontal_m.cell_length);
    assert_eq!(vertical_m.cell_length, horizontal_m.thickness);
}
#[test]
fn fractional_presets_snap_to_the_display_pixel_grid() {
    let metric_set = metrics(NotchSize::Compact, 1.5, Edge::Right);
    // 76 pt * 0.875 = 66.5 pt; snapped to a whole number of 1.5x device pixels.
    assert_eq!(metric_set.thickness, 100.0 / 1.5);
    let snapped = value(10.25, 1.0, 1.0);
    assert_eq!(snapped, 10.0, "1x displays snap to whole pixels");
}
#[test]
fn rail_cells_stack_along_the_axis_after_the_first_ear() {
    let metric_set = metrics(NotchSize::Standard, 1.0, Edge::Right);
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
    assert_eq!(frames[0].w, metric_set.thickness);

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
#[test]
fn the_collapsed_pill_is_a_thin_strip_centred_on_the_rail() {
    let rail = R::new(1854.0, 480.0, 66.0, 366.0);
    let settings = settings();
    let pill = pill_frame(&settings, &rail);
    assert_eq!(pill.w, 6.0);
    assert_eq!(pill.h, 120.0);
    assert_eq!(pill.mid_y(), rail.mid_y());
    assert_eq!(
        pill.x + pill.w,
        rail.x + rail.w,
        "flush with the right edge"
    );

    let top = pill_frame(
        &NotchSettings {
            edge: Edge::Top,
            ..settings
        },
        &rail,
    );
    assert_eq!(top.h, 6.0);
    assert_eq!(top.x + top.w, rail.x + rail.w);
}
#[test]
fn the_popover_places_inward_from_the_edge_and_clamps_to_the_work_area() {
    let panel = display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0);
    let cell = R::new(1844.0, 300.0, 76.0, 73.0);
    let placed = popover_frame(&cell, Edge::Right, (272.0, 400.0), &panel, 1.0);
    assert_eq!(placed.w, 272.0);
    assert_eq!(
        placed.x,
        cell.x - 2.0 - 272.0,
        "inward from the edge with a 2 pt gap"
    );
    assert!(
        (placed.mid_y() - cell.mid_y()).abs() <= 0.5,
        "centred on the cell within a pixel"
    );

    let tiny = display("d1", 0.0, 0.0, 400.0, 600.0, 1.0);
    let cell_on_tiny = R::new(120.0, 100.0, 76.0, 73.0);
    let clamped = popover_frame(&cell_on_tiny, Edge::Right, (272.0, 400.0), &tiny, 1.0);
    assert_eq!(clamped.x, 8.0, "clamped to the display margin");
    assert_eq!(clamped.y, 8.0);
}
#[test]
fn plan_hides_when_layout_hides_and_rails_when_it_fits() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![CellData::Provider(provider_data(AgentId::Claude, 42.0))];
    let planned = plan(&settings(), &displays, &data(cells), Hover::default()).expect("fits");
    assert_eq!(planned.cells.len(), 1);
    assert_eq!(planned.window.w, 76.0);
    assert!(planned.pill.is_none());
    assert!(planned.popover.is_none());

    let mut mirrored_display = display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0);
    mirrored_display.mirrored = true;
    let mirrored = vec![mirrored_display];
    assert!(plan(&settings(), &mirrored, &data(Vec::new()), Hover::default()).is_none());

    let off_screen = vec![display("other", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    assert!(plan(
        &settings(),
        &off_screen,
        &data(Vec::new()),
        Hover::default()
    )
    .is_none());
}
#[test]
fn the_popover_unions_with_the_rail_and_places_zones() {
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
    let popover = planned.popover.as_ref().expect("popover planned");
    assert!(
        popover.rect.w > 272.0,
        "the window widens past the rail: {}",
        popover.rect.w
    );
    // The window (display coords) is the union of the 76 pt rail and the 280 pt popover
    // (272 card + 8 tail) with the 2 pt gap; the popover hugs the window's start.
    assert_eq!(planned.window.w, 76.0 + 280.0 + 2.0);
    assert_eq!(
        popover.rect.x, 0.0,
        "the popover sits at the window's inner edge"
    );
    assert_eq!(
        planned.rail.x,
        280.0 + 2.0,
        "the rail keeps its distance inward"
    );
    assert_eq!(
        popover
            .zones
            .iter()
            .filter(|(zone, _)| matches!(zone, Zone::Footer))
            .count(),
        1,
        "exactly one footer link"
    );
    assert!(
        popover.entries.iter().any(|(item, _)| matches!(
            item,
            PopItem::Bar {
                percent: Some(42.0),
                ..
            }
        )),
        "the quota bar carries the percent"
    );
    assert!(popover.entries.iter().any(
        |(item, _)| matches!(item, PopItem::Text { text, .. } if text.contains("Claude Usage"))
    ),);
}
#[test]
fn pull_request_popovers_carry_open_copy_and_footer_zones() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let row = PrRowData {
        id: "node-1".into(),
        number: 7,
        title: "Fix the flaky resize test on Windows".into(),
        url: "https://github.com/octo/tools/pull/7".into(),
        repo: "octo/tools".into(),
        is_draft: false,
        review_decision: Some(ReviewDecision::Approved),
        ci: CiState::Success,
        merge_kind: Some(MergeKind::Ready),
    };
    let cells = vec![CellData::PullRequests(PrCellData {
        status: GithubStatus::Ok,
        hint: None,
        stale: false,
        lists: vec![PrListData {
            id: GithubList::Mine,
            total: 2,
            items: vec![row],
        }],
    })];
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
    let popover = planned.popover.as_ref().expect("popover planned");
    assert_eq!(
        popover
            .zones
            .iter()
            .filter(|(zone, _)| matches!(zone, Zone::OpenRow { .. }))
            .count(),
        1
    );
    assert_eq!(
        popover
            .zones
            .iter()
            .filter(|(zone, _)| matches!(zone, Zone::CopyRow { .. }))
            .count(),
        1
    );
    assert_eq!(
        popover
            .zones
            .iter()
            .filter(|(zone, _)| matches!(zone, Zone::Footer))
            .count(),
        1
    );
    // The badges show approved and ready-to-merge wording.
    assert!(popover.entries.iter().any(
        |(item, _)| matches!(item, PopItem::Text { text, color, .. } if text == "Approved" && *color == LIVE_GREEN)
    ));
}
#[test]
fn the_popover_tail_points_at_its_own_cell() {
    // A full rail, with the last cell's popover clamped against the work area: the
    // card then starts below the window's top, so a tail measured from the card but
    // drawn from the window lands too high.
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells: Vec<CellData> = (0..5)
        .map(|index| {
            CellData::Provider(provider_data(
                [
                    AgentId::Claude,
                    AgentId::Codex,
                    AgentId::Antigravity,
                    AgentId::Cursor,
                    AgentId::Claude,
                ][index],
                40.0,
            ))
        })
        .collect();
    let last = cells.len() - 1;
    let planned = plan(
        &settings(),
        &displays,
        &data(cells),
        Hover {
            active: Some(last),
            ..Hover::default()
        },
    )
    .expect("fits");
    let popover = planned.popover.as_ref().expect("the popover is open");
    assert!(
        popover.card.y > 1.0,
        "this case only bites once the card is pushed off the window's top edge: {}",
        popover.card.y
    );
    let pixmap = render(&planned);
    // Two pixels into the tail, past the card's edge.
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
#[test]
fn popover_hit_zones_sit_on_the_thing_they_stand_for() {
    // Zones and entries come out of the same walk in card coordinates; only entries
    // used to be moved onto the card, so every popover affordance was inert.
    let pulls = PrCellData {
        status: GithubStatus::Ok,
        hint: None,
        stale: false,
        lists: vec![PrListData {
            id: GithubList::Mine,
            total: 1,
            items: vec![PrRowData {
                id: "n1".into(),
                number: 7,
                title: "Fix the flaky resize test on Windows".into(),
                url: "https://github.com/o/r/pull/7".into(),
                repo: "o/r".into(),
                is_draft: false,
                review_decision: None,
                ci: CiState::Success,
                merge_kind: None,
            }],
        }],
    };
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    // The pull-request cell is last, so its popover is centred low and the card no
    // longer starts at the window's top edge.
    let cells = vec![
        CellData::Provider(provider_data(AgentId::Claude, 42.0)),
        CellData::Provider(provider_data(AgentId::Codex, 42.0)),
        CellData::PullRequests(pulls),
    ];
    let last = cells.len() - 1;
    let planned = plan(
        &settings(),
        &displays,
        &data(cells),
        Hover {
            active: Some(last),
            ..Hover::default()
        },
    )
    .expect("fits");
    let popover = planned.popover.as_ref().expect("the popover is open");
    let card = popover.card;
    assert!(card.y > 1.0, "the card starts below the window top");
    for (zone, rect) in &popover.zones {
        assert!(
            rect.x >= card.x
                && rect.y >= card.y
                && rect.x + rect.w <= card.x + card.w + 1.0
                && rect.y + rect.h <= card.y + card.h + 1.0,
            "{zone:?} at {rect:?} is inside the card {card:?}"
        );
    }
    let footer = popover
        .zones
        .iter()
        .find_map(|(zone, rect)| matches!(zone, Zone::Footer).then_some(*rect))
        .expect("a footer zone");
    let label = popover
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Text { text, .. } if text == "Open Pull requests" => Some(*rect),
            _ => None,
        })
        .expect("the footer label is planned");
    assert!(
        footer.contains(label.mid_x(), label.mid_y()),
        "the footer zone covers its label: zone {footer:?}, label {label:?}"
    );
    let row = popover
        .zones
        .iter()
        .find_map(|(zone, rect)| matches!(zone, Zone::OpenRow { .. }).then_some(*rect))
        .expect("a row zone");
    let title = popover
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Text { text, .. } if text.starts_with("Fix the flaky") => Some(*rect),
            _ => None,
        })
        .expect("the row title is planned");
    assert!(
        row.contains(title.mid_x(), title.mid_y()),
        "the row zone covers its title: zone {row:?}, title {title:?}"
    );
}
#[test]
fn the_footer_link_reads_label_then_arrow() {
    let (planned, _) = popover_render(claude_with(vec![window(
        "w",
        "Weekly - all models",
        LimitWindowKind::Weekly,
        50.0,
    )]));
    let popover = planned.popover.as_ref().expect("the popover is open");
    let arrow = popover
        .entries
        .iter()
        .find_map(|(item, rect)| {
            matches!(
                item,
                PopItem::Mark {
                    kind: MarkKind::OpenArrow,
                    ..
                }
            )
            .then_some(*rect)
        })
        .expect("the footer arrow is planned");
    let label = popover
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Text { text, .. } if text == "Open Limits" => Some(*rect),
            _ => None,
        })
        .expect("the footer label is planned");
    assert!(
        arrow.x > label.x,
        "the arrow trails the label like the macOS FooterLink: label {} arrow {}",
        label.x,
        arrow.x
    );
}
