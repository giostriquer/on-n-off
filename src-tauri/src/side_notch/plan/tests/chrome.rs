use super::*;
use crate::side_notch::model::NotchSize;

fn opened_on(edge: Edge, size: NotchSize, scale: f64) -> Plan {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, scale)];
    let cells = [AgentId::Claude, AgentId::Codex, AgentId::Cursor]
        .into_iter()
        .map(|provider| CellData::Provider(session_only(provider, 40.0)))
        .collect();
    let rail = data_for(
        NotchSettings {
            edge,
            size,
            ..settings()
        },
        cells,
    );
    plan(
        &displays,
        &rail,
        open_on(CellId::Provider(AgentId::Codex)),
        now(),
    )
    .expect("fits")
}

#[track_caller]
fn close(actual: f64, expected: f64, case: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{case}: {actual} is not {expected}"
    );
}

#[test]
fn the_tail_leaves_the_card_edge_facing_the_rail_and_points_at_its_cell() {
    for edge in [Edge::Right, Edge::Left, Edge::Top, Edge::Bottom] {
        for (size, scale) in [(NotchSize::Standard, 1.0), (NotchSize::Compact, 1.5)] {
            let planned = opened_on(edge, size, scale);
            let popover = planned.popover.as_ref().expect("the popover is open");
            let (card, tail) = (popover.card, popover.tail);
            let cell = planned.cells[1].rect;
            let [first, last] = tail.base;
            let (edge_at, along, toward) = match edge {
                Edge::Right => (card.x + card.w, cell.mid_y(), 1.0),
                Edge::Left => (card.x, cell.mid_y(), -1.0),
                Edge::Bottom => (card.y + card.h, cell.mid_x(), 1.0),
                Edge::Top => (card.y, cell.mid_x(), -1.0),
            };
            let (base_across, base_along, apex_across, apex_along) = if edge.is_vertical() {
                (
                    [first.0, last.0],
                    [first.1, last.1],
                    tail.apex.0,
                    tail.apex.1,
                )
            } else {
                (
                    [first.1, last.1],
                    [first.0, last.0],
                    tail.apex.1,
                    tail.apex.0,
                )
            };
            let case = format!("{edge:?} {size:?} {scale}x");
            for across in base_across {
                close(
                    across,
                    edge_at,
                    &format!("{case}: the base sits on the card"),
                );
            }
            close(
                apex_along,
                along,
                &format!("{case}: the apex points at its own cell"),
            );
            close(
                base_along[0] + base_along[1],
                2.0 * along,
                &format!("{case}: the base is centred on the apex"),
            );
            assert!(base_along[0] < base_along[1], "{case}");
            assert!(
                (apex_across - edge_at) * toward > 0.0,
                "{case}: the apex reaches toward the rail"
            );
        }
    }
}

#[test]
fn a_cell_centres_its_rings_and_glyph_in_its_icon_slot_with_the_label_below() {
    for (edge, size, scale) in [
        (Edge::Right, NotchSize::Standard, 1.0),
        (Edge::Bottom, NotchSize::Large, 1.25),
    ] {
        let planned = opened_on(edge, size, scale);
        let metrics = planned.metrics.cell;
        for cell in &planned.cells {
            let centre = (
                cell.rect.mid_x(),
                cell.rect.y + metrics.padding + metrics.icon_slot / 2.0,
            );
            assert!(cell.arcs.iter().all(|arc| arc.centre == centre));
            assert_eq!(
                cell.arcs[0].radius,
                (metrics.icon_slot - metrics.ring_stroke) / 2.0,
                "the ring's outer edge meets the slot"
            );
            assert_eq!(
                (cell.glyph.mid_x(), cell.glyph.mid_y()),
                centre,
                "the glyph is centred on the ring"
            );
            assert_eq!(cell.glyph.w, metrics.glyph);
            assert_eq!(
                cell.label_rect.y,
                cell.rect.y + metrics.padding + metrics.icon_slot + metrics.content_spacing,
                "the label sits under the slot"
            );
            assert_eq!(cell.label.size, 17.0 * size.scale());
        }
    }
}

#[test]
fn the_cap_pin_sits_in_the_cap() {
    let planned = opened_on(Edge::Right, NotchSize::Standard, 1.0);
    let (cap, pin) = (planned.cap.rect, planned.cap.pin);
    assert_eq!(
        (pin.mid_x(), pin.mid_y()),
        (cap.x + cap.w * 0.62, cap.y + cap.h * 0.62)
    );
    assert_eq!((pin.w, pin.h), (12.0, 12.0));
}

#[test]
fn the_tail_the_label_nudge_and_the_corners_are_the_helpers_scaled_by_size_on_whole_pixels() {
    for (edge, size, scale) in [
        (Edge::Right, NotchSize::Compact, 1.5),
        (Edge::Top, NotchSize::Large, 1.25),
        (Edge::Left, NotchSize::Standard, 2.0),
        (Edge::Bottom, NotchSize::Compact, 1.0),
    ] {
        let case = format!("{edge:?} {size:?} {scale}x");
        let value =
            |points: f64| crate::side_notch::model::pixel_aligned(points * size.scale(), scale);
        let weekly = claude_with(vec![window(
            "weekly",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            41.0,
        )]);
        let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, scale)];
        let rail = data_for(
            NotchSettings {
                edge,
                size,
                ..settings()
            },
            vec![CellData::Provider(weekly)],
        );
        let planned = plan(&displays, &rail, open_on(CLAUDE), now()).expect("fits");
        let cell = &planned.cells[0];
        close(
            cell.label_rect.mid_x() - cell.arcs[0].centre.0,
            value(2.0),
            &format!("{case}: the figure sits 2 pt right of the ring's centre"),
        );
        let popover = planned.popover.as_ref().expect("the popover is open");
        assert_eq!(popover.radius, value(12.0), "{case}: the card's corners");
        let [first, last] = popover.tail.base;
        let (half, length) = if edge.is_vertical() {
            (
                (last.1 - first.1) / 2.0,
                (popover.tail.apex.0 - first.0).abs(),
            )
        } else {
            (
                (last.0 - first.0) / 2.0,
                (popover.tail.apex.1 - first.1).abs(),
            )
        };
        close(half, value(7.0), &format!("{case}: the tail's half width"));
        close(length, value(8.0), &format!("{case}: the tail's length"));
    }
}

#[test]
fn a_quota_bar_fills_its_share_of_the_width_and_never_less_than_its_height() {
    for (percent, share) in [(0.0, None), (50.0, Some(0.5)), (130.0, Some(1.0))] {
        let planned = popover_plan(claude_with(vec![window(
            "weekly",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            percent,
        )]));
        let popover = planned.popover.expect("the popover is open");
        let (fill, bar) = popover
            .entries
            .iter()
            .find_map(|(item, rect)| match item {
                PopItem::Bar { fill, .. } => Some((*fill, *rect)),
                _ => None,
            })
            .expect("the quota bar is planned");
        let expected = share.map_or(bar.h, |share| bar.w * share);
        assert_eq!(
            fill,
            Some(expected),
            "{percent}% fills {expected} of {bar:?}"
        );
    }
}

#[test]
fn each_rows_ci_dot_wears_its_states_ink_and_is_a_ring_without_checks() {
    let rows = ["success", "failure", "pending", "none"]
        .into_iter()
        .enumerate()
        .map(|(index, ci)| {
            let mut row = pr(index as u64 + 1, "Fix it", "acme/webapp");
            row["ci"] = ci.into();
            row
        })
        .collect();
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &displays,
        &data(vec![pulls(rows)]),
        open_on(CellId::PullRequests),
        now(),
    )
    .expect("fits");
    let dots: Vec<(Color, bool)> = planned
        .popover
        .expect("the popover is open")
        .entries
        .into_iter()
        .filter_map(|(item, _)| match item {
            PopItem::Dot { ink, hollow } => Some((ink, hollow)),
            _ => None,
        })
        .collect();
    assert_eq!(
        dots,
        [
            (LIVE_GREEN, false),
            (TRIP_RED, false),
            (WARN_AMBER, false),
            ([89, 89, 89, 255], true),
        ]
    );
}
