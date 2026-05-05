//! Shared helper utilities for FileNode method handlers.

pub(crate) use jmap_server::extract_account_id;

/// Serialize a [`SetError`] to a JSON value for inclusion in
/// `notCreated`/`notUpdated`/`notDestroyed` maps.
///
/// Falls back to a `serverFail` object on the unlikely event that
/// `SetError`'s `Serialize` impl fails.
pub(crate) fn set_error_value(e: &jmap_server::SetError) -> serde_json::Value {
    serde_json::to_value(e).unwrap_or_else(
        |err| serde_json::json!({ "type": "serverFail", "description": err.to_string() }),
    )
}
