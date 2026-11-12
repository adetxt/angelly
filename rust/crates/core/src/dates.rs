use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn from_unix_secs(seconds: f64) -> Option<SystemTime> {
    if !seconds.is_finite() {
        return None;
    }
    if seconds >= 0.0 {
        UNIX_EPOCH.checked_add(Duration::try_from_secs_f64(seconds).ok()?)
    } else {
        UNIX_EPOCH.checked_sub(Duration::try_from_secs_f64(-seconds).ok()?)
    }
}

pub(crate) fn to_unix_secs(time: SystemTime) -> f64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs_f64(),
        Err(e) => -e.duration().as_secs_f64(),
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn digits(s: &str, len: usize) -> Option<i64> {
    (s.len() == len && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok())?
}

fn to_system_time(seconds: i64, nanos: u32) -> Option<SystemTime> {
    let base = if seconds >= 0 {
        UNIX_EPOCH.checked_add(Duration::from_secs(seconds as u64))?
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))?
    };
    base.checked_add(Duration::from_nanos(nanos.into()))
}

fn civil(date: &str, time: &str) -> Option<i64> {
    let mut d = date.split('-');
    let (year, month, day) = (digits(d.next()?, 4)?, digits(d.next()?, 2)?, digits(d.next()?, 2)?);
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut t = time.split(':');
    let (hour, minute, second) = (digits(t.next()?, 2)?, digits(t.next()?, 2)?, digits(t.next()?, 2)?);
    if t.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Parses an RFC 3339 / ISO 8601 internet date-time such as `2026-11-10T12:30:00.123Z`
/// or `2026-11-10T12:30:00+02:00`.
pub(crate) fn parse_iso8601(text: &str) -> Option<SystemTime> {
    let (date, rest) = text.split_once('T')?;
    let zone_at = rest.find(['Z', '+', '-'])?;
    let (clock, zone) = rest.split_at(zone_at);
    let (clock, nanos) = match clock.split_once('.') {
        Some((whole, fraction)) => {
            if fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let padded: String = fraction.chars().chain(std::iter::repeat('0')).take(9).collect();
            (whole, padded.parse::<u32>().ok()?)
        }
        None => (clock, 0),
    };
    let offset = match zone {
        "Z" => 0,
        _ => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let body = &zone[1..];
            let (h, m) = match body.split_once(':') {
                Some((h, m)) => (digits(h, 2)?, digits(m, 2)?),
                None if body.len() == 4 => (digits(&body[..2], 2)?, digits(&body[2..], 2)?),
                None => return None,
            };
            sign * (h * 3_600 + m * 60)
        }
    };
    to_system_time(civil(date, clock)? - offset, nanos)
}

/// Parses an HTTP-date in the preferred `Sun, 06 Nov 1994 08:49:37 GMT` form.
pub(crate) fn parse_http_date(text: &str) -> Option<SystemTime> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (_, rest) = text.split_once(", ")?;
    let mut parts = rest.split(' ');
    let day = digits(parts.next()?, 2)?;
    let month_name = parts.next()?;
    let month = MONTHS.iter().position(|m| *m == month_name)? as i64 + 1;
    let year = digits(parts.next()?, 4)?;
    let clock = parts.next()?;
    if parts.next()? != "GMT" || parts.next().is_some() {
        return None;
    }
    let date = format!("{year:04}-{month:02}-{day:02}");
    to_system_time(civil(&date, clock)?, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(t: SystemTime) -> f64 {
        to_unix_secs(t)
    }

    #[test]
    fn iso8601_forms() {
        assert_eq!(secs(parse_iso8601("1970-01-01T00:00:00Z").unwrap()), 0.0);
        assert_eq!(secs(parse_iso8601("2026-11-10T12:30:00Z").unwrap()), 1_794_313_800.0);
        let fractional = secs(parse_iso8601("2026-11-10T12:30:00.123Z").unwrap());
        assert!((fractional - 1_794_313_800.123).abs() < 1e-6);
        assert_eq!(parse_iso8601("2026-11-10T14:30:00+02:00"), parse_iso8601("2026-11-10T12:30:00Z"));
        assert_eq!(parse_iso8601("2026-11-10T07:30:00-0500"), parse_iso8601("2026-11-10T12:30:00Z"));
        assert_eq!(secs(parse_iso8601("1969-12-31T23:59:59Z").unwrap()), -1.0);
    }

    #[test]
    fn iso8601_rejects_malformed_input() {
        for bad in ["invalid", "", "2026-11-10", "2026-11-10T12:30:00", "2026-13-10T12:30:00Z", "2026-11-10T25:00:00Z", "2026-11-10T12:30:00.Z"] {
            assert_eq!(parse_iso8601(bad), None, "{bad}");
        }
    }

    #[test]
    fn http_dates() {
        assert_eq!(secs(parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT").unwrap()), 784_111_777.0);
        assert_eq!(parse_http_date("Sun, 06 Nov 1994 08:49:37 PST"), None);
        assert_eq!(parse_http_date("garbage"), None);
    }

    #[test]
    fn unix_seconds_round_trip() {
        assert_eq!(secs(from_unix_secs(1_800_000_000.0).unwrap()), 1_800_000_000.0);
        assert_eq!(from_unix_secs(f64::NAN), None);
    }
}
