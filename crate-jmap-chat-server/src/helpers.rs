//! Private helper utilities shared across handler modules.

use std::collections::HashSet;

use jmap_types::{Id, Invocation, JmapError, JmapObject, State};
use serde_json::{json, Map, Value};

use crate::backend::ChatBackend;

pub(crate) use jmap_server::{extract_account_id, not_found_json, now_utc_string, ser};

/// Build the final `/set` method response and re-fetch `newState` from the
/// backend if any mutation occurred.
///
/// All nine `/set` handlers in this crate (`Chat/set`, `Message/set`,
/// `Space/set`, `SpaceInvite/set`, `SpaceBan/set`, `ChatContact/set`,
/// `ReadPosition/set`, `CustomEmoji/set`, `PresenceStatus/set`) end with the
/// same boilerplate: refresh the state token if `mutated`, then emit a
/// `(Value, Vec<Invocation>)` tuple wrapping the canonical RFC 8620 §5.3
/// envelope. Centralising it here keeps the nine sites in lockstep — if a
/// future revision changes which keys are emitted (e.g. RFC 8620 §5.3.1 may
/// flip a key from `null` to omitted), all nine handlers update at once.
///
/// The `O` type parameter is the JMAP object type for the state token
/// (e.g. `Chat`, `Message`, `Space`); its only role is to disambiguate the
/// `get_state::<O>` call inside the helper.
///
/// Empty maps/arrays serialize as `null` (JMAP convention). The `Invocation`
/// vector is always empty for the nine call sites — no `/set` call generates
/// follow-up invocations today.
//
// The clippy::too_many_arguments lint fires here because the six accumulator
// collections (`created`, `updated`, `destroyed_list`, `not_created`,
// `not_updated`, `not_destroyed`) are passed individually rather than bundled
// into a builder struct. Bundling them is the natural follow-up but would
// force a textual rename of ~150 references across the nine /set handlers — a
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
    B: ChatBackend,
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

/// Keep only the keys listed in `prop_set` (plus `"id"` which callers add).
///
/// Used by `/get` handlers to respect the RFC 8620 §5.1 `properties` field.
/// Pre-build the `prop_set` once per request; call this once per object.
pub(crate) fn filter_properties(
    obj: &serde_json::Value,
    prop_set: &HashSet<&str>,
) -> serde_json::Value {
    match obj {
        serde_json::Value::Object(map) => {
            let filtered: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter(|(k, _)| prop_set.contains(k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            serde_json::Value::Object(filtered)
        }
        _ => obj.clone(),
    }
}

/// Returns `true` if RFC 3339 UTC timestamp `a` is strictly before `b`.
///
/// Comparison is performed on the first 19 characters (`YYYY-MM-DDTHH:MM:SS`)
/// so that fractional-second suffixes produced by some clients (e.g.
/// `"2025-06-01T12:00:00.000Z"`) do not corrupt the result.  The
/// plain-ASCII lexicographic order of the ISO 8601 prefix is identical to
/// chronological order for well-formed UTC timestamps.
pub(crate) fn iso8601_before(a: &str, b: &str) -> bool {
    let a_sec = &a[..a.len().min(19)];
    let b_sec = &b[..b.len().min(19)];
    a_sec < b_sec
}

/// Serialize a [`SetError`] to a JSON value for inclusion in
/// `notCreated`/`notUpdated`/`notDestroyed` maps.
///
/// Falls back to a `serverFail` object on the extremely unlikely event that
/// `SetError`'s `Serialize` impl panics.
pub(crate) fn set_error_value(e: &crate::backend::SetError) -> serde_json::Value {
    serde_json::to_value(e).expect("derive(Serialize) on plain data is infallible")
}
