use std::fmt::Display;

use chrono::{DateTime, TimeZone, Utc};

use super::super::window::ResetInstant;
use super::{readable, text_of};
use crate::dto::{LimitWindowDto, LimitsWorkspaceCreditsDto};

const HARDENS_FROM: f64 = 70.0;
const TRIPPED: f64 = 90.0;
const STARTS_ON_FIRST_MESSAGE: &str = "Starts with first message";
const RESET_UNKNOWN: &str = "Reset time unavailable";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Meter {
    Unreadable,
    Base,
    Blend(f64),
    Trip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cycle {
    Pending(ResetInstant),
    Renewed(ResetInstant),
    Idle,
    Unknown,
}

impl Cycle {
    fn of(reset: Option<ResetInstant>, starts_on_first_message: bool, now: DateTime<Utc>) -> Self {
        match reset {
            Some(reset) if reset.passed_by(now) => Self::Renewed(reset),
            Some(reset) => Self::Pending(reset),
            None if starts_on_first_message => Self::Idle,
            None => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Subject {
    Window,
    Share,
}

impl Subject {
    fn when<Tz: TimeZone>(self, reset: ResetInstant, zone: &Tz) -> String
    where
        Tz::Offset: Display,
    {
        let local = reset.at().with_timezone(zone);
        match self {
            Self::Window => local.format("%a %H:%M"),
            Self::Share => local.format("%b %-d"),
        }
        .to_string()
    }

    fn unknown_reset(self) -> &'static str {
        match self {
            Self::Window => RESET_UNKNOWN,
            Self::Share => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowAt {
    used_now: Option<f64>,
    cycle: Cycle,
    subject: Subject,
}

impl WindowAt {
    pub(crate) fn of(window: &LimitWindowDto, now: DateTime<Utc>) -> Self {
        Self::new(
            window.used_percent,
            Cycle::of(
                ResetInstant::of(window),
                window.starts_on_first_message,
                now,
            ),
            Subject::Window,
        )
    }

    pub(crate) fn of_share(share: &LimitsWorkspaceCreditsDto, now: DateTime<Utc>) -> Self {
        let reset = share.resets_at.as_deref().and_then(ResetInstant::parse);
        Self::new(
            share.used_percent,
            Cycle::of(reset, false, now),
            Subject::Share,
        )
    }

    fn new(used_percent: f64, cycle: Cycle, subject: Subject) -> Self {
        let renewed = matches!(cycle, Cycle::Renewed(_));
        Self {
            used_now: readable(used_percent).map(|used| if renewed { 0.0 } else { used }),
            cycle,
            subject,
        }
    }

    pub(crate) fn used_now(&self) -> Option<f64> {
        self.used_now
    }

    pub(crate) fn renewed(&self) -> bool {
        matches!(self.cycle, Cycle::Renewed(_))
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
        match self.cycle {
            Cycle::Pending(reset) => format!("Resets {}", self.subject.when(reset, zone)),
            Cycle::Renewed(reset) => format!("Reset {}", self.subject.when(reset, zone)),
            Cycle::Idle => STARTS_ON_FIRST_MESSAGE.into(),
            Cycle::Unknown => self.subject.unknown_reset().into(),
        }
    }
}

#[cfg(test)]
mod tests;
