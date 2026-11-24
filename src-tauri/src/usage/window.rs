use crate::timestamp::TimestampExt;
use jiff::civil::Date;
use jiff::tz::{AmbiguousOffset, TimeZone, TimeZoneDatabase};
use jiff::SignedDuration;
use jiff::Timestamp;

use crate::dto::{AdapterError, UsageSummaryInput};

const HOUR_MS: i64 = 60 * 60 * 1000;
const MAX_HOURLY_MS: i64 = 24 * HOUR_MS;
const MTIME_ALLOWANCE_MS: i64 = 36 * HOUR_MS;
const HOURLY_GROUP: &str = "hour";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Period {
    day: Date,
    hour_start: Option<Timestamp>,
}

impl Period {
    pub fn day(self) -> String {
        day_text(self.day)
    }

    pub fn hour_start(self) -> Option<String> {
        self.hour_start.map(|at| at.utc_millis_text())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheIdentity {
    key: String,
    group: String,
}

impl CacheIdentity {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn group(&self) -> &str {
        &self.group
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolution {
    Day,
    Hour,
}

#[derive(Debug, Clone)]
pub struct UsageWindow {
    since_day: Date,
    until_day: Date,
    time_zone: String,
    zone: TimeZone,
    resolution: Resolution,
    span: Span,
}

impl UsageWindow {
    pub fn of(input: &UsageSummaryInput) -> Result<Self, AdapterError> {
        let since_day = day("sinceDay", &input.since_day)?;
        let until_day = day("untilDay", &input.until_day)?;
        if since_day > until_day {
            return Err(AdapterError::message(format!(
                "sinceDay '{}' is after untilDay '{}'",
                input.since_day, input.until_day
            )));
        }
        let resolution = if input.resolution.as_deref() == Some("hour") {
            Resolution::Hour
        } else {
            Resolution::Day
        };
        let zone = TimeZoneDatabase::bundled()
            .get(&input.time_zone)
            .ok()
            .filter(|zone| zone.iana_name() == Some(input.time_zone.as_str()))
            .unwrap_or(TimeZone::UTC);
        let span = match resolution {
            Resolution::Hour => hours(input)?,
            Resolution::Day => {
                let after_until = until_day
                    .tomorrow()
                    .map_err(|_| not_a_day("untilDay", &input.until_day))?;
                Span {
                    start_ms: first_instant(since_day, &zone)
                        .map_err(|_| not_a_day("sinceDay", &input.since_day))?,
                    end_ms: first_instant(after_until, &zone)
                        .map_err(|_| not_a_day("untilDay", &input.until_day))?,
                }
            }
        };
        Ok(Self {
            since_day,
            until_day,
            time_zone: input.time_zone.clone(),
            zone,
            resolution,
            span,
        })
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn earliest_mtime_ms(&self) -> i64 {
        let opening_ms = match self.resolution {
            Resolution::Hour => self.span.start_ms,
            Resolution::Day => self
                .since_day
                .duration_since(jiff::civil::date(1970, 1, 1))
                .as_millis() as i64,
        };
        opening_ms.saturating_sub(MTIME_ALLOWANCE_MS)
    }

    pub fn period_of(&self, at_ms: i64) -> Option<Period> {
        let Span { start_ms, end_ms } = self.span;
        if !(start_ms..end_ms).contains(&at_ms) {
            return None;
        }
        let at = Timestamp::from_millisecond(at_ms).ok()?;
        Some(Period {
            day: self.zone.to_datetime(at).date(),
            hour_start: (self.resolution == Resolution::Hour)
                .then(|| at - SignedDuration::from_millis((at_ms - start_ms) % HOUR_MS)),
        })
    }

    pub fn cache_identity(&self) -> CacheIdentity {
        let since = day_text(self.since_day);
        let until = day_text(self.until_day);
        let zone = &self.time_zone;
        match self.resolution {
            Resolution::Hour => CacheIdentity {
                key: format!(
                    "hour|{since}|{until}|{}|{}|{zone}",
                    self.span.start_ms, self.span.end_ms
                ),
                group: HOURLY_GROUP.to_string(),
            },
            Resolution::Day => {
                let key = format!("day|{since}|{until}|{zone}");
                CacheIdentity {
                    group: key.clone(),
                    key,
                }
            }
        }
    }
}

fn day(field: &str, text: &str) -> Result<Date, AdapterError> {
    text.parse::<Date>()
        .ok()
        .filter(|day| text.len() == 10 && day_text(*day) == text)
        .ok_or_else(|| not_a_day(field, text))
}

fn not_a_day(field: &str, text: &str) -> AdapterError {
    AdapterError::message(format!("{field} '{text}' is not a day (YYYY-MM-DD)"))
}

fn hours(input: &UsageSummaryInput) -> Result<Span, AdapterError> {
    let instant = |text: &Option<String>| {
        text.as_deref()
            .and_then(|text| crate::timestamp::parse(text).ok())
            .map(|at| at.epoch_millis())
    };
    let (Some(since_ms), Some(until_ms)) = (instant(&input.since_time), instant(&input.until_time))
    else {
        return Err(AdapterError::message(
            "Hourly usage requires valid sinceTime and untilTime instants",
        ));
    };
    let duration = until_ms - since_ms;
    if duration <= 0 || duration > MAX_HOURLY_MS {
        return Err(AdapterError::message(
            "Hourly usage window must be greater than zero and at most 24 hours",
        ));
    }
    Ok(Span {
        start_ms: since_ms,
        end_ms: until_ms,
    })
}

fn first_instant(day: Date, zone: &TimeZone) -> Result<i64, jiff::Error> {
    let midnight = zone.to_ambiguous_timestamp(day.at(0, 0, 0, 0));
    let at = if matches!(midnight.offset(), AmbiguousOffset::Gap { .. }) {
        let before = midnight.earlier()?;
        zone.following(before)
            .next()
            .map_or(midnight.later()?, |transition| transition.timestamp())
    } else {
        midnight.earlier()?
    };
    Ok(at.as_millisecond())
}

fn day_text(day: Date) -> String {
    day.to_string()
}

#[cfg(test)]
mod tests;
