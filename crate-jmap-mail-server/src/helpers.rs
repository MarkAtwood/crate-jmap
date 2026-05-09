//! Private helper utilities — re-exported from jmap_server.
use std::collections::HashSet;

use jmap_types::{Id, Invocation, JmapError, JmapObject, PatchObject, State};
use serde_json::{json, Map, Value};

use crate::backend::MailBackend;

pub(crate) use jmap_server::{extract_account_id, not_found_json, now_utc_string, ser};

/// Sentinel blob ID set by the `Email/set` create and import handlers.
///
/// Backends MUST replace this value in [`crate::backend::MailBackend::create_object`]
/// and [`crate::backend::MailBackend::import_email`] with the real blob ID before
/// returning. Clients must never see this value. Defined as a constant so all
/// three sites that reference it (handler, debug_assert, backend test harness)
/// use the same string and a rename is caught at compile time.
pub(crate) const PLACEHOLDER_BLOB_ID: &str = "placeholder-blob";

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
    serde_json::to_value(e).expect("derive(Serialize) on plain data is infallible")
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
///
/// `IMMUTABLE_EMAIL_FIELDS` has 21 entries; a linear scan is simpler and fast
/// enough that a static `HashSet` adds no benefit.
pub(crate) fn find_immutable_patch_key(patch: &PatchObject) -> Option<&'static str> {
    let map = patch.as_map();
    for key in map.keys() {
        for &field in IMMUTABLE_EMAIL_FIELDS {
            // Exact match, or sub-path "field/..." — both are immutable.
            //
            // The byte-index check `key.as_bytes().get(field.len()) == Some(&b'/')`
            // is the correct, zero-allocation way to distinguish three cases
            // (using `field = "messageId"` as the example):
            //
            //   "messageId"    → exact match            → blocked (== check above)
            //   "messageId/0"  → sub-path               → blocked (starts_with + byte check)
            //   "messageIdX"   → coincidental prefix    → allowed
            //
            // Two simpler-looking alternatives are both wrong:
            //   `key.starts_with(field)` alone would block "messageIdX" (false positive).
            //   `key.starts_with(&format!("{field}/"))` is correct but allocates a
            //   String on every iteration — avoidable given we only need the one byte.
            if key == field
                || (key.starts_with(field) && key.as_bytes().get(field.len()) == Some(&b'/'))
            {
                return Some(field);
            }
        }
    }
    None
}

/// Build the final `/set` method response and re-fetch `newState` from the
/// backend if any mutation occurred.
///
/// Five `/set` handlers in this crate (`Email/set`, `Mailbox/set`,
/// `Identity/set`, `SieveScript/set`, `VacationResponse/set`) end with the
/// same boilerplate: refresh the state token if `mutated`, then emit a
/// `(Value, Vec<Invocation>)` tuple wrapping the canonical RFC 8620 §5.3
/// envelope. Centralising it here keeps the five sites in lockstep — if a
/// future revision changes which keys are emitted (e.g. RFC 8620 §5.3.1 may
/// flip a key from `null` to omitted), all five handlers update at once.
///
/// `EmailSubmission/set` deliberately does NOT use this helper: it pushes
/// follow-up `Email/set` invocations from `onSuccessUpdateEmail` /
/// `onSuccessDestroyEmail` (RFC 8621 §7.4) and therefore needs to control
/// the returned `Vec<Invocation>` rather than relying on the helper's
/// hardcoded `vec![]`.
///
/// The `O` type parameter is the JMAP object type for the state token
/// (e.g. `Email`, `Mailbox`); its only role is to disambiguate the
/// `get_state::<O>` call inside the helper.
///
/// Empty maps/arrays serialize as `null` (JMAP convention). The `Invocation`
/// vector is always empty for the five call sites — no `/set` call generates
/// follow-up invocations today.
//
// The clippy::too_many_arguments lint fires here because the six accumulator
// collections (`created`, `updated`, `destroyed_list`, `not_created`,
// `not_updated`, `not_destroyed`) are passed individually rather than bundled
// into a builder struct. Bundling them is the natural follow-up but would
// force a textual rename of ~150 references across the five /set handlers — a
// larger and more invasive change than is in scope for the boilerplate-extraction
// step. Allowing the lint here keeps the handler-side diff to one line each
// (`finalize_set_response::<B, O>(...).await`) and leaves the builder refactor
// as a tractable follow-on.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_set_response<B, O>(
    backend: &B,
    account_id: &Id,
    old_state: State,
    mutated: bool,
    created: Map<String, Value>,
    updated: Map<String, Value>,
    destroyed_list: Vec<Value>,
    not_created: Map<String, Value>,
    not_updated: Map<String, Value>,
    not_destroyed: Map<String, Value>,
) -> Result<(Value, Vec<Invocation>), JmapError>
where
    B: MailBackend,
    O: JmapObject + Send + Sync,
{
    let new_state = if mutated {
        backend
            .get_state::<O>(account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    } else {
        old_state.clone()
    };

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldState": old_state.as_ref(),
            "newState": new_state.as_ref(),
            "created":      if created.is_empty()        { Value::Null } else { Value::Object(created) },
            "updated":      if updated.is_empty()        { Value::Null } else { Value::Object(updated) },
            "destroyed":    if destroyed_list.is_empty() { Value::Null } else { Value::Array(destroyed_list) },
            "notCreated":   if not_created.is_empty()    { Value::Null } else { Value::Object(not_created) },
            "notUpdated":   if not_updated.is_empty()    { Value::Null } else { Value::Object(not_updated) },
            "notDestroyed": if not_destroyed.is_empty()  { Value::Null } else { Value::Object(not_destroyed) },
        }),
        vec![],
    ))
}
