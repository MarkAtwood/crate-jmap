//! Private helper utilities — re-exported from jmap_server.
use std::collections::HashSet;

use jmap_types::{Id, JmapError, JmapObject, State};
use serde_json::{Map, Value, json};

use crate::MailBackend;
use jmap_server::{HandlerOutput, server_fail_from_backend};

pub(crate) use jmap_server::{
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id, not_found_json,
    now_utc_string, serialize_value,
};

/// Sentinel blob ID set by the `Email/set` create and import handlers.
///
/// Backends MUST replace this value in [`crate::MailBackend::create_object`]
/// and [`crate::MailBackend::import_email`] with the real blob ID before
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
/// `SetError` carries only plain data (string error type plus optional
/// scalar fields) and its `Serialize` impl is derive-generated, so the
/// `.expect` below is provably unreachable: `serde_json::to_value` on a
/// derive-Serialize type with no custom logic cannot fail.
pub(crate) fn set_error_value(e: &crate::SetError) -> serde_json::Value {
    serde_json::to_value(e).expect("derive(Serialize) on plain data is infallible")
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
    created_ids: &std::collections::BTreeMap<Id, Id>,
) -> Result<Id, Box<crate::SetError>> {
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
/// of the maps is non-empty — e.g. `VacationResponse/set` never creates or
/// destroys, so only `updated` / `not_updated` are populated.
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
/// Five `/set` handlers in this crate (`Email/set`, `Mailbox/set`,
/// `Identity/set`, `SieveScript/set`, `VacationResponse/set`) end with the
/// same boilerplate: refresh the state token if `mutated`, then emit a
/// `HandlerOutput` wrapping the canonical RFC 8620 §5.3
/// envelope. Centralising it here keeps the five sites in lockstep — if a
/// future revision changes which keys are emitted (e.g. RFC 8620 §5.3.1 may
/// flip a key from `null` to omitted), all five handlers update at once.
///
/// `EmailSubmission/set` deliberately does NOT use this helper: it pushes
/// follow-up `Email/set` invocations from `onSuccessUpdateEmail` /
/// `onSuccessDestroyEmail` (RFC 8621 §7.4) and therefore needs to control
/// the returned `extra_invocations` rather than relying on the helper's
/// empty list.
///
/// The `O` type parameter is the JMAP object type for the state token
/// (e.g. `Email`, `Mailbox`); its only role is to disambiguate the
/// `get_state::<O>` call inside the helper.
///
/// Empty maps/arrays serialize as `null` (JMAP convention). The `Invocation`
/// vector is always empty for the five call sites — no `/set` call generates
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
    B: MailBackend,
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
