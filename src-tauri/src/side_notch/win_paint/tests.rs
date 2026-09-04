use super::*;
use crate::side_notch::model::{Display, NotchSettings, ShowMode};

fn display(id: &str, x: f64, y: f64, width: f64, height: f64, scale: f64) -> Display {
    Display {
        id: id.into(),
        name: "Test Display".into(),
        x,
        y,
        width,
        height,
        work_y: y,
        work_height: height,
        scale,
        mirrored: false,
    }
}

fn provider_data(provider: AgentId, percent: f64) -> ProviderData {
    ProviderData {
        provider,
        status: LimitsStatus::Ok,
        message: None,
        windows: vec![LimitWindowDto {
            id: "w".into(),
            label: "Current session".into(),
            kind: LimitWindowKind::Session,
            used_percent: percent,
            resets_at: None,
            window_seconds: None,
            observed_at: "2026-09-01T10:00:00Z".into(),
        }],
        sessions: Vec::new(),
    }
}

fn settings() -> NotchSettings {
    NotchSettings {
        enabled: true,
        display_id: Some("d1".into()),
        ..NotchSettings::default()
    }
}

fn data(cells: Vec<CellData>) -> RailData {
    RailData {
        settings: settings(),
        cells,
        action_error: None,
    }
}

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
fn codex_hides_internal_windows_from_both_ring_and_popover() {
    let provider = ProviderData {
        provider: AgentId::Codex,
        status: LimitsStatus::Ok,
        message: None,
        windows: vec![
            LimitWindowDto {
                id: "w-session".into(),
                label: "Session".into(),
                kind: LimitWindowKind::Session,
                used_percent: 10.0,
                resets_at: None,
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
            },
            LimitWindowDto {
                id: "extra:base_model_inference".into(),
                label: "Weekly · gpt-5.3-codex-spark".into(),
                kind: LimitWindowKind::Model,
                used_percent: 90.0,
                resets_at: None,
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
            },
        ],
        sessions: Vec::new(),
    };
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![CellData::Provider(provider)]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
    )
    .unwrap();
    let popover = planned.popover.unwrap();
    let bars = popover
        .entries
        .iter()
        .filter(|(item, _)| matches!(item, PopItem::Bar { .. }))
        .count();
    assert_eq!(
        bars, 1,
        "the internal codex window never reaches the popover"
    );
}

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
fn unreadable_providers_fall_back_to_the_dash_label() {
    let mut provider = provider_data(AgentId::Cursor, 50.0);
    provider.status = LimitsStatus::Failed;
    provider.message = Some("Could not read usage.".into());
    let content = cell_content(&CellData::Provider(provider));
    match content {
        CellContent::Provider { label, primary, .. } => {
            assert_eq!(label, "—");
            assert!(primary.is_none());
        }
        _ => panic!("wrong content kind"),
    }
}
