#![allow(clippy::too_many_arguments)]

mod marks;
mod text;

pub(super) use text::DirectWrite;

use super::model::{meter_color, Color, Edge, InnerRing, RailMetrics, TRIP_RED, UNREADABLE_INK};
use super::plan::{
    provider_color, CapPlan, CellContent, CellPlan, MarkKind, Plan, PopItem, PopoverPlan,
    TextMetrics, TextWeight, LIVE_GREEN, MUTED_INK, R, TAIL_LENGTH, WARN_AMBER,
};
use crate::dto::CiState;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

fn inner_ring_colors(ring: &InnerRing) -> (Color, Color) {
    match ring {
        InnerRing::Fable { .. } => (FABLE_ORANGE, FABLE_TRACK),
        InnerRing::WorkspaceShare => (CREDITS_INK, CREDITS_TRACK),
    }
}

const RAIL_INK: Color = [8, 8, 8, 255];
const POPOVER_INK: Color = [14, 14, 17, 255];
const TRACK_INK: Color = [44, 44, 44, 255];
const FABLE_ORANGE: Color = [204, 98, 64, 255];
const FABLE_TRACK: Color = [53, 42, 38, 255];
const CREDITS_INK: Color = [168, 176, 186, 255];
const CREDITS_TRACK: Color = [38, 41, 45, 255];
const CAP_HIGHLIGHT: Color = [255, 255, 255, 28];

fn ci_color(ci: CiState) -> Color {
    match ci {
        CiState::Success => LIVE_GREEN,
        CiState::Failure | CiState::Error => TRIP_RED,
        CiState::Pending => WARN_AMBER,
        CiState::None => UNREADABLE_INK,
    }
}

pub fn render(plan: &Plan) -> Pixmap {
    let scale = plan.display_scale as f32;
    let width = ((plan.window.w * plan.display_scale).round() as u32).max(1);
    let height = ((plan.window.h * plan.display_scale).round() as u32).max(1);
    let mut pixmap = Pixmap::new(width, height).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
    pixmap.fill(tiny_skia::Color::TRANSPARENT);

    if let Some(pill) = plan.pill {
        draw_pill(&mut pixmap, &pill, scale);
        return pixmap;
    }

    draw_silhouette_masked(
        &mut pixmap,
        &plan.rail,
        plan.edge,
        &plan.metrics,
        scale,
        RAIL_INK,
        None,
    );
    draw_cap(
        &mut pixmap,
        &plan.cap,
        plan.edge,
        &plan.metrics,
        &plan.rail,
        scale,
    );
    for cell in &plan.cells {
        draw_cell(&mut pixmap, cell, plan, scale);
    }
    if let Some(popover) = &plan.popover {
        draw_popover(&mut pixmap, popover, plan, scale);
    }
    pixmap
}

fn paint_solid(pixmap: &mut Pixmap, path: &Path, color: Color, even_odd: bool) {
    paint_solid_masked(pixmap, path, color, even_odd, None);
}

fn paint_solid_masked(
    pixmap: &mut Pixmap,
    path: &Path,
    color: Color,
    even_odd: bool,
    mask: Option<&tiny_skia::Mask>,
) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    pixmap.fill_path(
        path,
        &paint,
        if even_odd {
            FillRule::EvenOdd
        } else {
            FillRule::Winding
        },
        Transform::identity(),
        mask,
    );
}

use tiny_skia::Path;

fn draw_silhouette_masked(
    pixmap: &mut Pixmap,
    rail: &R,
    edge: Edge,
    metrics: &RailMetrics,
    scale: f32,
    color: Color,
    mask: Option<&tiny_skia::Mask>,
) {
    let t = if edge.is_vertical() { rail.w } else { rail.h } as f32;
    let length = if edge.is_vertical() { rail.h } else { rail.w } as f32;
    let ear = (metrics.ear() as f32).min(length / 2.0);
    let edge_x = t + 1.0;
    let mut pb = PathBuilder::new();
    pb.move_to(edge_x, 0.0);
    pb.cubic_to(t, ear * 0.39, t * 0.76, ear * 0.53, t * 0.45, ear * 0.5);
    pb.cubic_to(t * 0.16, ear * 0.46, 0.0, ear * 0.69, 0.0, ear);
    pb.line_to(0.0, length - ear);
    pb.cubic_to(
        0.0,
        length - ear * 0.69,
        t * 0.16,
        length - ear * 0.46,
        t * 0.45,
        length - ear * 0.5,
    );
    pb.cubic_to(
        t * 0.76,
        length - ear * 0.53,
        t,
        length - ear * 0.39,
        edge_x,
        length,
    );
    pb.close();
    let Some(local) = pb.finish() else {
        return;
    };
    let place = match edge {
        Edge::Right => Transform::from_translate(rail.x as f32, rail.y as f32),
        Edge::Left => Transform::from_row(-1.0, 0.0, 0.0, 1.0, rail.x as f32 + t, rail.y as f32),
        Edge::Top => Transform::from_row(0.0, -1.0, 1.0, 0.0, rail.x as f32, rail.y as f32 + t),
        Edge::Bottom => Transform::from_row(0.0, 1.0, 1.0, 0.0, rail.x as f32, rail.y as f32),
    };
    let scaled = Transform::from_scale(scale, scale).pre_concat(place);
    if let Some(path) = local.transform(scaled) {
        paint_solid_masked(pixmap, &path, color, false, mask);
    }
}

fn draw_pill(pixmap: &mut Pixmap, pill: &R, scale: f32) {
    let bg = PathBuilder::from_rect(
        Rect::from_xywh(
            pill.x as f32 * scale,
            pill.y as f32 * scale,
            pill.w as f32 * scale,
            pill.h as f32 * scale,
        )
        .unwrap(),
    );
    paint_solid(pixmap, &bg, [0, 0, 0, 1], false);
    let inset = 1.0;
    let (cx, cy) = (pill.mid_x() as f32 * scale, pill.mid_y() as f32 * scale);
    let vertical = pill.w < pill.h;
    let (rx, ry) = if vertical {
        (
            pill.w as f32 * scale / 2.0 - inset,
            pill.h as f32 * scale / 2.0,
        )
    } else {
        (
            pill.w as f32 * scale / 2.0,
            pill.h as f32 * scale / 2.0 - inset,
        )
    };
    if let Some(path) = capsule_px(cx - rx, cy - ry, rx * 2.0, ry * 2.0) {
        paint_solid(pixmap, &path, [255, 255, 255, 82], false);
    }
}

fn draw_cap(
    pixmap: &mut Pixmap,
    cap: &CapPlan,
    edge: Edge,
    metrics: &RailMetrics,
    rail: &R,
    scale: f32,
) {
    if !cap.hovered {
        return;
    }
    if let Some(mut mask) = tiny_skia::Mask::new(pixmap.width(), pixmap.height()) {
        let cap_px = Rect::from_xywh(
            cap.rect.x as f32 * scale,
            cap.rect.y as f32 * scale,
            cap.rect.w as f32 * scale,
            cap.rect.h as f32 * scale,
        )
        .unwrap();
        mask.fill_path(
            &PathBuilder::from_rect(cap_px),
            FillRule::Winding,
            false,
            Transform::identity(),
        );
        draw_silhouette_masked(
            pixmap,
            rail,
            edge,
            metrics,
            scale,
            CAP_HIGHLIGHT,
            Some(&mask),
        );
    }
    marks::pin(
        (
            (cap.rect.x + cap.rect.w * 0.62 - 6.0) as f32 * scale,
            (cap.rect.y + cap.rect.h * 0.62 - 6.0) as f32 * scale,
            12.0 * scale,
            12.0 * scale,
        ),
        if cap.pinned {
            [255, 255, 255, 242]
        } else {
            [255, 255, 255, 140]
        },
        pixmap,
    );
}

fn capsule_px(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    rounded_rect_path(
        R::new(f64::from(x), f64::from(y), f64::from(w), f64::from(h)),
        f64::from(w.min(h)) / 2.0,
        1.0,
    )
}

fn rounded_rect_path(rect: R, radius: f64, scale: f32) -> Option<Path> {
    let x = rect.x as f32 * scale;
    let y = rect.y as f32 * scale;
    let w = rect.w as f32 * scale;
    let h = rect.h as f32 * scale;
    let r = (radius as f32 * scale).min(w / 2.0).min(h / 2.0);
    if r <= 0.0 || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn stroke_ring(
    pixmap: &mut Pixmap,
    cx: f32,
    cy: f32,
    radius: f32,
    stroke: f32,
    from_deg: f32,
    to_deg: f32,
    color: Color,
    round_caps: bool,
    conflict: bool,
) {
    let n = (((to_deg - from_deg).abs() / 5.0).ceil() as usize).max(2);
    let mut pb = PathBuilder::new();
    for step in 0..=n {
        let t = step as f32 / n as f32;
        let angle = (from_deg + (to_deg - from_deg) * t).to_radians();
        let x = cx + radius * angle.cos();
        let y = cy + radius * angle.sin();
        if step == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    if let Some(path) = pb.finish() {
        let stroke_style = Stroke {
            width: stroke,
            line_cap: if round_caps {
                tiny_skia::LineCap::Round
            } else {
                tiny_skia::LineCap::Butt
            },
            ..Default::default()
        };
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
        paint.anti_alias = true;
        paint.force_hq_pipeline = conflict || color == LIVE_GREEN;
        if conflict {
            let outer = radius + stroke / 2.0;
            let boundary = radius + stroke / 6.0;
            let blend = stroke * 0.18;
            let green =
                tiny_skia::Color::from_rgba8(LIVE_GREEN[0], LIVE_GREEN[1], LIVE_GREEN[2], 255);
            let red = tiny_skia::Color::from_rgba8(TRIP_RED[0], TRIP_RED[1], TRIP_RED[2], 255);
            if let Some(shader) = tiny_skia::RadialGradient::new(
                tiny_skia::Point::from_xy(cx, cy),
                0.0,
                tiny_skia::Point::from_xy(cx, cy),
                outer,
                vec![
                    tiny_skia::GradientStop::new(0.0, green),
                    tiny_skia::GradientStop::new((boundary - blend) / outer, green),
                    tiny_skia::GradientStop::new((boundary + blend) / outer, red),
                    tiny_skia::GradientStop::new(1.0, red),
                ],
                tiny_skia::SpreadMode::Pad,
                Transform::identity(),
            ) {
                paint.shader = shader;
            }
        }
        if let Some(stroked) = path.stroke(&stroke_style, 1.0) {
            pixmap.fill_path(
                &stroked,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

fn draw_cell(pixmap: &mut Pixmap, cell: &CellPlan, plan: &Plan, scale: f32) {
    let metrics = &plan.metrics.cell;
    let icon_slot = metrics.icon_slot as f32 * scale;
    let ring_stroke = metrics.ring_stroke as f32 * scale;
    let center = (
        cell.rect.mid_x() as f32 * scale,
        cell.rect.y as f32 * scale + metrics.padding as f32 * scale + icon_slot / 2.0,
    );
    let radius = (icon_slot - ring_stroke) / 2.0;

    stroke_ring(
        pixmap,
        center.0,
        center.1,
        radius,
        ring_stroke,
        0.0,
        360.0,
        TRACK_INK,
        true,
        false,
    );

    let glyph_size = metrics.glyph as f32 * scale;
    let glyph_rect = (
        center.0 - glyph_size / 2.0,
        center.1 - glyph_size / 2.0,
        glyph_size,
        glyph_size,
    );

    let (label, offset) = match &cell.content {
        CellContent::Provider {
            provider,
            headline,
            inner,
            label,
        } => {
            if let Some(headline) = headline {
                let percent = headline.used_now().unwrap_or(0.0);
                stroke_ring(
                    pixmap,
                    center.0,
                    center.1,
                    radius,
                    ring_stroke,
                    -90.0,
                    -90.0 + percent.clamp(0.0, 100.0) as f32 * 3.6,
                    meter_color(headline.meter(), provider_color(*provider)),
                    true,
                    false,
                );
            }
            if let Some((ring, at)) = inner {
                let (ink, track) = inner_ring_colors(ring);
                let inner_percent = at.used_now().unwrap_or(0.0);
                let inner_radius = radius - metrics.inner_ring_inset as f32 * scale;
                stroke_ring(
                    pixmap,
                    center.0,
                    center.1,
                    inner_radius,
                    metrics.inner_ring_stroke as f32 * scale,
                    0.0,
                    360.0,
                    track,
                    false,
                    false,
                );
                stroke_ring(
                    pixmap,
                    center.0,
                    center.1,
                    inner_radius,
                    metrics.inner_ring_stroke as f32 * scale,
                    -90.0,
                    -90.0 + inner_percent.clamp(0.0, 100.0) as f32 * 3.6,
                    meter_color(at.meter(), ink),
                    true,
                    false,
                );
            }
            marks::provider(*provider, glyph_rect, [255, 255, 255, 255], pixmap);
            (label.clone(), if headline.is_some() { 2.0 } else { 0.0 })
        }
        CellContent::PullRequests { label, segments } => {
            if !segments.is_empty() {
                let span = 360.0 / segments.len() as f32;
                let gap_deg = if segments.len() > 1 {
                    0.025 * 360.0
                } else {
                    0.0
                };
                for (index, segment) in segments.iter().enumerate() {
                    stroke_ring(
                        pixmap,
                        center.0,
                        center.1,
                        radius,
                        ring_stroke,
                        -90.0 + index as f32 * span + gap_deg / 2.0,
                        -90.0 + (index + 1) as f32 * span - gap_deg / 2.0,
                        ci_color(segment.ci),
                        false,
                        segment.passing_with_conflicts,
                    );
                }
            }
            marks::pull_request(glyph_rect, 1.6 * scale, [255, 255, 255, 255], pixmap);
            (label.clone(), 0.0)
        }
    };

    let label_size = 17.0 * plan.size_scale as f32 * scale;
    let baseline = cell.rect.y as f32 * scale
        + metrics.padding as f32 * scale
        + icon_slot
        + metrics.content_spacing as f32 * scale
        + text::ascent_px(label_size, TextWeight::Medium);
    let measured = DirectWrite.measure_px(&label, label_size, TextWeight::Medium);
    text::draw_px(
        pixmap,
        center.0 - measured / 2.0 + offset,
        baseline,
        &label,
        label_size,
        TextWeight::Medium,
        [255, 255, 255, 255],
    );
}

fn draw_popover(pixmap: &mut Pixmap, popover: &PopoverPlan, plan: &Plan, scale: f32) {
    let edge = plan.edge;
    let card = &popover.card;
    let tail_space = if edge.is_vertical() {
        TAIL_LENGTH * plan.size_scale
    } else {
        0.0
    };
    let tail_space_y = if edge.is_vertical() {
        0.0
    } else {
        TAIL_LENGTH * plan.size_scale
    };

    if let Some(path) = rounded_rect_path(*card, 12.0, scale) {
        paint_solid(pixmap, &path, POPOVER_INK, false);
        let mut paint = Paint::default();
        paint.set_color_rgba8(255, 255, 255, 28);
        paint.anti_alias = true;
        if let Some(stroked) = path.stroke(
            &Stroke {
                width: 1.0,
                ..Default::default()
            },
            1.0,
        ) {
            pixmap.fill_path(
                &stroked,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    let half = (7.0 * plan.size_scale) as f32 * scale;
    let length = tail_space.max(tail_space_y) as f32 * scale;
    let along = if edge.is_vertical() {
        (card.y + popover.tail) as f32 * scale
    } else {
        (card.x + popover.tail) as f32 * scale
    };
    let (tx, ty) = match edge {
        Edge::Right => ((card.x + card.w) as f32 * scale, along),
        Edge::Left => (card.x as f32 * scale, along),
        Edge::Top => (along, card.y as f32 * scale),
        Edge::Bottom => (along, (card.y + card.h) as f32 * scale),
    };
    let mut tail_pb = PathBuilder::new();
    match edge {
        Edge::Right => {
            tail_pb.move_to(tx, ty - half);
            tail_pb.line_to(tx + length, ty);
            tail_pb.line_to(tx, ty + half);
        }
        Edge::Left => {
            tail_pb.move_to(tx, ty - half);
            tail_pb.line_to(tx - length, ty);
            tail_pb.line_to(tx, ty + half);
        }
        Edge::Top => {
            tail_pb.move_to(tx - half, ty);
            tail_pb.line_to(tx, ty - length);
            tail_pb.line_to(tx + half, ty);
        }
        Edge::Bottom => {
            tail_pb.move_to(tx - half, ty);
            tail_pb.line_to(tx, ty + length);
            tail_pb.line_to(tx + half, ty);
        }
    }
    tail_pb.close();
    if let Some(path) = tail_pb.finish() {
        paint_solid(pixmap, &path, POPOVER_INK, false);
    }

    for (item, rect) in &popover.entries {
        let px = R::new(
            rect.x * scale as f64,
            rect.y * scale as f64,
            rect.w * scale as f64,
            rect.h * scale as f64,
        );
        match item {
            PopItem::Mark { kind, size } => {
                let device = (
                    px.x as f32,
                    px.y as f32,
                    *size as f32 * scale,
                    *size as f32 * scale,
                );
                match kind {
                    MarkKind::Provider(provider) => {
                        marks::provider(*provider, device, [255, 255, 255, 255], pixmap)
                    }
                    MarkKind::PullRequests => {
                        marks::pull_request(device, 1.4 * scale, [255, 255, 255, 255], pixmap)
                    }
                    MarkKind::OpenArrow => {
                        marks::open_arrow(device, 1.2 * scale, MUTED_INK, pixmap)
                    }
                }
            }
            PopItem::Text {
                text,
                size,
                weight,
                color,
                right,
            } => {
                let device_size = *size as f32 * scale;
                let weight = *weight;
                let baseline = px.y as f32 + text::ascent_px(device_size, weight);
                if *right {
                    text::draw_px_right(
                        pixmap,
                        px.x as f32,
                        px.w as f32,
                        baseline,
                        text,
                        device_size,
                        weight,
                        *color,
                    );
                } else {
                    text::draw_px(
                        pixmap,
                        px.x as f32,
                        baseline,
                        text,
                        device_size,
                        weight,
                        *color,
                    );
                }
            }
            PopItem::Bar { percent, color } => {
                if let Some(path) = capsule_px(px.x as f32, px.y as f32, px.w as f32, px.h as f32) {
                    paint_solid(pixmap, &path, [255, 255, 255, 36], false);
                }
                if let Some(percent) = percent {
                    let fill_w = (px.w * percent.clamp(0.0, 100.0) / 100.0).max(px.h);
                    if let Some(path) =
                        capsule_px(px.x as f32, px.y as f32, fill_w as f32, px.h as f32)
                    {
                        paint_solid(pixmap, &path, *color, false);
                    }
                }
            }
            PopItem::Divider => {
                let divider = PathBuilder::from_rect(
                    Rect::from_xywh(px.x as f32, px.y as f32, px.w as f32, px.h as f32).unwrap(),
                );
                paint_solid(pixmap, &divider, [255, 255, 255, 26], false);
            }
            PopItem::Dot { ci } => {
                let mut dot_pb = PathBuilder::new();
                dot_pb.push_circle(px.mid_x() as f32, px.mid_y() as f32, px.w as f32 / 2.0);
                if let Some(path) = dot_pb.finish() {
                    if *ci == CiState::None {
                        marks::stroke(pixmap, &path, 1.0 * scale, [89, 89, 89, 255]);
                    } else {
                        paint_solid(pixmap, &path, ci_color(*ci), false);
                    }
                }
            }
            PopItem::CopyIcon { color } => {
                marks::copy_icon(
                    (px.x as f32, px.y as f32, px.w as f32, px.h as f32),
                    1.2 * scale,
                    *color,
                    pixmap,
                );
            }
            PopItem::Stripe { color } => {
                if let Some(path) = capsule_px(px.x as f32, px.y as f32, px.w as f32, px.h as f32) {
                    paint_solid(pixmap, &path, *color, false);
                }
            }
        }
    }
    let _ = tail_space_y;
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod visual;
