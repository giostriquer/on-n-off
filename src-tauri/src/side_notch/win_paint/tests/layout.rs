use super::*;
use crate::side_notch::model::NotchSize;

#[test]
fn values_snap_to_the_display_pixel_grid() {
    assert_eq!(
        value(10.25, 1.0, 1.0),
        10.0,
        "1x displays snap to whole pixels"
    );
    assert_eq!(
        value(76.0, 0.875, 1.5),
        100.0 / 1.5,
        "a compact cell on a 1.5x display is 100 px wide"
    );
}
#[test]
fn a_compact_popover_on_the_top_edge_is_a_scaled_card_with_its_tail_below() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.5)];
    let current = NotchSettings {
        size: NotchSize::Compact,
        edge: Edge::Top,
        ..settings()
    };
    let rail_data = RailData {
        settings: current.clone(),
        cells: vec![CellData::Provider(session_only(AgentId::Claude, 42.0))],
        action_error: None,
    };
    let planned = plan(
        &current,
        &displays,
        &rail_data,
        Hover {
            active: Some(0),
            ..Hover::default()
        },
    )
    .expect("fits");
    let popover = planned.popover.as_ref().expect("the popover is open");
    assert_eq!(popover.card.w, 238.0, "272 pt at compact");
    assert_eq!(
        popover.rect.w, popover.card.w,
        "the tail is not beside the card"
    );
    assert_eq!(
        popover.rect.h,
        popover.card.h + 11.0 / 1.5,
        "the 8 pt tail at compact is 11 px on a 1.5x display"
    );
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
    let cells = vec![CellData::Provider(session_only(AgentId::Claude, 42.0))];
    let planned = plan(
        &settings(),
        &displays,
        &data(cells.clone()),
        Hover::default(),
    )
    .expect("fits");
    assert_eq!(planned.cells.len(), 1);
    assert_eq!(planned.window.w, 76.0);
    assert!(planned.pill.is_none());
    assert!(planned.popover.is_none());
    assert!(
        plan(&settings(), &displays, &data(Vec::new()), Hover::default()).is_none(),
        "a frame without cells has no rail"
    );

    let mut mirrored_display = display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0);
    mirrored_display.mirrored = true;
    let mirrored = vec![mirrored_display];
    assert!(plan(
        &settings(),
        &mirrored,
        &data(cells.clone()),
        Hover::default()
    )
    .is_none());

    let off_screen = vec![display("other", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    assert!(plan(&settings(), &off_screen, &data(cells), Hover::default()).is_none());
}
#[test]
fn the_popover_unions_with_the_rail_and_places_zones() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![CellData::Provider(session_only(AgentId::Claude, 42.0))];
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
    let mut row = pr(7, "Fix the flaky resize test on Windows", "octo/tools");
    row["reviewDecision"] = "APPROVED".into();
    row["mergeKind"] = "ready".into();
    let cells = vec![pulls_read("ok", None, 2, vec![row])];
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
    assert!(popover.entries.iter().any(
        |(item, _)| matches!(item, PopItem::Text { text, color, .. } if text == "Approved" && *color == LIVE_GREEN)
    ));
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
    let pulls = pulls(vec![pr(7, "Fix the flaky resize test on Windows", "o/r")]);
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![
        CellData::Provider(session_only(AgentId::Claude, 42.0)),
        CellData::Provider(session_only(AgentId::Codex, 42.0)),
        pulls,
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
