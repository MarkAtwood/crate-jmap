//! Private helper utilities shared across handler modules.

pub(crate) use jmap_server::{extract_account_id, not_found_json, now_utc_string, ser};

/// Returns `true` if RFC 3339 UTC timestamp `a` is strictly before `b`.
///
/// Comparison is performed on the first 19 characters (`YYYY-MM-DDTHH:MM:SS`)
/// so that fractional-second suffixes produced by some clients (e.g.
/// `"2025-06-01T12:00:00.000Z"`) do not corrupt the result.  The
/// plain-ASCII lexicographic order of the ISO 8601 prefix is identical to
/// chronological order for well-formed UTC timestamps.
pub(crate) fn iso8601_before(a: &str, b: &str) -> bool {
    let a_sec = &a[..a.len().min(19)];
    let b_sec = &b[..b.len().min(19)];
    a_sec < b_sec
}

/// Serialize a [`SetError`] to a JSON value for inclusion in
/// `notCreated`/`notUpdated`/`notDestroyed` maps.
///
/// Falls back to a `serverFail` object on the extremely unlikely event that
/// `SetError`'s `Serialize` impl panics.
pub(crate) fn set_error_value(e: &crate::backend::SetError) -> serde_json::Value {
    serde_json::to_value(e).unwrap_or_else(
        |err| serde_json::json!({ "type": "serverFail", "description": err.to_string() }),
    )
}
