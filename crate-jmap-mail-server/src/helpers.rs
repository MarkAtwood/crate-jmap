//! Private helper utilities — re-exported from jmap_server.
pub(crate) use jmap_server::{extract_account_id, not_found_json, now_utc_string, ser};

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
