use super::*;
use crate::dto::{AgentId, LimitWindowKind};
use crate::side_notch::model::frame::{CellData, PrSegment, ProviderCell, RailData};
use crate::side_notch::model::{Display, NotchSettings, ShowMode};
use crate::side_notch::plan::tests::{
    claude_with, data, display, now, session_only, settings, tagged_rows, window, PERSONAL, WORK,
};
use crate::side_notch::plan::Hover;
use chrono::{DateTime, Utc};

mod render;

fn plan(
    settings: &NotchSettings,
    displays: &[Display],
    data: &RailData,
    hover: Hover,
    now: DateTime<Utc>,
) -> Option<Plan> {
    crate::side_notch::plan::plan(settings, displays, data, hover, now, &DirectWrite)
}

fn popover_render(provider: ProviderCell) -> (Plan, tiny_skia::Pixmap) {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![CellData::Provider(provider)]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
        now(),
    )
    .expect("fits");
    let pixmap = render(&planned);
    (planned, pixmap)
}
fn column_ink(pixmap: &tiny_skia::Pixmap, x: u32, y0: u32, y1: u32) -> u32 {
    (y0..y1)
        .map(|y| u32::from(pixmap.pixel(x, y).map_or(0, |px| px.red())))
        .sum()
}
fn row_ink(pixmap: &tiny_skia::Pixmap, y: u32, x0: u32, x1: u32) -> u32 {
    (x0..x1)
        .map(|x| u32::from(pixmap.pixel(x, y).map_or(0, |px| px.alpha())))
        .sum()
}
