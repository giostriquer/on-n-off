#[cfg(any(target_os = "windows", test))]
mod instant;

#[cfg(any(target_os = "windows", test))]
pub(crate) use instant::{Meter, WindowAt};

const REACHED: f64 = 100.0;
const LAST_UNREACHED: u8 = 99;
const SLIVER: u8 = 1;
pub(crate) const UNREADABLE: &str = "—";

fn readable(used_percent: f64) -> Option<f64> {
    (used_percent.is_finite() && (0.0..=REACHED).contains(&used_percent)).then_some(used_percent)
}

fn is_sliver(used: f64) -> bool {
    used > 0.0 && used < 1.0
}

fn figure_of(used: f64) -> u8 {
    if used >= REACHED {
        100
    } else if is_sliver(used) {
        SLIVER
    } else {
        (used.round() as u8).min(LAST_UNREACHED)
    }
}

fn text_of(used_now: Option<f64>) -> String {
    match used_now {
        None => UNREADABLE.into(),
        Some(used) if is_sliver(used) => "<1%".into(),
        Some(used) => format!("{}%", figure_of(used)),
    }
}

pub(crate) fn reached(used_percent: f64) -> bool {
    readable(used_percent).is_some_and(|used| used >= REACHED)
}

pub(crate) fn used_text(used_percent: f64) -> String {
    text_of(readable(used_percent))
}

#[cfg(test)]
mod tests;
