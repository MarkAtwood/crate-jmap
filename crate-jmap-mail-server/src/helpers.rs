//! Private helper utilities — re-exported from jmap_server.
pub(crate) use jmap_server::{extract_account_id, not_found_json, now_utc_string, ser};

/// Collapse an empty JSON map to `null`; otherwise wrap it as a JSON object.
///
/// Used in RFC 8621 response serialisation: JMAP /get responses represent
/// absent optional maps as JSON `null` rather than `{}`.
#[allow(dead_code)]
pub(crate) fn map_or_null(m: serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    if m.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::Object(m)
    }
}

/// Collapse an empty JSON array to `null`; otherwise wrap it as a JSON array.
///
/// Used in RFC 8621 response serialisation: JMAP /get responses represent
/// absent optional arrays as JSON `null` rather than `[]`.
#[allow(dead_code)]
pub(crate) fn array_or_null(v: Vec<serde_json::Value>) -> serde_json::Value {
    if v.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::Array(v)
    }
}
