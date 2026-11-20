//! Slack `ts` values and RFC 8620 §1.4 `UTCDate`s.

use jmap_types::UTCDate;

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian
/// (Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms",
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // `day` is 1..=31 and `month` 1..=12 by construction.
    (year, month as u32, day as u32)
}

/// `secs` Unix seconds plus `millis` as a `UTCDate`, the fraction omitted
/// when zero (RFC 8620 §1.4: "If the time-secfrac is zero, it MUST be
/// omitted").
pub(crate) fn utc_from_unix(secs: i64, millis: u32) -> UTCDate {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let frac = if millis == 0 {
        String::new()
    } else {
        format!(".{millis:03}")
    };
    UTCDate::from(format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}{frac}Z").as_str())
}

/// A Slack `ts` (`"1355517523.000005"`) as a `UTCDate`, to the millisecond.
pub(crate) fn utc_from_ts(ts: &str) -> Option<UTCDate> {
    let (secs, frac) = ts.split_once('.').unwrap_or((ts, ""));
    let secs: i64 = secs.parse().ok()?;
    let millis = frac
        .chars()
        .chain(std::iter::repeat('0'))
        .take(3)
        .collect::<String>()
        .parse::<u32>()
        .ok()?;
    Some(utc_from_unix(secs, millis))
}

/// The current time as a `UTCDate`.
pub(crate) fn now_utc() -> UTCDate {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(now.as_secs()).unwrap_or(i64::MAX);
    utc_from_unix(secs, now.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: `date -u -d @1355517523 +%FT%TZ` (GNU coreutils).
    #[test]
    fn ts_to_utc_date() {
        assert_eq!(
            utc_from_ts("1355517523.000005").map(|d| d.to_string()),
            Some("2012-12-14T20:38:43Z".to_owned())
        );
        assert_eq!(
            utc_from_ts("1512085950.218404").map(|d| d.to_string()),
            Some("2017-11-30T23:52:30.218Z".to_owned())
        );
        assert_eq!(
            utc_from_unix(0, 0).to_string(),
            "1970-01-01T00:00:00Z".to_owned()
        );
        // 2000-02-29 (leap day): 951782400.
        assert_eq!(
            utc_from_unix(951_782_400, 0).to_string(),
            "2000-02-29T00:00:00Z"
        );
        assert!(utc_from_ts("nope").is_none());
    }

    #[test]
    fn now_is_valid() {
        assert!(UTCDate::new_validated(now_utc().to_string()).is_ok());
    }
}
