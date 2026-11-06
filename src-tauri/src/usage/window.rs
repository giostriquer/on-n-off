use chrono::{DateTime, NaiveDate, NaiveTime, SecondsFormat, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;

use crate::dto::{AdapterError, UsageSummaryInput};

const HOUR_MS: i64 = 60 * 60 * 1000;
const MAX_HOURLY_MS: i64 = 24 * HOUR_MS;
const MTIME_ALLOWANCE_MS: i64 = 36 * HOUR_MS;
const DAY_SEARCH_MS: i64 = 48 * HOUR_MS;
const HOURLY_GROUP: &str = "hour";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Period {
    day: NaiveDate,
    hour_start: Option<DateTime<Utc>>,
}

impl Period {
    pub fn day(self) -> String {
        day_text(self.day)
    }

    pub fn hour_start(self) -> Option<String> {
        self.hour_start
            .map(|at| at.to_rfc3339_opts(SecondsFormat::Millis, true))
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
    since_day: NaiveDate,
    until_day: NaiveDate,
    time_zone: String,
    zone: Tz,
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
        let zone: Tz = input.time_zone.parse().unwrap_or(chrono_tz::UTC);
        let span = match resolution {
            Resolution::Hour => hours(input)?,
            Resolution::Day => {
                let after_until = until_day
                    .succ_opt()
                    .ok_or_else(|| not_a_day("untilDay", &input.until_day))?;
                Span {
                    start_ms: first_instant(since_day, zone),
                    end_ms: first_instant(after_until, zone),
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
                .and_time(NaiveTime::MIN)
                .and_utc()
                .timestamp_millis(),
        };
        opening_ms.saturating_sub(MTIME_ALLOWANCE_MS)
    }

    pub fn period_of(&self, at_ms: i64) -> Option<Period> {
        let Span { start_ms, end_ms } = self.span;
        if !(start_ms..end_ms).contains(&at_ms) {
            return None;
        }
        let at = Utc.timestamp_millis_opt(at_ms).single()?;
        Some(Period {
            day: at.with_timezone(&self.zone).date_naive(),
            hour_start: (self.resolution == Resolution::Hour)
                .then(|| at - TimeDelta::milliseconds((at_ms - start_ms) % HOUR_MS)),
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

fn day(field: &str, text: &str) -> Result<NaiveDate, AdapterError> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
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
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|at| at.timestamp_millis())
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

fn first_instant(day: NaiveDate, zone: Tz) -> i64 {
    let midnight = day.and_time(NaiveTime::MIN);
    if let Some(at) = zone.from_local_datetime(&midnight).earliest() {
        return at.timestamp_millis();
    }
    let naive_ms = midnight.and_utc().timestamp_millis();
    let (mut before, mut from) = (naive_ms - DAY_SEARCH_MS, naive_ms + DAY_SEARCH_MS);
    while from - before > 1 {
        let probe = before + (from - before) / 2;
        if local_day(probe, zone).is_some_and(|local| local >= day) {
            from = probe;
        } else {
            before = probe;
        }
    }
    from
}

fn local_day(at_ms: i64, zone: Tz) -> Option<NaiveDate> {
    Utc.timestamp_millis_opt(at_ms)
        .single()
        .map(|at| at.with_timezone(&zone).date_naive())
}

fn day_text(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests;
