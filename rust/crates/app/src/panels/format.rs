//! Display formatting for dates, durations and sizes, matching the strings
//! SwiftUI's formatters produce in an English locale.

use std::time::{SystemTime, UNIX_EPOCH};

/// A broken-down local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// The local civil time of an instant.
#[cfg(windows)]
pub fn civil(time: SystemTime) -> Civil {
    use windows_sys::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime},
    };

    const UNIX_EPOCH_AS_FILETIME: i128 = 116_444_736_000_000_000;
    let ticks = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128 / 100,
        Err(e) => -(e.duration().as_nanos() as i128 / 100),
    } + UNIX_EPOCH_AS_FILETIME;
    let ticks = ticks.max(0) as u64;
    let file_time = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    // SAFETY: both calls only write into the zeroed structs we own.
    let local = unsafe {
        let mut utc: SYSTEMTIME = std::mem::zeroed();
        let mut local: SYSTEMTIME = std::mem::zeroed();
        if FileTimeToSystemTime(&file_time, &mut utc) == 0
            || SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0
        {
            return Civil { year: 1970, month: 1, day: 1, hour: 0, minute: 0 };
        }
        local
    };
    Civil {
        year: i32::from(local.wYear),
        month: u32::from(local.wMonth),
        day: u32::from(local.wDay),
        hour: u32::from(local.wHour),
        minute: u32::from(local.wMinute),
    }
}

/// The local civil time of an instant.
#[cfg(unix)]
pub fn civil(time: SystemTime) -> Civil {
    let secs = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as libc::time_t,
        Err(e) => -(e.duration().as_secs() as libc::time_t),
    };
    // SAFETY: `localtime_r` only writes into the zeroed `tm` we own.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    };
    Civil {
        year: tm.tm_year + 1900,
        month: tm.tm_mon as u32 + 1,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "3:05 PM".
pub fn short_time(c: Civil) -> String {
    let hour = match c.hour % 12 {
        0 => 12,
        h => h,
    };
    format!("{hour}:{:02} {}", c.minute, if c.hour < 12 { "AM" } else { "PM" })
}

/// "Nov 11, 2026".
pub fn abbreviated_date(c: Civil) -> String {
    format!("{} {}, {}", MONTHS[(c.month as usize).saturating_sub(1).min(11)], c.day, c.year)
}

/// "Nov 11, 2026 at 3:05 PM".
pub fn abbreviated_date_time(c: Civil) -> String {
    format!("{} at {}", abbreviated_date(c), short_time(c))
}

/// Time remaining as SwiftUI's relative date style shows it: "4 min, 9 sec".
pub fn relative_duration(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    match (h, m) {
        (0, 0) => format!("{s} sec"),
        (0, _) => format!("{m} min, {s} sec"),
        _ => format!("{h} hr, {m} min"),
    }
}

/// Decimal file sizes as Finder shows them: "845 KB", "12.4 MB".
pub fn file_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return if bytes == 1 { "1 byte".to_string() } else { format!("{bytes} bytes") };
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value >= 100.0 || unit == 1 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// A path with the home directory abbreviated to `~`.
pub fn abbreviate_home(path: &str, home: Option<&str>) -> String {
    match home {
        Some(home) if path == home => "~".to_string(),
        Some(home) => match path.strip_prefix(home) {
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_string(),
        },
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    fn at(hour: u32, minute: u32) -> Civil {
        Civil { year: 2026, month: 11, day: 9, hour, minute }
    }

    #[test]
    fn times_use_twelve_hour_clock() {
        assert_eq!(short_time(at(0, 5)), "12:05 AM");
        assert_eq!(short_time(at(12, 0)), "12:00 PM");
        assert_eq!(short_time(at(15, 30)), "3:30 PM");
        assert_eq!(abbreviated_date_time(at(15, 30)), "Nov 9, 2026 at 3:30 PM");
        assert_eq!(abbreviated_date(at(1, 1)), "Nov 9, 2026");
    }

    #[test]
    fn relative_durations() {
        assert_eq!(relative_duration(9), "9 sec");
        assert_eq!(relative_duration(249), "4 min, 9 sec");
        assert_eq!(relative_duration(7380), "2 hr, 3 min");
    }

    #[test]
    fn file_sizes() {
        assert_eq!(file_size(1), "1 byte");
        assert_eq!(file_size(999), "999 bytes");
        assert_eq!(file_size(845_000), "845 KB");
        assert_eq!(file_size(12_400_000), "12.4 MB");
        assert_eq!(file_size(123_400_000), "123 MB");
        assert_eq!(file_size(1_500_000_000), "1.5 GB");
    }

    #[test]
    fn home_abbreviation_only_applies_on_component_boundaries() {
        assert_eq!(abbreviate_home("/Users/me", Some("/Users/me")), "~");
        assert_eq!(abbreviate_home("/Users/me/src", Some("/Users/me")), "~/src");
        assert_eq!(abbreviate_home("/Users/melt/src", Some("/Users/me")), "/Users/melt/src");
        assert_eq!(abbreviate_home("/tmp", None), "/tmp");
    }

    #[test]
    fn local_time_conversion_is_plausible() {
        let c = civil(SystemTime::now());
        assert!(c.year >= 2024 && (1..=12).contains(&c.month) && c.hour < 24);
    }
}
