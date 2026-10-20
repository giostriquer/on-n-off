#[cfg(any(target_os = "windows", test))]
use std::fmt::Display;

#[cfg(any(target_os = "windows", test))]
use chrono::{DateTime, TimeZone, Utc};

#[cfg(any(target_os = "windows", test))]
use super::window::{unstarted, ResetInstant};
#[cfg(any(target_os = "windows", test))]
use crate::dto::{LimitWindowDto, LimitsWorkspaceCreditsDto};

const REACHED: f64 = 100.0;
const LAST_UNREACHED: u8 = 99;
const UNREADABLE: &str = "—";
#[cfg(any(target_os = "windows", test))]
const HARDENS_FROM: f64 = 70.0;
#[cfg(any(target_os = "windows", test))]
const TRIPPED: f64 = 90.0;
#[cfg(any(target_os = "windows", test))]
const STARTS_ON_FIRST_MESSAGE: &str = "Starts with first message";
#[cfg(any(target_os = "windows", test))]
const RESET_UNKNOWN: &str = "Reset time unavailable";

fn readable(used_percent: f64) -> Option<f64> {
    (used_percent.is_finite() && (0.0..=REACHED).contains(&used_percent)).then_some(used_percent)
}

pub(crate) fn reached(used_percent: f64) -> bool {
    readable(used_percent).is_some_and(|used| used >= REACHED)
}

pub(crate) fn used_text(used_percent: f64) -> String {
    text_of(readable(used_percent))
}

fn text_of(used_now: Option<f64>) -> String {
    match used_now {
        None => UNREADABLE.into(),
        Some(used) if used >= REACHED => "100%".into(),
        Some(used) if used > 0.0 && used < 1.0 => "<1%".into(),
        Some(used) => format!("{}%", (used.round() as u8).min(LAST_UNREACHED)),
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Meter {
    Unreadable,
    Base,
    Blend(f64),
    Trip,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Subject {
    Window,
    Share,
}

#[cfg(any(target_os = "windows", test))]
impl Subject {
    fn unknown_reset(self) -> &'static str {
        match self {
            Self::Window => RESET_UNKNOWN,
            Self::Share => "",
        }
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoteKind {
    Pending,
    Elapsed,
    Idle,
    Unknown,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowAt {
    used_now: Option<f64>,
    reset: Option<ResetInstant>,
    renewed: bool,
    idle: bool,
    subject: Subject,
}

#[cfg(any(target_os = "windows", test))]
impl WindowAt {
    pub(crate) fn of(window: &LimitWindowDto, now: DateTime<Utc>) -> Self {
        Self::new(
            window.used_percent,
            ResetInstant::of(window),
            window.starts_on_first_message,
            Subject::Window,
            now,
        )
    }

    pub(crate) fn of_share(share: &LimitsWorkspaceCreditsDto, now: DateTime<Utc>) -> Self {
        Self::new(
            share.used_percent,
            share.resets_at.as_deref().and_then(ResetInstant::parse),
            false,
            Subject::Share,
            now,
        )
    }

    fn new(
        used_percent: f64,
        reset: Option<ResetInstant>,
        starts_on_first_message: bool,
        subject: Subject,
        now: DateTime<Utc>,
    ) -> Self {
        let renewed = reset.is_some_and(|reset| reset.passed_by(now));
        let used_now = readable(used_percent).map(|used| if renewed { 0.0 } else { used });
        Self {
            used_now,
            reset,
            renewed,
            idle: starts_on_first_message
                && used_now.is_some_and(|used| unstarted(used, reset.is_some())),
            subject,
        }
    }

    pub(crate) fn used_now(&self) -> Option<f64> {
        self.used_now
    }

    pub(crate) fn renewed(&self) -> bool {
        self.renewed
    }

    pub(crate) fn used_text(&self) -> String {
        text_of(self.used_now)
    }

    pub(crate) fn meter(&self) -> Meter {
        match self.used_now {
            None => Meter::Unreadable,
            Some(used) if used >= TRIPPED => Meter::Trip,
            Some(used) if used > HARDENS_FROM => {
                Meter::Blend(((used - HARDENS_FROM) / (TRIPPED - HARDENS_FROM)).sqrt())
            }
            Some(_) => Meter::Base,
        }
    }

    pub(crate) fn notch_note<Tz: TimeZone>(&self, zone: &Tz) -> String
    where
        Tz::Offset: Display,
    {
        let at = self.reset_at(zone).unwrap_or_default();
        match self.note_kind() {
            NoteKind::Pending => format!("Resets {at}"),
            NoteKind::Elapsed => format!("Reset {at}"),
            NoteKind::Idle => STARTS_ON_FIRST_MESSAGE.into(),
            NoteKind::Unknown => self.subject.unknown_reset().into(),
        }
    }

    fn note_kind(&self) -> NoteKind {
        match self.reset {
            Some(_) if self.renewed => NoteKind::Elapsed,
            Some(_) => NoteKind::Pending,
            None if self.idle => NoteKind::Idle,
            None => NoteKind::Unknown,
        }
    }

    fn reset_at<Tz: TimeZone>(&self, zone: &Tz) -> Option<String>
    where
        Tz::Offset: Display,
    {
        let local = self.reset?.at().with_timezone(zone);
        Some(
            match self.subject {
                Subject::Window => local.format("%a %H:%M"),
                Subject::Share => local.format("%b %-d"),
            }
            .to_string(),
        )
    }
}

#[cfg(test)]
mod tests;
