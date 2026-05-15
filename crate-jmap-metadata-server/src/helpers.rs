//! Shared helper utilities for Metadata method handlers.
//!
//! Items here mirror the canonical extension-server helpers pattern
//! (see `jmap_mail_server::helpers` for the template per workspace
//! `AGENTS.md` "Canonical Templates"). `Metadata/set` is the primary
//! consumer of [`SetAccumulators`], [`finalize_set_response`], and
//! [`set_error_value`].

use jmap_metadata_types::Metadata;
use jmap_types::{Id, Invocation, JmapError, JmapObject, State};
use serde_json::{json, Map, Value};

use crate::backend::MetadataBackend;
use jmap_server::server_fail_from_backend;

pub(crate) use jmap_server::extract_account_id;

/// Per-`/set` accumulators emitted in the RFC 8620 §5.3 response envelope.
///
/// The six fields correspond to the six top-level result keys (`created`,
/// `updated`, `destroyed`, `notCreated`, `notUpdated`, `notDestroyed`).
/// The `/set` handler builds these as it walks the request's `create`,
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
/// Centralising this boilerplate keeps the call site in lockstep with the
/// other extension-server crates — if a future revision changes which keys
/// are emitted (e.g. RFC 8620 §5.3.1 may flip a key from `null` to
/// omitted), every server crate updates at once.
///
/// The `O` type parameter is the JMAP object type for the state token
/// (e.g. `Metadata`); its only role is to disambiguate the
/// `get_state::<O>` call inside the helper.
///
/// Empty maps/arrays serialize as `null` (JMAP convention). The `Invocation`
/// vector is always empty for the call site — no `/set` call generates
/// follow-up invocations today.
pub(crate) async fn finalize_set_response<B, O>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    old_state: State,
    mutated: bool,
    acc: SetAccumulators,
) -> Result<(Value, Vec<Invocation>), JmapError>
where
    B: MetadataBackend,
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

/// Serialize a [`SetError`](jmap_server::SetError) to a JSON value for inclusion in
/// `notCreated`/`notUpdated`/`notDestroyed` maps.
///
/// `SetError` carries only plain data (string error type plus optional
/// scalar fields) and its `Serialize` impl is derive-generated, so the
/// `.expect` below is provably unreachable: `serde_json::to_value` on a
/// derive-Serialize type with no custom logic cannot fail.
pub(crate) fn set_error_value(e: &jmap_server::SetError) -> serde_json::Value {
    serde_json::to_value(e).expect(INFALLIBLE_SERIALIZE_JUSTIFICATION)
}

/// Serialize a [`Metadata`] to a JSON value for inclusion in
/// `created`/`updated` maps in the `Metadata/set` response.
///
/// Sibling helper to [`set_error_value`]: factors out the
/// "derive-Serialize is infallible" justification so it lives in
/// exactly one place. `Metadata` is a `#[derive(Serialize)]`-only type
/// (no custom serializer logic), so `serde_json::to_value` on it cannot
/// fail in practice — the `.expect` is a documentation marker rather
/// than a runtime path.
pub(crate) fn metadata_value(m: &Metadata) -> serde_json::Value {
    serde_json::to_value(m).expect(INFALLIBLE_SERIALIZE_JUSTIFICATION)
}

/// Shared `.expect` message for plain-data Serialize sites in this
/// crate. Hoisted out of duplicated string literals at the four call
/// sites that previously inlined the same justification (bd:JMAP-826m.50,
/// bd:JMAP-826m.29).
///
/// Note: this is the message a future panic would surface. Keep it
/// short and reader-actionable; the rustdoc above each call site is
/// where the "why this is unreachable" rationale belongs.
const INFALLIBLE_SERIALIZE_JUSTIFICATION: &str =
    "derive(Serialize) on plain data is infallible";

/// Build the `serverFail` response value for the
/// `BackendSetError`-non-exhaustive catch-all in `/set` handlers.
///
/// `BackendSetError` is `#[non_exhaustive]` (workspace foundation
/// convention) so every `match` on it carries an `Err(_)` catch-all
/// that fires only when a future variant lands in `jmap-server`
/// without a matching update here. The catch-all previously emitted
/// a generic `"unhandled backend error variant"` description that
/// gave operators triaging a production `serverFail` no signal about
/// WHICH variant fired.
///
/// This helper surfaces the variant via `{e:?}` (`BackendSetError`
/// derives `Debug`) so the wire description reads e.g.
/// `"unhandled backend error variant: SomeFutureVariant { .. }"`.
/// RFC 8620 §5.3 declares `description` as non-localised
/// debugging-grade text, which is the right channel for this
/// information.
///
/// Tracks bd:JMAP-826m.30 (de-duplication) and bd:JMAP-826m.36
/// (actionable variant name).
pub(crate) fn unhandled_backend_set_error<E: std::fmt::Debug>(
    e: &jmap_server::BackendSetError<E>,
) -> Value {
    json!({
        "type": "serverFail",
        "description": format!("unhandled backend error variant: {e:?}"),
    })
}
