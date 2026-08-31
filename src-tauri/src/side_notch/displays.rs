use super::model::Display;
use crate::process::{wait_with_deadline, CommandOutcome};
use core_graphics::display::CGDisplay;
use std::{
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

struct CachedDisplays {
    topology: String,
    displays: Vec<Display>,
    checked_at: Instant,
}
static CACHE: Mutex<Option<CachedDisplays>> = Mutex::new(None);

pub fn read() -> Result<Vec<Display>, String> {
    let mut ids =
        CGDisplay::active_displays().map_err(|error| format!("Cannot list displays: {error:?}"))?;
    ids.sort_unstable();
    let topology = ids
        .into_iter()
        .map(|id| {
            let display = CGDisplay::new(id);
            format!(
                "{id}:{:?}:{}:{}:{}",
                display.bounds(),
                display.pixels_wide(),
                display.pixels_high(),
                display.is_in_mirror_set()
            )
        })
        .collect::<Vec<_>>()
        .join(";");
    let mut cache = CACHE.lock().map_err(|_| "Display cache is unavailable.")?;
    if let Some(cached) = cache.as_ref() {
        if cached.topology == topology && cached.checked_at.elapsed() < Duration::from_secs(60) {
            return Ok(cached.displays.clone());
        }
    }
    let child = Command::new(super::transport::executable()?)
        .arg("--displays")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Cannot read display identities: {error}"))?;
    let body = match wait_with_deadline(child, Duration::from_secs(5))
        .map_err(|error| error.to_string())?
    {
        CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        } => stdout,
        CommandOutcome::Exited { stderr, .. } => {
            return Err(format!("Cannot read display identities: {}", stderr.trim()))
        }
        CommandOutcome::TimedOut => return Err("Reading display identities timed out.".into()),
    };
    let displays: Vec<Display> = serde_json::from_str(&body)
        .map_err(|error| format!("Invalid display information: {error}"))?;
    if displays.iter().any(|display| {
        display.id.is_empty()
            || ![
                display.x,
                display.y,
                display.width,
                display.height,
                display.work_y,
                display.work_height,
                display.scale,
            ]
            .iter()
            .all(|n| n.is_finite())
            || display.scale <= 0.0
    }) {
        return Err("macOS returned incomplete display information.".into());
    }
    *cache = Some(CachedDisplays {
        topology,
        displays: displays.clone(),
        checked_at: Instant::now(),
    });
    Ok(displays)
}
