//! Private helper utilities shared across handler modules.

use jmap_types::{Id, Invocation, JmapError, JmapObject, State};
use serde_json::{json, Map, Value};

use crate::backend::{CalendarsBackend, SetDefaultResult};

pub(crate) use jmap_server::extract_account_id;

/// Build the final `/set` method response and re-fetch `newState` from the
/// backend if any mutation occurred.
///
/// All four `/set` handlers in this crate (`Calendar/set`, `CalendarEvent/set`,
/// `CalendarEventNotification/set`, `ParticipantIdentity/set`) end with the
/// same boilerplate: refresh the state token if `mutated`, then emit a
/// `(Value, Vec<Invocation>)` tuple wrapping the canonical RFC 8620 §5.3
/// envelope. Centralising it here keeps the four sites in lockstep — if a
/// future revision changes which keys are emitted (e.g. RFC 8620 §5.3.1 may
/// flip a key from `null` to omitted), all four handlers update at once.
///
/// The `O` type parameter is the JMAP object type for the state token
/// (e.g. `Calendar`, `CalendarEvent`); its only role is to disambiguate the
/// `get_state::<O>` call inside the helper.
///
/// Empty maps/arrays serialize as `null` (JMAP convention). The `Invocation`
/// vector is always empty for the four call sites — no `/set` call generates
/// follow-up invocations today.
//
// The clippy::too_many_arguments lint fires here because the six accumulator
// collections (`created`, `updated`, `destroyed_list`, `not_created`,
// `not_updated`, `not_destroyed`) are passed individually rather than bundled
// into a builder struct. Bundling them is the natural follow-up (the
// JMAP-r3pg.12 description explicitly mentions a SetResponseBuilder<O>) but
// would force a textual rename of ~150 references across the four /set
// handlers — a larger and more invasive change than is in scope for the
// boilerplate-extraction step. Allowing the lint here keeps the handler-side
// diff to one line each (`finalize_set_response::<B, O>(...).await`); the
// builder refactor is tracked under bd:JMAP-g7wu.3.3 (propagates from
// bd:JMAP-g7wu.3.1).
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
    B: CalendarsBackend,
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
