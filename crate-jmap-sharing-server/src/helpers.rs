//! Private helper utilities shared across handler modules.

use jmap_types::{Id, Invocation, JmapError, JmapObject, State};
use serde_json::{json, Map, Value};

use crate::backend::SharingBackend;

pub(crate) use jmap_server::extract_account_id;

/// Serialize a [`SetError`] to a JSON value for inclusion in
/// `notCreated`/`notUpdated`/`notDestroyed` maps.
///
/// Falls back to a `serverFail` object on the extremely unlikely event that
/// `SetError`'s `Serialize` impl fails.
pub(crate) fn set_error_value(e: &crate::backend::SetError) -> serde_json::Value {
    serde_json::to_value(e).expect("derive(Serialize) on plain data is infallible")
}

/// Build the final `/set` method response and re-fetch `newState` from the
/// backend if any mutation occurred.
///
/// Both `/set` handlers in this crate (`Principal/set`,
/// `ShareNotification/set`) end with the same boilerplate: refresh the state
/// token if `mutated`, then emit a `(Value, Vec<Invocation>)` tuple wrapping
/// the canonical RFC 8620 §5.3 envelope. Centralising it here keeps the two
/// sites in lockstep — if a future revision changes which keys are emitted
/// (e.g. RFC 8620 §5.3.1 may flip a key from `null` to omitted), both
/// handlers update at once.
///
/// The `O` type parameter is the JMAP object type for the state token
/// (e.g. `Principal`, `ShareNotification`); its only role is to disambiguate
/// the `get_state::<O>` call inside the helper.
///
/// Empty maps/arrays serialize as `null` (JMAP convention). The `Invocation`
/// vector is always empty for the two call sites — no `/set` call generates
/// follow-up invocations today.
//
// The clippy::too_many_arguments lint fires here because the six accumulator
// collections (`created`, `updated`, `destroyed_list`, `not_created`,
// `not_updated`, `not_destroyed`) are passed individually rather than bundled
// into a builder struct. Bundling them is the natural follow-up but would
// force a textual rename of references across the two /set handlers — a
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
    B: SharingBackend,
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
