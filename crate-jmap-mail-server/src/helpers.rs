//! Private helper utilities shared across handler modules.

use jmap_types::{Id, JmapError};
use serde_json::Value;

/// Extract `accountId` from a JMAP method arguments object.
pub(crate) fn extract_account_id(args: &Value) -> Result<Id, JmapError> {
    match args.get("accountId").and_then(|v| v.as_str()) {
        Some(s) => Ok(Id::from(s)),
        None => Err(JmapError::invalid_arguments("accountId is required")),
    }
}

/// Return the current UTC instant formatted as an RFC 3339 string.
///
/// Uses `std::time::SystemTime` so no external dependency is needed.
pub(crate) fn now_utc_string() -> String {
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
