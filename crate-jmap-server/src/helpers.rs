//! Shared helper utilities for JMAP method handlers.

use jmap_types::{Id, JmapError};
use serde_json::Value;

/// Serialize any [`serde::Serialize`] type to a [`serde_json::Value`],
/// mapping serialization errors to [`JmapError::server_fail`].
pub fn ser<T: serde::Serialize>(val: T) -> Result<serde_json::Value, JmapError> {
    serde_json::to_value(val).map_err(|e| JmapError::server_fail(e.to_string()))
}

/// Convert a slice of [`Id`]s to a JSON `notFound` value.
///
/// Returns `Value::Null` when the slice is empty (the `notFound` field will be
/// JSON `null`, which RFC 8620 §5.1 specifies when no ids are not-found), or
/// `Value::Array` of string ids when non-empty.
pub fn not_found_json(ids: &[Id]) -> Value {
    if ids.is_empty() {
        Value::Null
    } else {
        Value::Array(
            ids.iter()
                .map(|id| Value::String(id.as_ref().to_owned()))
                .collect(),
        )
    }
}

/// Extract `accountId` from a JMAP method arguments object.
pub fn extract_account_id(args: &Value) -> Result<Id, JmapError> {
    match args.get("accountId").and_then(|v| v.as_str()) {
        Some(s) => Ok(Id::from(s)),
        None => Err(JmapError::invalid_arguments("accountId is required")),
    }
}

/// Return the current UTC instant formatted as an RFC 3339 string.
///
/// Uses `std::time::SystemTime` so no external dependency is needed.
pub fn now_utc_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let (year, month, day) = civil_from_days(days);

    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Convert a count of days since the Unix epoch (1970-01-01) to a proleptic
/// Gregorian (year, month, day) triple.
///
/// Algorithm by Howard Hinnant (public domain):
/// <https://howardhinnant.github.io/date_algorithms.html>
fn civil_from_days(z: i64) -> (i32, u8, u8) {
    let z = z + 719_468;
    let era: i64 = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let mo = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let yr = if mo <= 2 { y + 1 } else { y };
    (yr as i32, mo as u8, d as u8)
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, now_utc_string};

    /// Test vectors derived independently with Python's `datetime.date` module.
    /// `days` is the count of days since 1970-01-01.
    #[test]
    fn civil_from_days_known_dates() {
        let cases: &[(i64, (i32, u8, u8))] = &[
            (0, (1970, 1, 1)),       // Unix epoch
            (365, (1971, 1, 1)),     // one year later (1970 is not a leap year)
            (10957, (2000, 1, 1)),   // Y2K
            (11016, (2000, 2, 29)),  // leap day in a century-divisible leap year
            (11017, (2000, 3, 1)),   // day after the leap day (era boundary in algorithm)
            (19358, (2023, 1, 1)),   // a recent non-leap year start
            (19722, (2023, 12, 31)), // end of 2023
            (19782, (2024, 2, 29)),  // leap day in 2024
            (19783, (2024, 3, 1)),   // day after 2024 leap day
        ];

        for &(days, expected) in cases {
            assert_eq!(
                civil_from_days(days),
                expected,
                "civil_from_days({days}) mismatch"
            );
        }
    }

    #[test]
    fn now_utc_string_format() {
        let s = now_utc_string();
        // Must match YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(s.len(), 20, "unexpected length: {s}");
        assert!(s.ends_with('Z'), "must end with Z: {s}");
        assert_eq!(&s[4..5], "-", "missing year-month separator: {s}");
        assert_eq!(&s[7..8], "-", "missing month-day separator: {s}");
        assert_eq!(&s[10..11], "T", "missing date-time separator: {s}");
        assert_eq!(&s[13..14], ":", "missing hour-minute separator: {s}");
        assert_eq!(&s[16..17], ":", "missing minute-second separator: {s}");
        assert!(
            s.starts_with("20"),
            "year should start with 20 in 21st century: {s}"
        );
    }
}
