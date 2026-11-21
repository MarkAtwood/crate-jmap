//! UTC calendar formatting for SigV4 dates, JWT `exp` and `UTCDate`.

use std::time::{SystemTime, UNIX_EPOCH};

/// A UTC instant broken into calendar fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Utc {
    pub(crate) year: i64,
    pub(crate) month: u32,
    pub(crate) day: u32,
    pub(crate) hour: u32,
    pub(crate) minute: u32,
    pub(crate) second: u32,
}

/// Whole seconds since the Unix epoch; `0` for a time before it.
pub(crate) fn unix_seconds(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Utc {
    /// The calendar fields of `t`, truncated to the second.
    pub(crate) fn from_system_time(t: SystemTime) -> Self {
        let secs = unix_seconds(t);
        let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX / 2);
        let rem = secs % 86_400;
        let (year, month, day) = civil_from_days(days);
        // `rem` < 86_400, so each field fits a u32.
        let rem = u32::try_from(rem).unwrap_or(0);
        Self {
            year,
            month,
            day,
            hour: rem / 3600,
            minute: rem % 3600 / 60,
            second: rem % 60,
        }
    }

    /// SigV4 `X-Amz-Date`: `YYYYMMDD'T'HHMMSS'Z'`.
    pub(crate) fn amz_date(&self) -> String {
        format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// The credential-scope date: `YYYYMMDD`.
    pub(crate) fn scope_date(&self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }

    /// RFC 3339 `YYYY-MM-DDTHH:MM:SSZ` (RFC 8620 §1.4 `UTCDate`).
    pub(crate) fn rfc3339(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian
/// calendar. Howard Hinnant's `civil_from_days`,
/// <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    // d is in [1, 31] and m in [1, 12] by construction.
    (
        if m <= 2 { y + 1 } else { y },
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Oracle: the AWS SigV4 example date 20150830T123600Z is Unix time
    /// 1440938160 (`date -u -d @1440938160`).
    #[test]
    fn amz_date_of_the_sigv4_example() {
        let t = UNIX_EPOCH + Duration::from_secs(1_440_938_160);
        let u = Utc::from_system_time(t);
        assert_eq!(u.amz_date(), "20150830T123600Z");
        assert_eq!(u.scope_date(), "20150830");
        assert_eq!(u.rfc3339(), "2015-08-30T12:36:00Z");
    }

    /// 2024-02-29 (a leap day) is Unix day 19782.
    #[test]
    fn leap_day() {
        let t = UNIX_EPOCH + Duration::from_secs(19_782 * 86_400 + 1);
        assert_eq!(Utc::from_system_time(t).rfc3339(), "2024-02-29T00:00:01Z");
    }
}
