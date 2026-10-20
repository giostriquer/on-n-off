use std::fmt;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};

use crate::dto::{LimitWindowDto, LimitWindowKind, Reading, WindowRole};

pub(super) const FIVE_HOURS: Duration = Duration::from_secs(5 * 60 * 60);
pub(super) const ONE_WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);

const WEEKLY_FROM: Duration = Duration::from_secs(24 * 60 * 60);
const SEPARATOR: &str = " · ";
const ALL_MODELS: &str = "all models";
const WEEKLY: &str = "Weekly";
const RESET_TOLERANCE_SECONDS: i64 = 2;
const RESET_DROP_POINTS: f64 = 0.5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Scope {
    AllModels,
    Model(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct QuotaWindow {
    pub(super) id: String,
    pub(super) scope: Scope,
    pub(super) role: Option<WindowRole>,
    pub(super) starts_on_first_message: bool,
    pub(super) length: Option<Duration>,
    pub(super) used_percent: f64,
    pub(super) reset: Option<ResetInstant>,
}

impl QuotaWindow {
    pub(super) fn idle(id: impl Into<String>, length: Duration) -> Self {
        Self {
            id: id.into(),
            scope: Scope::AllModels,
            role: None,
            starts_on_first_message: true,
            length: Some(length),
            used_percent: 0.0,
            reset: None,
        }
    }

    pub(super) fn into_dto(self) -> LimitWindowDto {
        let used_percent = in_range(self.used_percent);
        LimitWindowDto {
            kind: self.kind(),
            label: self.label(),
            used_percent,
            resets_at: self.reset.map(|reset| reset.to_string()),
            window_seconds: self.length.map(|length| length.as_secs()),
            observed_at: String::new(),
            role: self.role,
            starts_on_first_message: self.starts_on_first_message
                && used_percent == 0.0
                && self.reset.is_none(),
            id: self.id,
        }
    }

    fn kind(&self) -> LimitWindowKind {
        match (&self.scope, self.length) {
            (Scope::Model(_), _) => LimitWindowKind::Model,
            (Scope::AllModels, Some(length)) if length < WEEKLY_FROM => LimitWindowKind::Session,
            (Scope::AllModels, _) => LimitWindowKind::Weekly,
        }
    }

    fn label(&self) -> String {
        let scope = match &self.scope {
            Scope::AllModels => ALL_MODELS,
            Scope::Model(name) => name.as_str(),
        };
        format!("{}{SEPARATOR}{scope}", length_name(self.length))
    }
}

fn length_name(length: Option<Duration>) -> String {
    match length {
        Some(length) if length < WEEKLY_FROM => {
            let seconds = length.as_secs();
            if seconds >= 3600 && seconds.is_multiple_of(3600) {
                format!("{} hour", seconds / 3600)
            } else {
                format!("{} minute", seconds / 60)
            }
        }
        _ => WEEKLY.to_string(),
    }
}

pub(super) fn scope_of_label(label: &str) -> Option<Scope> {
    let (_, scope) = label.split_once(SEPARATOR)?;
    Some(if scope == ALL_MODELS {
        Scope::AllModels
    } else {
        Scope::Model(scope.to_string())
    })
}

fn in_range(used_percent: f64) -> f64 {
    if used_percent.is_finite() {
        used_percent.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

pub(super) fn keep_in_range(reading: &mut Reading) {
    for window in &mut reading.windows {
        window.used_percent = in_range(window.used_percent);
    }
    if let Some(share) = &mut reading.workspace_credits {
        share.used_percent = in_range(share.used_percent);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ResetInstant(DateTime<Utc>);

impl ResetInstant {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|at| Self(at.with_timezone(&Utc)))
    }

    pub(crate) fn of(window: &LimitWindowDto) -> Option<Self> {
        window.resets_at.as_deref().and_then(Self::parse)
    }

    pub(crate) fn at(self) -> DateTime<Utc> {
        self.0
    }

    pub(crate) fn passed_by(self, at: DateTime<Utc>) -> bool {
        self.0 <= at
    }

    pub(crate) fn same_cycle_as(self, other: Self) -> bool {
        (self.0 - other.0).num_seconds().abs() <= RESET_TOLERANCE_SECONDS
    }

    pub(crate) fn later_than(self, other: Self) -> bool {
        self > other && !self.same_cycle_as(other)
    }
}

impl From<DateTime<Utc>> for ResetInstant {
    fn from(at: DateTime<Utc>) -> Self {
        Self(at)
    }
}

impl fmt::Display for ResetInstant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.to_rfc3339_opts(SecondsFormat::Millis, true))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Observation {
    pub(crate) used_percent: f64,
    pub(crate) reset: Option<ResetInstant>,
}

pub(crate) fn cycle_turned_over(
    before: Observation,
    after: Observation,
    observed_at: DateTime<Utc>,
) -> bool {
    let Some(saved) = before.reset else {
        return false;
    };
    let later = after.reset.is_some_and(|reset| reset.later_than(saved));
    (later || saved.passed_by(observed_at))
        && before.used_percent - after.used_percent > RESET_DROP_POINTS
}

#[cfg(test)]
pub(super) fn fixture(
    id: &str,
    label: &str,
    kind: LimitWindowKind,
    used_percent: f64,
    resets_at: Option<String>,
) -> LimitWindowDto {
    LimitWindowDto {
        id: id.into(),
        label: label.into(),
        kind,
        used_percent,
        resets_at,
        window_seconds: None,
        observed_at: String::new(),
        role: None,
        starts_on_first_message: false,
    }
}

#[cfg(test)]
mod tests;
