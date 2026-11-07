use super::*;
use crate::side_notch::model::NotchSize;

#[test]
fn values_snap_to_the_display_pixel_grid() {
    assert_eq!(
        Scale {
            size: 1.0,
            display: 1.0
        }
        .value(10.25),
        10.0,
        "1x displays snap to whole pixels"
    );
    assert_eq!(
        Scale {
            size: 0.875,
            display: 1.5
        }
        .value(76.0),
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
    let rail_data = data_for(
        current,
        vec![CellData::Provider(session_only(AgentId::Claude, 42.0))],
    );
    let planned = plan(&displays, &rail_data, open_on(CLAUDE), now()).expect("fits");
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
fn plan_hides_when_layout_hides_and_rails_when_it_fits() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![CellData::Provider(session_only(AgentId::Claude, 42.0))];
    let planned = plan(&displays, &data(cells.clone()), Hover::default(), now()).expect("fits");
    assert_eq!(planned.cells.len(), 1);
    assert_eq!(planned.window.w, 76.0);
    assert!(planned.pill.is_none());
    assert!(planned.popover.is_none());
    assert!(
        plan(&displays, &data(Vec::new()), Hover::default(), now()).is_none(),
        "a frame without cells has no rail"
    );

    let mut mirrored_display = display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0);
    mirrored_display.mirrored = true;
    let mirrored = vec![mirrored_display];
    assert!(plan(&mirrored, &data(cells.clone()), Hover::default(), now()).is_none());

    let off_screen = vec![display("other", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    assert!(plan(&off_screen, &data(cells), Hover::default(), now()).is_none());
}
#[test]
fn the_popover_unions_with_the_rail_and_places_zones() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![CellData::Provider(session_only(AgentId::Claude, 42.0))];
    let planned = plan(&displays, &data(cells), open_on(CLAUDE), now()).expect("fits");
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
            .filter(|(zone, _)| matches!(zone, Zone::Footer(_)))
            .count(),
        1,
        "exactly one footer link"
    );
    assert!(
        popover.entries.iter().any(|(item, rect)| matches!(
            item,
            PopItem::Bar { fill: Some(fill), .. } if *fill == rect.w * 42.0 / 100.0
        )),
        "the quota bar fills its share of the width"
    );
    assert!(popover.entries.iter().any(
        |(item, _)| matches!(item, PopItem::Text(Text { text, .. }) if text.contains("Claude Usage"))
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
        &displays,
        &data(cells),
        open_on(CellId::PullRequests),
        now(),
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
            .filter(|(zone, _)| matches!(zone, Zone::Footer(_)))
            .count(),
        1
    );
    assert!(popover.entries.iter().any(
        |(item, _)| matches!(item, PopItem::Text(Text { text, color, .. }) if text == "Approved" && *color == LIVE_GREEN)
    ));
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
    let planned = plan(
        &displays,
        &data(cells),
        open_on(CellId::PullRequests),
        now(),
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
        .find_map(|(zone, rect)| matches!(zone, Zone::Footer(_)).then_some(*rect))
        .expect("a footer zone");
    let label = popover
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Text(Text { text, .. }) if text == "Open Pull requests" => Some(*rect),
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
            PopItem::Text(Text { text, .. }) if text.starts_with("Fix the flaky") => Some(*rect),
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
    let planned = popover_plan(claude_with(vec![window(
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
            PopItem::Text(Text { text, .. }) if text == "Open Limits" => Some(*rect),
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
#[test]
fn a_point_on_the_rail_names_the_cell_under_it() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let cells = vec![
        CellData::Provider(session_only(AgentId::Claude, 42.0)),
        CellData::Provider(session_only(AgentId::Codex, 42.0)),
    ];
    let planned = plan(&displays, &data(cells), Hover::default(), now()).expect("fits");
    let (first, second) = (planned.cells[0].rect, planned.cells[1].rect);
    let codex = Some(Hit::Cell(CellId::Provider(AgentId::Codex)));
    assert_eq!(planned.hit(second.mid_x(), second.mid_y()), codex);
    assert_eq!(
        planned.hit(second.x, second.y),
        codex,
        "a cell's own corner is in it"
    );
    assert_eq!(
        planned.hit(first.mid_x(), first.y + first.h),
        None,
        "the spacing between two cells is in neither"
    );
    let cap = planned.cap.rect;
    assert_eq!(
        planned.hit(cap.mid_x(), cap.mid_y()),
        Some(Hit::Cap),
        "the cap is no cell"
    );
}
#[test]
fn a_rect_holds_its_left_and_top_edges_but_not_its_right_and_bottom() {
    let rect = R::new(10.0, 20.0, 30.0, 40.0);
    assert!(rect.contains(10.0, 20.0), "its top left corner");
    assert!(
        rect.contains(39.5, 59.5),
        "just inside its bottom right corner"
    );
    assert!(
        !rect.contains(40.0, 30.0),
        "its right edge is the next rect's"
    );
    assert!(
        !rect.contains(20.0, 60.0),
        "its bottom edge is the next rect's"
    );
    assert!(!rect.contains(9.5, 30.0) && !rect.contains(20.0, 19.5));
}
#[test]
fn a_collapsed_rail_on_the_right_shows_its_strip_against_the_screen_edge() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let mut on_hover = settings();
    on_hover.show = ShowMode::OnHover;
    let planned = plan(
        &displays,
        &data_for(
            on_hover,
            vec![CellData::Provider(session_only(AgentId::Claude, 42.0))],
        ),
        Hover::default(),
        now(),
    )
    .expect("fits");
    let pill = planned.pill.expect("collapsed shows the strip");
    assert_eq!(planned.window.x, 1920.0 - 76.0, "the window is the rail");
    assert_eq!(
        (pill.x + pill.w, pill.y + pill.h / 2.0),
        (planned.window.w, planned.window.h / 2.0),
        "the strip hugs the screen edge, centred along the rail, in window coordinates"
    );
}
#[test]
fn a_plan_moves_with_its_rail_cells_or_show_mode_but_not_when_a_popover_opens() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let claude = data(vec![CellData::Provider(session_only(
        AgentId::Claude,
        42.0,
    ))]);
    let rail = plan(&displays, &claude, Hover::default(), now()).expect("fits");
    let opened = plan(&displays, &claude, open_on(CLAUDE), now()).expect("fits");
    assert_ne!(opened.window, rail.window, "the popover widens the window");
    assert!(!opened.moved_from(&rail), "an open popover is no move");
    assert!(!rail.moved_from(&rail.clone()));

    let codex = data(vec![CellData::Provider(session_only(AgentId::Codex, 42.0))]);
    let swapped = plan(&displays, &codex, Hover::default(), now()).expect("fits");
    assert_eq!(swapped.window, rail.window);
    assert!(swapped.moved_from(&rail), "another cell in the same slot");

    let mut left = claude.clone();
    left.settings.edge = Edge::Left;
    let moved = plan(&displays, &left, Hover::default(), now()).expect("fits");
    assert!(moved.moved_from(&rail), "the rail is somewhere else");

    let mut on_hover = claude;
    on_hover.settings.show = ShowMode::OnHover;
    let open_rail = Hover {
        rail_open: true,
        ..Hover::default()
    };
    let loose = plan(&displays, &on_hover, open_rail, now()).expect("fits");
    assert_eq!(loose.window, rail.window);
    assert!(loose.moved_from(&rail), "the cap pins another show mode");
}
