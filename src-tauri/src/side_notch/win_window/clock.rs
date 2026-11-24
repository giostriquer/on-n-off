use jiff::{
    civil::DateTime,
    tz::{Offset, TimeZone},
    Timestamp,
};
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::Time::{
    GetDynamicTimeZoneInformation, SystemTimeToTzSpecificLocalTimeEx, DYNAMIC_TIME_ZONE_INFORMATION,
};

use crate::side_notch::plan::ViewerClock;

pub(super) struct WindowsClock(pub Timestamp);

impl ViewerClock for WindowsClock {
    fn now(&self) -> Timestamp {
        self.0
    }

    fn local_at(&self, at: Timestamp) -> DateTime {
        let mut settings = DYNAMIC_TIME_ZONE_INFORMATION::default();
        if unsafe { GetDynamicTimeZoneInformation(&mut settings) } != u32::MAX {
            if let Some(local) = local_time(at, &settings) {
                return local;
            }
        }
        TimeZone::system().to_datetime(at)
    }
}

fn local_time(at: Timestamp, settings: &DYNAMIC_TIME_ZONE_INFORMATION) -> Option<DateTime> {
    if settings.DynamicDaylightTimeDisabled {
        let seconds = (settings.Bias + settings.StandardBias).checked_mul(-60)?;
        return Some(Offset::from_seconds(seconds).ok()?.to_datetime(at));
    }
    let utc = TimeZone::UTC.to_datetime(at);
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
    unsafe { SystemTimeToTzSpecificLocalTimeEx(Some(settings), &input, &mut local) }.ok()?;
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
    fn disabling_daylight_adjustment_keeps_summer_resets_on_standard_time() {
        let settings = DYNAMIC_TIME_ZONE_INFORMATION {
            Bias: 480,
            DaylightBias: -60,
            DynamicDaylightTimeDisabled: true,
            ..Default::default()
        };
        let summer = "2026-07-01T07:30:00Z".parse().unwrap();
        assert_eq!(
            local_time(summer, &settings).unwrap(),
            jiff::civil::date(2026, 6, 30).at(23, 30, 0, 0)
        );
    }
}
