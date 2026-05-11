//! Shared helper utilities for FileNode method handlers.

use jmap_types::{Id, Invocation, JmapError, JmapObject, State};
use serde_json::{json, Map, Value};

use crate::backend::FileNodeBackend;

pub(crate) use jmap_server::extract_account_id;

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
/// The single `/set` handler in this crate (`FileNode/set`) ends with the
/// same boilerplate as its siblings: refresh the state token if `mutated`,
/// then emit a `(Value, Vec<Invocation>)` tuple wrapping the canonical RFC
/// 8620 §5.3 envelope. Centralising it here keeps the call site in lockstep
/// with the other extension-server crates — if a future revision changes
/// which keys are emitted (e.g. RFC 8620 §5.3.1 may flip a key from `null`
/// to omitted), every server crate updates at once.
///
/// `FileNode/copy` deliberately does NOT use this helper: it is a `/copy`
/// method (draft-ietf-jmap-filenode-13 §3.2.4) with a different envelope
/// shape — it carries `fromAccountId` and only `created`/`notCreated`,
/// not the full six-bucket `/set` envelope.
///
/// The `O` type parameter is the JMAP object type for the state token
/// (e.g. `FileNode`); its only role is to disambiguate the
/// `get_state::<O>` call inside the helper.
///
/// Empty maps/arrays serialize as `null` (JMAP convention). The `Invocation`
/// vector is always empty for the call site — no `/set` call generates
/// follow-up invocations today.
pub(crate) async fn finalize_set_response<B, O>(
    backend: &B,
    account_id: &Id,
    old_state: State,
    mutated: bool,
    acc: SetAccumulators,
) -> Result<(Value, Vec<Invocation>), JmapError>
where
    B: FileNodeBackend,
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

    let SetAccumulators {
        created,
        updated,
        destroyed,
        not_created,
        not_updated,
        not_destroyed,
    } = acc;

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldState": old_state.as_ref(),
            "newState": new_state.as_ref(),
            "created":      if created.is_empty()      { Value::Null } else { Value::Object(created) },
            "updated":      if updated.is_empty()      { Value::Null } else { Value::Object(updated) },
            "destroyed":    if destroyed.is_empty()    { Value::Null } else { Value::Array(destroyed) },
            "notCreated":   if not_created.is_empty()  { Value::Null } else { Value::Object(not_created) },
            "notUpdated":   if not_updated.is_empty()  { Value::Null } else { Value::Object(not_updated) },
            "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
        }),
        vec![],
    ))
}

/// Serialize a [`SetError`] to a JSON value for inclusion in
/// `notCreated`/`notUpdated`/`notDestroyed` maps.
///
/// Falls back to a `serverFail` object on the unlikely event that
/// `SetError`'s `Serialize` impl fails.
pub(crate) fn set_error_value(e: &jmap_server::SetError) -> serde_json::Value {
    serde_json::to_value(e).expect("derive(Serialize) on plain data is infallible")
}
