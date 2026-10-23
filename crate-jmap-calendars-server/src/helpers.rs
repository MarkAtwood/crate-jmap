//! Private helper utilities shared across handler modules.

use jmap_types::{Id, JmapError, JmapObject, State};
use serde_json::{Map, Value, json};

use crate::{CalendarsBackend, SetDefaultResult};
use jmap_server::{HandlerOutput, server_fail_from_backend};

pub(crate) use jmap_server::{
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
};

/// Per-`/set` accumulators emitted in the RFC 8620 §5.3 response envelope.
///
/// The six fields correspond to the six top-level result keys (`created`,
/// `updated`, `destroyed`, `notCreated`, `notUpdated`, `notDestroyed`).
/// Each `/set` handler builds these as it walks the request's `create`,
/// `update`, and `destroy` maps, then hands the bundle to
/// [`finalize_set_response`] for envelope construction.
///
/// Bundling the six accumulators into a struct (instead of six positional
/// parameters) keeps [`finalize_set_response`] under clippy's
/// `too_many_arguments` threshold and lets callers use struct-update syntax
/// (`SetAccumulators { created, ..Default::default() }`) when only a subset
/// of the maps is non-empty.
#[derive(Debug, Default)]
pub(crate) struct SetAccumulators {
    pub created: Map<String, Value>,
    pub updated: Map<String, Value>,
    pub destroyed: Vec<Value>,
    pub not_created: Map<String, Value>,
    pub not_updated: Map<String, Value>,
    pub not_destroyed: Map<String, Value>,
}

/// Build the final `/set` method response and re-fetch `newState` from the
/// backend if any mutation occurred.
///
/// All four `/set` handlers in this crate (`Calendar/set`, `CalendarEvent/set`,
/// `CalendarEventNotification/set`, `ParticipantIdentity/set`) end with the
/// same boilerplate: refresh the state token if `mutated`, then emit a
/// `HandlerOutput` wrapping the canonical RFC 8620 §5.3
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
pub(crate) async fn finalize_set_response<B, O>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    old_state: State,
    mutated: bool,
    acc: SetAccumulators,
) -> Result<HandlerOutput, JmapError>
where
    B: CalendarsBackend,
    O: JmapObject + Send + Sync,
{
    let new_state = if mutated {
        backend
            .get_state::<O>(caller, account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
    } else {
        old_state.clone()
    };

    let SetAccumulators {
        created,
        updated,
        destroyed,
        not_created,
        not_updated,
        not_destroyed,
    } = acc;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created":      if created.is_empty()      { Value::Null } else { Value::Object(created) },
        "updated":      if updated.is_empty()      { Value::Null } else { Value::Object(updated) },
        "destroyed":    if destroyed.is_empty()    { Value::Null } else { Value::Array(destroyed) },
        "notCreated":   if not_created.is_empty()  { Value::Null } else { Value::Object(not_created) },
        "notUpdated":   if not_updated.is_empty()  { Value::Null } else { Value::Object(not_updated) },
        "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
    })))
}

/// Resolve an Id-typed property value that may be a `#creationId`
/// reference (RFC 8620 §5.3) through [`jmap_server::resolve_creation_ref`].
///
/// A literal id is returned unchanged; its validation stays with the
/// caller.
///
/// # Errors
///
/// A boxed `invalidProperties` [`crate::SetError`] naming `property` when
/// the reference matches no record created earlier in the request.
pub(crate) fn resolve_ref(
    raw: &str,
    property: &str,
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<jmap_types::Id, Box<crate::SetError>> {
    jmap_server::resolve_creation_ref(raw, created_ids).ok_or_else(|| {
        Box::new(
            crate::SetError::new(crate::SetErrorType::InvalidProperties)
                .with_properties([property])
                .with_description(
                    "creation id reference matches no record created earlier in this request",
                ),
        )
    })
}

/// Replace `#creationId` references among the ids of the `property` map
/// (an `Id[Boolean]` set such as `addressBookIds`) in a create or patch
/// with the ids they name (RFC 8620 §5.3): the keys of a `property` object
/// and the id segment of a `property/<id>` patch path.
///
/// # Errors
///
/// As [`resolve_ref`], naming `property`.
pub(crate) fn resolve_map_key_refs(
    obj: &mut serde_json::Value,
    property: &str,
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<(), Box<crate::SetError>> {
    let Some(obj) = obj.as_object_mut() else {
        return Ok(());
    };
    if let Some(serde_json::Value::Object(ids)) = obj.get_mut(property) {
        let mut resolved = serde_json::Map::with_capacity(ids.len());
        for (key, value) in std::mem::take(ids) {
            let id = resolve_ref(&key, property, created_ids)?;
            resolved.insert(id.as_ref().to_owned(), value);
        }
        *ids = resolved;
    }
    let prefix = format!("{property}/");
    let ref_paths: Vec<String> = obj
        .keys()
        .filter(|k| k.strip_prefix(&prefix).is_some_and(|r| r.starts_with('#')))
        .cloned()
        .collect();
    for path in ref_paths {
        let id = resolve_ref(&path[prefix.len()..], property, created_ids)?;
        if let Some(value) = obj.remove(&path) {
            obj.insert(format!("{prefix}{}", id.as_ref()), value);
        }
    }
    Ok(())
}

/// Resolve an `onSuccessSetIsDefault` argument value to the target [`Id`].
///
/// Per draft-ietf-jmap-calendars-33 §3.3 / §4.3, a `#`-prefixed string
/// references a creation id from the same `/set` call; the resolved id is
/// the backend-assigned id for that creation. A bare string is treated as a
/// literal id. A bare string that is not an RFC 8620 §1.2 Id, or any
/// other JSON shape (number, object, array), yields `None`
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
        // Id::from: the backend assigned this id; it is not client input.
        Some(Id::from(assigned))
    } else {
        Id::new_validated(s).ok()
    }
}

/// Apply the response-mutation contract for `onSuccessSetIsDefault`
/// (draft-ietf-jmap-calendars-33 §3.3, §4.3).
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

    if let Some(previous) = result.previous_default.as_ref()
        && previous != new_default
    {
        merge_is_default(updated, previous.as_ref(), false);
        state_changed = true;
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
