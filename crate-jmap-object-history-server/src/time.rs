//! Millisecond timestamps as RFC 8620 `UTCDate`s.

use jmap_types::{UTCDate, ValidationError};

/// `ms` milliseconds since the Unix epoch as a `UTCDate`, with a
/// fractional second only when it is not zero (RFC 8620 §1.4) and never
/// more than millisecond precision (§3.1 SHOULD NOT). Clamped to years
/// 0000 to 9999.
pub fn utc_date_from_millis(ms: i64) -> UTCDate {
    // 0000-01-01T00:00:00Z and 9999-12-31T23:59:59.999Z.
    let ms = ms.clamp(-62_167_219_200_000, 253_402_300_799_999);
    let secs = ms.div_euclid(1000);
    let frac = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days ("chrono-Compatible Low-Level Date
    // Algorithms"), the inverse of the days_from_civil behind
    // UTCDate::to_epoch_seconds.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let mut s = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    );
    if frac != 0 {
        let f = format!("{frac:03}");
        s.push('.');
        s.push_str(f.trim_end_matches('0'));
    }
    s.push('Z');
    UTCDate::from(s)
}

/// `date` as milliseconds since the Unix epoch, truncating any precision
/// below a millisecond.
///
/// # Errors
///
/// [`ValidationError`] when `date` is not a valid `UTCDate`.
pub fn millis_from_utc_date(date: &UTCDate) -> Result<i64, ValidationError> {
    let secs = date.to_epoch_seconds()?;
    let s: &str = date.as_ref();
    let frac: i64 = s
        .get(19..s.len().saturating_sub(1))
        .and_then(|f| f.strip_prefix('.'))
        .map_or(0, |f| {
            let digits: String = f.chars().chain("000".chars()).take(3).collect();
            digits.parse().unwrap_or(0)
        });
    Ok(secs.saturating_mul(1000).saturating_add(frac))
}
