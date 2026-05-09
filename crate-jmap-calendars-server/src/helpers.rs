//! Private helper utilities shared across handler modules.

use jmap_types::Id;
use serde_json::{json, Map, Value};

use crate::backend::SetDefaultResult;

pub(crate) use jmap_server::extract_account_id;

/// Serialize a [`crate::backend::SetError`] to a JSON value for inclusion in
/// `notCreated`/`notUpdated`/`notDestroyed` maps.
///
/// `SetError` (re-exported from `jmap-server`) uses `#[derive(Serialize)]` on
/// plain data, so `serde_json::to_value` is infallible; we assert that with
/// `.expect()` rather than silently masking a hypothetical failure as
/// `serverFail`. A `serverFail` description carrying a serde error string is
/// not something any client knows to look at, and the fallback would also
/// hide a real bug if a future custom `Serialize` impl ever did fail.
pub(crate) fn set_error_value(e: &crate::backend::SetError) -> serde_json::Value {
    serde_json::to_value(e).expect("derive(Serialize) on plain data is infallible")
}

/// Resolve an `onSuccessSetIsDefault` argument value to the target [`Id`].
///
/// Per draft-ietf-jmap-calendars-26 §3.3 / §4.3, a `#`-prefixed string
/// references a creation id from the same `/set` call; the resolved id is
/// the backend-assigned id for that creation. A bare string is treated as a
/// literal id. Any other JSON shape (number, object, array) yields `None`
/// and the caller silently skips the default-change per the spec's
/// "no error is returned" rule for malformed/unknown ids.
///
/// `created` is the in-progress `created` map; entries map a creation id
/// (the key) to the serialized created object whose `id` field carries the
/// backend-assigned id.
pub(crate) fn resolve_on_success_set_is_default(
    raw: &Value,
    created: &Map<String, Value>,
) -> Option<Id> {
    let s = raw.as_str()?;
    if let Some(create_ref) = s.strip_prefix('#') {
        // Look up the assigned id from the matching created entry.
        // The value's "id" field is what the backend assigned.
        let entry = created.get(create_ref)?;
        let assigned = entry.get("id")?.as_str()?;
        Some(Id::from(assigned))
    } else {
        Some(Id::from(s))
    }
}

/// Apply the response-mutation contract for `onSuccessSetIsDefault`
/// (draft-ietf-jmap-calendars-26 §3.3, §4.3).
///
/// On a successful default change, the spec requires that any object whose
/// `isDefault` flipped MUST appear in either the `created` or `updated`
/// argument with the server-set value included (RFC 8620 §5.3 echo rule).
///
/// Behaviour:
/// - If `result.new_default` matches an entry in `created` (by its
///   backend-assigned `id`), that entry is mutated in place to carry
///   `isDefault: true`.
/// - Otherwise, a `updated.<new_default>` entry is created or merged with
///   `isDefault: true`. A pre-existing `null` value is upgraded to an
///   object so the field can be added.
/// - If `result.previous_default` is `Some` and differs from
///   `result.new_default`, an `updated.<previous_default>` entry is created
///   or merged with `isDefault: false`.
///
/// Returns `true` if any visible state changed (so the caller knows to
/// re-fetch `newState` from the backend). Returns `false` when
/// `result.new_default` is `None` (silent no-op).
pub(crate) fn apply_default_change_to_response(
    created: &mut Map<String, Value>,
    updated: &mut Map<String, Value>,
    result: &SetDefaultResult,
) -> bool {
    let Some(new_default) = result.new_default.as_ref() else {
        // Silent no-op per §3.3 / §4.3 (id not found or forbidden).
        return false;
    };

    let mut state_changed = false;

    // Try to find a matching entry in `created` first. The created map's
    // values are the full serialized objects, each carrying its backend-
    // assigned `id` field.
    let mut updated_in_created = false;
    for (_create_id, val) in created.iter_mut() {
        let assigned_id = val.get("id").and_then(|i| i.as_str());
        if assigned_id == Some(new_default.as_ref()) {
            // Mutate in-place: insert/overwrite isDefault on this created entry.
            if let Some(obj) = val.as_object_mut() {
                obj.insert("isDefault".to_owned(), json!(true));
                updated_in_created = true;
                state_changed = true;
            }
            break;
        }
    }

    if !updated_in_created {
        // The new default is an existing object (not created in this /set),
        // OR is in created but the entry was malformed. Fall through to
        // emitting an updated entry — RFC 8620 §5.3 lets us return only the
        // server-changed field rather than the whole object.
        merge_is_default(updated, new_default.as_ref(), true);
        state_changed = true;
    }

    if let Some(previous) = result.previous_default.as_ref() {
        if previous != new_default {
            merge_is_default(updated, previous.as_ref(), false);
            state_changed = true;
        }
    }

    state_changed
}

/// Insert or merge `{"isDefault": flag}` into `updated[id]`. If the entry
/// is `Value::Null` (the JMAP convention for "applied verbatim, no
/// server-set fields"), it is upgraded to an object so we can add the
/// flipped flag.
fn merge_is_default(updated: &mut Map<String, Value>, id: &str, flag: bool) {
    let entry = updated
        .entry(id.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if entry.is_null() {
        *entry = Value::Object(Map::new());
    }
    if let Some(obj) = entry.as_object_mut() {
        obj.insert("isDefault".to_owned(), json!(flag));
    }
}
