//! Private helper utilities shared across handler modules.

use jmap_types::{Id, JmapError, JmapObject, State};
use serde_json::{Map, Value, json};

use crate::SharingBackend;
use jmap_server::{HandlerOutput, server_fail_from_backend};

pub(crate) use jmap_server::{enforce_max_objects_in_set, extract_account_id};

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
/// Both `/set` handlers in this crate (`Principal/set`,
/// `ShareNotification/set`) end with the same boilerplate: refresh the state
/// token if `mutated`, then emit a `HandlerOutput` wrapping
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
pub(crate) async fn finalize_set_response<B, O>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    old_state: State,
    mutated: bool,
    acc: SetAccumulators,
) -> Result<HandlerOutput, JmapError>
where
    B: SharingBackend,
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

/// draft-ietf-jmap-metadata-04 checks a `Principal/set` handler applies to
/// one create object or update patch (`entry`) before the backend sees it.
///
/// - -04 §3: for a request whose `using` lacks
///   `urn:ietf:params:jmap:metadata`, "Foo/set MUST reject any write to
///   them with an "invalidProperties" SetError".
/// - -04 §3.2: on create, "A value of null for either property MUST be
///   rejected with an "invalidProperties" SetError".
///
/// The backend validates the rest (-04 §3.2 rules 1 to 4). Mirrors the
/// canonical `jmap-mail-server` helper (bd JMAP-s4kp.1).
///
/// # Errors
///
/// A boxed `invalidProperties` [`jmap_server::SetError`] naming the offending
/// key.
pub(crate) fn check_metadata_entry(
    entry: &Map<String, Value>,
    is_create: bool,
    using: &[String],
) -> Result<(), Box<jmap_server::SetError>> {
    let uses_metadata = using
        .iter()
        .any(|u| u == jmap_metadata_types::JMAP_METADATA_URI);
    for (key, value) in entry {
        if !is_metadata_key(key) {
            continue;
        }
        let description = if !uses_metadata {
            "writing metadata requires urn:ietf:params:jmap:metadata in using"
        } else if is_create && value.is_null() {
            "null is not allowed on create; use {}"
        } else {
            continue;
        };
        return Err(Box::new(
            jmap_server::SetError::new(jmap_server::SetErrorType::InvalidProperties)
                .with_properties([key.as_str()])
                .with_description(description),
        ));
    }
    Ok(())
}

/// Whether a `/set` key writes draft-ietf-jmap-metadata-04 `metadata` or
/// `privateMetadata`: the property itself or a patch path below it.
pub(crate) fn is_metadata_key(key: &str) -> bool {
    let root = key.split_once('/').map_or(key, |(r, _)| r);
    root == "metadata" || root == "privateMetadata"
}
