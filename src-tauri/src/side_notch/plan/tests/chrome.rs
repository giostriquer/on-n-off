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
            assert_eq!(
                cell.label.size,
                crate::side_notch::model::pixel_aligned(17.0 * size.scale(), scale),
                "the figure is 17 pt at the size, on whole pixels"
            );
        }
    }
}

#[test]
fn the_cap_pin_sits_toward_the_screen_edge_and_is_twelve_points_at_the_size() {
    for edge in [Edge::Right, Edge::Left, Edge::Top, Edge::Bottom] {
        for (size, scale) in [(NotchSize::Standard, 1.0), (NotchSize::Large, 1.25)] {
            let case = format!("{edge:?} {size:?} {scale}x");
            let planned = opened_on(edge, size, scale);
            let (cap, pin) = (planned.cap.rect, planned.cap.pin);
            let (ear, thickness) = (planned.metrics.ear(), planned.metrics.thickness);
            let along = ear * 0.62;
            let toward = thickness * 0.62;
            let away = thickness - toward;
            let (x, y) = match edge {
                Edge::Right => (toward, along),
                Edge::Left => (away, along),
                Edge::Top => (along, away),
                Edge::Bottom => (along, toward),
            };
            close(pin.mid_x(), cap.x + x, &format!("{case}: across"));
            close(pin.mid_y(), cap.y + y, &format!("{case}: along"));
            let side = crate::side_notch::model::pixel_aligned(12.0 * size.scale(), scale);
            assert_eq!((pin.w, pin.h), (side, side), "{case}: its size");
        }
    }
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
fn the_cards_border_is_one_point_at_the_size_drawn_inside_its_edge() {
    for (edge, size, scale) in [
        (Edge::Right, NotchSize::Standard, 2.0),
        (Edge::Top, NotchSize::Large, 1.25),
        (Edge::Left, NotchSize::Compact, 1.0),
    ] {
        let case = format!("{edge:?} {size:?} {scale}x");
        let popover = opened_on(edge, size, scale)
            .popover
            .expect("the popover is open");
        let width = crate::side_notch::model::pixel_aligned(size.scale(), scale);
        let (card, border) = (popover.card, popover.border);
        assert_eq!(border.width, width, "{case}: its width");
        assert_eq!(
            border.rect,
            R::new(
                card.x + width / 2.0,
                card.y + width / 2.0,
                card.w - width,
                card.h - width
            ),
            "{case}: its stroke's centre line runs half its width inside the card"
        );
        assert_eq!(
            border.radius,
            popover.radius - width / 2.0,
            "{case}: its corners follow the card's"
        );
    }
}

#[test]
fn the_card_and_its_tail_fill_the_popover_on_every_edge() {
    for edge in [Edge::Right, Edge::Left, Edge::Top, Edge::Bottom] {
        let case = format!("{edge:?}");
        let popover = opened_on(edge, NotchSize::Standard, 1.0)
            .popover
            .expect("the popover is open");
        let (rect, card, apex) = (popover.rect, popover.card, popover.tail.apex);
        let tail = 8.0;
        let expected = match edge {
            Edge::Right => R::new(rect.x, rect.y, rect.w - tail, rect.h),
            Edge::Left => R::new(rect.x + tail, rect.y, rect.w - tail, rect.h),
            Edge::Top => R::new(rect.x, rect.y + tail, rect.w, rect.h - tail),
            Edge::Bottom => R::new(rect.x, rect.y, rect.w, rect.h - tail),
        };
        assert_eq!(
            card, expected,
            "{case}: the card is the popover less its tail"
        );
        assert_eq!(
            popover.clip, None,
            "{case}: a card that holds everything cuts nothing"
        );
        let reaches = match edge {
            Edge::Right => apex.0 == rect.x + rect.w,
            Edge::Left => apex.0 == rect.x,
            Edge::Top => apex.1 == rect.y,
            Edge::Bottom => apex.1 == rect.y + rect.h,
        };
        assert!(
            reaches,
            "{case}: the apex {apex:?} ends on the popover's side facing the rail, {rect:?}"
        );
    }
}

fn overflowing(edge: Edge, height: f64) -> PopoverPlan {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, height, 1.0)];
    let rail = data_for(
        NotchSettings { edge, ..settings() },
        vec![pulls(long_rows(25))],
    );
    plan(&displays, &rail, open_on(CellId::PullRequests), now())
        .expect("fits")
        .popover
        .expect("the popover is open")
}

#[test]
fn a_popover_taller_than_the_helpers_maximum_keeps_its_end_and_cuts_its_top() {
    for (edge, height, maximum) in [
        (Edge::Right, 1080.0, 560.0),
        (Edge::Top, 1080.0, 560.0),
        (Edge::Left, 400.0, 400.0 - 2.0 * 8.0),
    ] {
        let case = format!("{edge:?} on a {height} pt display");
        let popover = overflowing(edge, height);
        let card = popover.card;
        assert_eq!(card.h, maximum, "{case}: the card stops at the maximum");
        let clip = popover.clip.expect("the card clips what it cuts");
        assert_eq!(
            clip.rect,
            R::new(card.x + 1.0, card.y + 1.0, card.w - 2.0, card.h - 2.0),
            "{case}: inside the card's border"
        );
        let bottom = card.y + card.h;
        let footer = popover
            .entries
            .iter()
            .find_map(|(item, rect)| match item {
                PopItem::Text(Text { text, .. }) if text == "Open Pull requests" => Some(*rect),
                _ => None,
            })
            .expect("the footer is planned");
        close(
            footer.y + footer.h + 12.0,
            bottom,
            &format!("{case}: the footer keeps its place above the bottom padding"),
        );
        assert!(
            popover
                .entries
                .iter()
                .all(|(_, rect)| rect.y + rect.h > card.y && rect.y < bottom),
            "{case}: nothing wholly outside the card is planned"
        );
        assert!(
            !popover.entries.iter().any(|(item, _)| matches!(
                item,
                PopItem::Text(Text { text, .. }) if text == "Pull requests"
            )),
            "{case}: the header is cut with the top"
        );
        assert!(
            popover
                .zones
                .iter()
                .all(|(_, rect)| rect.y >= card.y && rect.y + rect.h <= bottom),
            "{case}: no zone answers outside the card"
        );
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

#[test]
fn marks_beside_text_centre_on_the_cap_band_of_the_size_the_text_is_drawn_at() {
    for (edge, size, scale) in [
        (Edge::Right, NotchSize::Compact, 1.5),
        (Edge::Bottom, NotchSize::Large, 1.0),
    ] {
        let case = format!("{edge:?} {size:?} {scale}x");
        let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, scale)];
        let rail = data_for(
            NotchSettings {
                edge,
                size,
                ..settings()
            },
            vec![pulls(vec![pr(7, "Fix it", "acme/webapp")])],
        );
        let popover = plan(&displays, &rail, open_on(CellId::PullRequests), now())
            .expect("fits")
            .popover
            .expect("the popover is open");
        let typeset = super::super::text::Typesetter::new(&FixedAdvance, scale);
        let text = |wanted: &str| {
            popover
                .entries
                .iter()
                .find_map(|(item, rect)| match item {
                    PopItem::Text(text) if text.text == wanted => Some((text.clone(), *rect)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{wanted:?} is planned"))
        };
        let mark = |wanted: fn(&PopItem) -> bool| {
            popover
                .entries
                .iter()
                .find_map(|(item, rect)| wanted(item).then_some(*rect))
                .expect("the mark is planned")
        };
        for (beside, mark) in [
            (
                "Pull requests",
                mark(|item| {
                    matches!(
                        item,
                        PopItem::Mark {
                            kind: MarkKind::PullRequests,
                            ..
                        }
                    )
                }),
            ),
            (
                "acme/webapp #7",
                mark(|item| matches!(item, PopItem::Dot { .. })),
            ),
            (
                "Open Pull requests",
                mark(|item| {
                    matches!(
                        item,
                        PopItem::Mark {
                            kind: MarkKind::OpenArrow,
                            ..
                        }
                    )
                }),
            ),
        ] {
            let (run, at) = text(beside);
            close(
                mark.mid_y(),
                at.y + typeset.cap_middle(run.size, run.weight),
                &format!("{case}: the mark beside {beside:?}"),
            );
        }
    }
}

#[test]
fn the_popover_keeps_the_shared_padding_inside_the_card_on_every_side() {
    for (edge, size, scale) in [
        (Edge::Right, NotchSize::Standard, 1.0),
        (Edge::Bottom, NotchSize::Compact, 1.5),
        (Edge::Left, NotchSize::Large, 1.25),
    ] {
        let case = format!("{edge:?} {size:?} {scale}x");
        let screen = display("d1", 0.0, 0.0, 1920.0, 1080.0, scale);
        let padding = crate::side_notch::model::popover_metrics(size, &screen).padding;
        let popover = opened_on(edge, size, scale)
            .popover
            .expect("the popover is open");
        let card = popover.card;
        let rects: Vec<R> = popover.entries.iter().map(|(_, rect)| *rect).collect();
        let left = rects.iter().map(|rect| rect.x).fold(f64::MAX, f64::min);
        let top = rects.iter().map(|rect| rect.y).fold(f64::MAX, f64::min);
        let right = rects
            .iter()
            .map(|rect| rect.x + rect.w)
            .fold(f64::MIN, f64::max);
        let bottom = rects
            .iter()
            .map(|rect| rect.bottom())
            .fold(f64::MIN, f64::max);
        close(left - card.x, padding, &format!("{case}: left"));
        close(top - card.y, padding, &format!("{case}: top"));
        close(card.x + card.w - right, padding, &format!("{case}: right"));
        close(card.bottom() - bottom, padding, &format!("{case}: bottom"));
    }
}
