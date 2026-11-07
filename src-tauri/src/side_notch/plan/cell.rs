use super::text::{TextWeight, Typesetter};
use super::{provider_color, Arc, ArcInk, CellId, CellPlan, Color, Text, R};
use crate::dto::CiState;
use crate::limits::window_at::{WindowAt, UNREADABLE};
use crate::side_notch::model::frame::CellData;
use crate::side_notch::model::{meter_color, CellMetrics, InnerRing, TRIP_RED, UNREADABLE_INK};
use chrono::{DateTime, Utc};

pub(super) const TRACK_INK: Color = [44, 44, 44, 255];
pub(super) const FABLE_ORANGE: Color = [204, 98, 64, 255];
pub(super) const FABLE_TRACK: Color = [53, 42, 38, 255];
pub(super) const CREDITS_INK: Color = [168, 176, 186, 255];
pub(super) const CREDITS_TRACK: Color = [38, 41, 45, 255];
const WHITE: Color = [255, 255, 255, 255];
const SEGMENT_GAP: f64 = 9.0;

pub(super) fn ci_color(ci: CiState) -> Color {
    match ci {
        CiState::Success => super::LIVE_GREEN,
        CiState::Failure | CiState::Error => TRIP_RED,
        CiState::Pending => super::WARN_AMBER,
        CiState::None => UNREADABLE_INK,
    }
}

fn inner_ring_inks(ring: &InnerRing) -> (Color, Color) {
    match ring {
        InnerRing::Fable { .. } => (FABLE_ORANGE, FABLE_TRACK),
        InnerRing::WorkspaceShare => (CREDITS_INK, CREDITS_TRACK),
    }
}

struct Ring {
    centre: (f64, f64),
    radius: f64,
    stroke: f64,
}

impl Ring {
    fn arc(&self, from: f64, to: f64, ink: ArcInk, round: bool) -> Arc {
        Arc {
            centre: self.centre,
            radius: self.radius,
            stroke: self.stroke,
            from,
            to,
            ink,
            round,
        }
    }

    fn whole(&self, ink: Color, round: bool) -> Arc {
        self.arc(0.0, 360.0, ArcInk::Solid(ink), round)
    }

    fn meter(&self, at: &WindowAt, ink: Color) -> Arc {
        let percent = at.used_now().unwrap_or(0.0).clamp(0.0, 100.0);
        self.arc(
            -90.0,
            -90.0 + percent * 3.6,
            ArcInk::Solid(meter_color(at.meter(), ink)),
            true,
        )
    }
}

pub(super) fn cell(
    data: &CellData,
    rect: R,
    active: bool,
    metrics: &CellMetrics,
    typeset: &Typesetter,
    now: DateTime<Utc>,
) -> CellPlan {
    let centre = (
        rect.mid_x(),
        rect.y + metrics.padding + metrics.icon_slot / 2.0,
    );
    let ring = Ring {
        centre,
        radius: metrics.ring_radius(),
        stroke: metrics.ring_stroke,
    };
    let mut arcs = vec![ring.whole(TRACK_INK, true)];
    let (text, offset) = match data {
        CellData::Provider(provider) => {
            let reading = provider.reading.as_ref();
            let headline = reading
                .and_then(|reading| reading.headline())
                .map(|window| WindowAt::of(window, now));
            if let Some(at) = &headline {
                arcs.push(ring.meter(at, provider_color(provider.provider)));
            }
            if let Some((inner, at)) = reading.and_then(|reading| reading.inner_at(now)) {
                let (ink, track) = inner_ring_inks(inner);
                let inner_ring = Ring {
                    centre,
                    radius: metrics.inner_ring_radius(),
                    stroke: metrics.inner_ring_stroke,
                };
                arcs.push(inner_ring.whole(track, false));
                arcs.push(inner_ring.meter(&at, ink));
            }
            match headline {
                Some(at) => (at.used_text(), metrics.label_nudge),
                None => (UNREADABLE.into(), 0.0),
            }
        }
        CellData::PullRequests(pulls) => {
            let segments = pulls
                .reading
                .as_ref()
                .map(|reading| reading.segments.as_slice())
                .unwrap_or_default();
            let span = 360.0 / segments.len() as f64;
            let gap = if segments.len() > 1 { SEGMENT_GAP } else { 0.0 };
            for (index, segment) in segments.iter().enumerate() {
                arcs.push(ring.arc(
                    -90.0 + index as f64 * span + gap / 2.0,
                    -90.0 + (index + 1) as f64 * span - gap / 2.0,
                    if segment.passing_with_conflicts {
                        ArcInk::Conflict
                    } else {
                        ArcInk::Solid(ci_color(segment.ci))
                    },
                    false,
                ));
            }
            (pulls.label.clone(), 0.0)
        }
    };
    let size = metrics.label_size;
    let width = typeset.width(&text, size, TextWeight::Medium);
    CellPlan {
        id: CellId::of(data),
        rect,
        active,
        arcs,
        glyph: R::new(
            centre.0 - metrics.glyph / 2.0,
            centre.1 - metrics.glyph / 2.0,
            metrics.glyph,
            metrics.glyph,
        ),
        label: Text {
            text,
            size,
            weight: TextWeight::Medium,
            color: WHITE,
            right: false,
        },
        label_rect: R::new(
            centre.0 - width / 2.0 + offset,
            rect.y + metrics.padding + metrics.icon_slot + metrics.content_spacing,
            width,
            metrics.label_height,
        ),
    }
}
