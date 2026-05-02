//! Private helper utilities — re-exported from jmap_server.
use std::collections::HashSet;
use std::sync::OnceLock;

use serde_json::Value;

pub(crate) use jmap_server::{extract_account_id, not_found_json, now_utc_string, ser};

/// Return only the keys in `prop_set` from the JSON object `obj`.
///
/// Used by all `*/get` handlers to enforce the RFC 8620 §5.1 rule that when
/// `properties` is specified the server MUST return only those fields (plus
/// `id`, which callers must include in `prop_set` if they want it).
///
/// The caller is responsible for building the `HashSet` once before iterating
/// over multiple objects so the set is not rebuilt on every call.
///
/// Takes `&Value` and clones surviving entries because the same `val` may be
/// needed after this call (e.g. for `header:` extraction in `handle_email_get`).
pub(crate) fn filter_properties(obj: &Value, prop_set: &HashSet<&str>) -> Value {
    match obj {
        Value::Object(map) => {
            let filtered: serde_json::Map<String, Value> = map
                .iter()
                .filter(|(k, _)| prop_set.contains(k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Value::Object(filtered)
        }
        _ => obj.clone(),
    }
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
