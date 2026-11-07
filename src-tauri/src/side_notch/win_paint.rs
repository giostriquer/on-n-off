#![allow(clippy::too_many_arguments)]

mod marks;
mod text;

pub(super) use text::DirectWrite;

use super::model::{Color, Edge, RailMetrics, TRIP_RED};
use super::plan::{
    Arc, ArcInk, CapPlan, CellId, CellPlan, MarkKind, Plan, PopItem, PopoverPlan, Text, LIVE_GREEN,
    MUTED_INK, R,
};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

const RAIL_INK: Color = [8, 8, 8, 255];
const POPOVER_INK: Color = [14, 14, 17, 255];
const CAP_HIGHLIGHT: Color = [255, 255, 255, 28];
const WHITE: Color = [255, 255, 255, 255];

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
        draw_cell(&mut pixmap, cell, scale);
    }
    if let Some(popover) = &plan.popover {
        draw_popover(&mut pixmap, popover, scale);
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
    let ear = metrics.ear() as f32;
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
            cap.pin.x as f32 * scale,
            cap.pin.y as f32 * scale,
            cap.pin.w as f32 * scale,
            cap.pin.h as f32 * scale,
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

fn draw_arc(pixmap: &mut Pixmap, arc: &Arc, scale: f32) {
    let (cx, cy) = (arc.centre.0 as f32 * scale, arc.centre.1 as f32 * scale);
    let radius = arc.radius as f32 * scale;
    let stroke = arc.stroke as f32 * scale;
    let (from, to) = (arc.from as f32, arc.to as f32);
    let n = (((to - from).abs() / 5.0).ceil() as usize).max(2);
    let mut pb = PathBuilder::new();
    for step in 0..=n {
        let t = step as f32 / n as f32;
        let angle = (from + (to - from) * t).to_radians();
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
            line_cap: if arc.round {
                tiny_skia::LineCap::Round
            } else {
                tiny_skia::LineCap::Butt
            },
            ..Default::default()
        };
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        match arc.ink {
            ArcInk::Solid(color) => {
                paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
                paint.force_hq_pipeline = color == LIVE_GREEN;
            }
            ArcInk::Conflict => {
                paint.force_hq_pipeline = true;
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

fn draw_cell(pixmap: &mut Pixmap, cell: &CellPlan, scale: f32) {
    for arc in &cell.arcs {
        draw_arc(pixmap, arc, scale);
    }
    let glyph = (
        cell.glyph.x as f32 * scale,
        cell.glyph.y as f32 * scale,
        cell.glyph.w as f32 * scale,
        cell.glyph.h as f32 * scale,
    );
    match cell.id {
        CellId::Provider(provider) => marks::provider(provider, glyph, WHITE, pixmap),
        CellId::PullRequests => marks::pull_request(glyph, 1.6 * scale, WHITE, pixmap),
    }
    draw_text(pixmap, &cell.label, &cell.label_rect, scale);
}

fn draw_text(pixmap: &mut Pixmap, run: &Text, rect: &R, scale: f32) {
    let device_size = run.size as f32 * scale;
    let (x, y, w) = (
        (rect.x * f64::from(scale)) as f32,
        (rect.y * f64::from(scale)) as f32,
        (rect.w * f64::from(scale)) as f32,
    );
    let baseline = y + text::ascent_px(device_size, run.weight);
    if run.right {
        text::draw_px_right(
            pixmap,
            x,
            w,
            baseline,
            &run.text,
            device_size,
            run.weight,
            run.color,
        );
    } else {
        text::draw_px(
            pixmap,
            x,
            baseline,
            &run.text,
            device_size,
            run.weight,
            run.color,
        );
    }
}

fn draw_popover(pixmap: &mut Pixmap, popover: &PopoverPlan, scale: f32) {
    if let Some(path) = rounded_rect_path(popover.card, popover.radius, scale) {
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

    let point = |(x, y): (f64, f64)| (x as f32 * scale, y as f32 * scale);
    let ([first, last], apex) = (popover.tail.base.map(point), point(popover.tail.apex));
    let mut tail_pb = PathBuilder::new();
    tail_pb.move_to(first.0, first.1);
    tail_pb.line_to(apex.0, apex.1);
    tail_pb.line_to(last.0, last.1);
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
                        marks::provider(*provider, device, WHITE, pixmap)
                    }
                    MarkKind::PullRequests => {
                        marks::pull_request(device, 1.4 * scale, WHITE, pixmap)
                    }
                    MarkKind::OpenArrow => {
                        marks::open_arrow(device, 1.2 * scale, MUTED_INK, pixmap)
                    }
                }
            }
            PopItem::Text(text) => draw_text(pixmap, text, rect, scale),
            PopItem::Bar { fill, color } => {
                if let Some(path) = capsule_px(px.x as f32, px.y as f32, px.w as f32, px.h as f32) {
                    paint_solid(pixmap, &path, [255, 255, 255, 36], false);
                }
                if let Some(fill) = fill {
                    let fill_w = fill * f64::from(scale);
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
            PopItem::Dot { ink, hollow } => {
                let mut dot_pb = PathBuilder::new();
                dot_pb.push_circle(px.mid_x() as f32, px.mid_y() as f32, px.w as f32 / 2.0);
                if let Some(path) = dot_pb.finish() {
                    if *hollow {
                        marks::stroke(pixmap, &path, 1.0 * scale, *ink);
                    } else {
                        paint_solid(pixmap, &path, *ink, false);
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
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod visual;
