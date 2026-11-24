use std::sync::LazyLock;

use jiff::{
    civil::Date,
    fmt::temporal::DateTimePrinter,
    tz::{Offset, TimeZone},
    Error, Timestamp,
};
use regex::Regex;

pub(crate) fn parse(text: &str) -> Result<Timestamp, Error> {
    static RFC3339: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?x)\A [0-9]{4}-[0-9]{2}-[0-9]{2} [Tt\x20] [0-9]{2}:[0-9]{2}:[0-9]{2} (?:\.[0-9]+)? (?P<zone>[Zz]|[+-](?:[01][0-9]|2[0-3]):[0-5][0-9]) \z").unwrap()
    });
    let fields = RFC3339
        .captures(text)
        .ok_or_else(|| Error::from_args(format_args!("invalid RFC 3339 timestamp")))?;
    let zone = fields.name("zone").unwrap();
    let at: Timestamp = if zone.start() > 29 {
        format!("{}{}", &text[..29], zone.as_str()).parse()?
    } else {
        text.parse()?
    };
    if &text[17..19] == "60" {
        at.checked_add(jiff::SignedDuration::from_secs(1))
    } else {
        Ok(at)
    }
}

pub(crate) trait TimestampExt {
    fn epoch_millis(self) -> i64;
    fn epoch_seconds(self) -> i64;
    fn utc_date(self) -> Date;
    fn utc_offset_text(self) -> String;
    fn utc_seconds_text(self) -> String;
    fn utc_millis_text(self) -> String;
}

impl TimestampExt for Timestamp {
    fn epoch_millis(self) -> i64 {
        self.as_nanosecond().div_euclid(1_000_000) as i64
    }

    fn epoch_seconds(self) -> i64 {
        self.as_nanosecond().div_euclid(1_000_000_000) as i64
    }

    fn utc_date(self) -> Date {
        TimeZone::UTC.to_datetime(self).date()
    }

    fn utc_offset_text(self) -> String {
        let nanos = self.subsec_nanosecond();
        let precision = if nanos == 0 {
            0
        } else if nanos % 1_000_000 == 0 {
            3
        } else if nanos % 1000 == 0 {
            6
        } else {
            9
        };
        DateTimePrinter::new()
            .precision(Some(precision))
            .timestamp_with_offset_to_string(&self, Offset::UTC)
    }

    fn utc_seconds_text(self) -> String {
        format!("{self:.0}")
    }

    fn utc_millis_text(self) -> String {
        format!("{self:.3}")
    }
}
