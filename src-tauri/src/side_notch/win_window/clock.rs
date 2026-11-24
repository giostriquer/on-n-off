use jiff::{civil::DateTime, tz::TimeZone, Timestamp};
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::Time::{
    GetTimeZoneInformationForYear, SystemTimeToTzSpecificLocalTime, TIME_ZONE_INFORMATION,
};

use crate::side_notch::plan::ViewerClock;

pub(super) struct WindowsClock(pub Timestamp);

impl ViewerClock for WindowsClock {
    fn now(&self) -> Timestamp {
        self.0
    }

    fn local_at(&self, at: Timestamp) -> DateTime {
        let utc = TimeZone::UTC.to_datetime(at);
        let mut settings = TIME_ZONE_INFORMATION::default();
        let native = u16::try_from(utc.year()).ok().and_then(|year| {
            unsafe { GetTimeZoneInformationForYear(year, None, &mut settings) }.ok()?;
            local_time(utc, &settings)
        });
        native.unwrap_or_else(|| TimeZone::system().to_datetime(at))
    }
}

fn local_time(utc: DateTime, settings: &TIME_ZONE_INFORMATION) -> Option<DateTime> {
    let input = SYSTEMTIME {
        wYear: utc.year().try_into().ok()?,
        wMonth: utc.month() as u16,
        wDay: utc.day() as u16,
        wHour: utc.hour() as u16,
        wMinute: utc.minute() as u16,
        wSecond: utc.second() as u16,
        ..Default::default()
    };
    let mut local = SYSTEMTIME::default();
    unsafe { SystemTimeToTzSpecificLocalTime(Some(settings), &input, &mut local) }.ok()?;
    DateTime::new(
        local.wYear.try_into().ok()?,
        local.wMonth as i8,
        local.wDay as i8,
        local.wHour as i8,
        local.wMinute as i8,
        local.wSecond as i8,
        utc.subsec_nanosecond(),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summer_resets_follow_disabled_and_fixed_daylight_rules() {
        let standard_only = TIME_ZONE_INFORMATION {
            Bias: 480,
            DaylightBias: -60,
            ..Default::default()
        };
        let recurring = TIME_ZONE_INFORMATION {
            StandardDate: SYSTEMTIME {
                wMonth: 11,
                wDay: 1,
                wHour: 2,
                ..Default::default()
            },
            DaylightDate: SYSTEMTIME {
                wMonth: 3,
                wDay: 2,
                wHour: 2,
                ..Default::default()
            },
            ..standard_only
        };
        let summer = jiff::civil::date(2026, 7, 1).at(7, 30, 0, 0);
        assert_eq!(
            local_time(summer, &standard_only).unwrap(),
            jiff::civil::date(2026, 6, 30).at(23, 30, 0, 0)
        );
        assert_eq!(
            local_time(summer, &recurring).unwrap(),
            jiff::civil::date(2026, 7, 1).at(0, 30, 0, 0)
        );
        let winter = jiff::civil::date(2026, 1, 1).at(8, 30, 0, 0);
        assert_eq!(
            local_time(winter, &recurring).unwrap(),
            jiff::civil::date(2026, 1, 1).at(0, 30, 0, 0)
        );
    }
}
