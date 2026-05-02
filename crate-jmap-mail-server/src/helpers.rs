//! Private helper utilities — re-exported from jmap_server.
use std::collections::HashSet;
use std::sync::OnceLock;

use serde_json::Value;

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

/// Immutable Email fields (RFC 8621 §5.5.4).
///
/// A patch key that equals or starts with `"<field>/"` for any of these names
/// is rejected with `invalidProperties`.
const IMMUTABLE_EMAIL_FIELDS: &[&str] = &[
    "id",
    "blobId",
    "threadId",
    "size",
    "receivedAt",
    "messageId",
    "inReplyTo",
    "references",
    "sender",
    "from",
    "to",
    "cc",
    "bcc",
    "replyTo",
    "subject",
    "sentAt",
    "bodyStructure",
    "bodyValues",
    "textBody",
    "htmlBody",
    "attachments",
    "hasAttachment",
    "preview",
    "headers",
];

/// Return the first patch key that names an immutable Email field, if any.
///
/// Used by `handle_email_set` and the `onSuccess*` side-effect paths in
/// `handle_email_copy` and `handle_submission_set` to enforce RFC 8621 §5.5.4.
///
/// A patch key violates immutability if it equals an immutable field name, or
/// starts with `"<field>/"` (JSON Merge Patch sub-path syntax).
pub(crate) fn find_immutable_patch_key(patch: &Value) -> Option<&'static str> {
    // Build the lookup set once; subsequent calls reuse it.
    static IMMUTABLE_SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    let set = IMMUTABLE_SET.get_or_init(|| IMMUTABLE_EMAIL_FIELDS.iter().copied().collect());

    let map = patch.as_object()?;
    for key in map.keys() {
        // Check exact match first via the O(1) HashSet lookup.
        if set.contains(key.as_str()) {
            // Return the canonical &'static str from the array so callers get a
            // stable pointer regardless of which spelling the client used.
            return IMMUTABLE_EMAIL_FIELDS
                .iter()
                .copied()
                .find(|&f| f == key.as_str());
        }
        // Then check sub-path matches: "field/..." is also immutable.
        // The byte-index check distinguishes three cases for `field = "messageId"`:
        //   "messageId"    → exact match (blocked above)
        //   "messageId/0"  → sub-path match (blocked here)
        //   "messageIdX"   → prefix but not a path segment (allowed)
        for &field in IMMUTABLE_EMAIL_FIELDS {
            if key.starts_with(field) && key.as_bytes().get(field.len()) == Some(&b'/') {
                return Some(field);
            }
        }
    }
    None
}
