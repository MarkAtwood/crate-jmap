//! Private helper utilities shared across handler modules.

use std::collections::HashSet;

use jmap_types::{Id, Invocation, JmapError, JmapObject, State};
use serde_json::{json, Map, Value};

use crate::backend::ChatBackend;
use jmap_server::server_fail_from_backend;

pub(crate) use jmap_server::{
    enforce_max_objects_in_set, extract_account_id, not_found_json, now_utc_string, serialize_value,
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
pub(crate) async fn finalize_set_response<B, O>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    old_state: State,
    mutated: bool,
    acc: SetAccumulators,
) -> Result<(Value, Vec<Invocation>), JmapError>
where
    B: ChatBackend,
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
///
/// Takes [`UTCDate`] by reference rather than `&str` to enforce the
/// "ASCII-only, validated by [`UTCDate`] construction" precondition at
/// the type system. A plain `&str` parameter would compile under
/// hypothetical multi-byte UTF-8 input that intersected byte index 19;
/// the byte-index slice below would panic at that boundary. The
/// [`UTCDate`] newtype carries the ASCII invariant from its
/// construction site, so the slice cannot panic here.
///
/// [`UTCDate`]: jmap_types::UTCDate
pub(crate) fn iso8601_before(a: &jmap_types::UTCDate, b: &jmap_types::UTCDate) -> bool {
    let a_str: &str = a.as_ref();
    let b_str: &str = b.as_ref();
    let a_sec = &a_str[..a_str.len().min(19)];
    let b_sec = &b_str[..b_str.len().min(19)];
    a_sec < b_sec
}

/// Serialize a [`SetError`] to a JSON value for inclusion in
/// `notCreated`/`notUpdated`/`notDestroyed` maps.
///
/// `SetError` carries only plain data (string error type plus optional
/// scalar fields) and its `Serialize` impl is derive-generated, so the
/// `.expect` below is provably unreachable: `serde_json::to_value` on a
/// derive-Serialize type with no custom logic cannot fail.
pub(crate) fn set_error_value(e: &crate::backend::SetError) -> serde_json::Value {
    serde_json::to_value(e).expect("derive(Serialize) on plain data is infallible")
}

/// Count `Add*` ops in a [`SpacePatchOp`] sequence, returning
/// `(add_roles, add_members, add_channels, add_categories)`.
///
/// `Remove*` and `Update*` ops are not counted: the conservative
/// `existing + add` cap check ignores in-flight removes so the
/// resulting count is bounded even if ops are reordered by the
/// backend. The check rejects strictly more patches than strict
/// "final-count" enforcement; both are spec-conformant (the spec only
/// requires that the resulting count not exceed the cap).
///
/// Used both by `handle_space_set` for the defense-in-depth pre-flight
/// and by the reference `MemoryBackend`'s `apply_space_patch_impl` for
/// the backend-canonical enforcement (bd:JMAP-x2gd.44).
pub(crate) fn count_add_ops(ops: &[crate::backend::SpacePatchOp]) -> (u32, u32, u32, u32) {
    let mut add_roles: u32 = 0;
    let mut add_members: u32 = 0;
    let mut add_channels: u32 = 0;
    let mut add_categories: u32 = 0;
    for op in ops {
        match op {
            crate::backend::SpacePatchOp::AddRole(_) => add_roles = add_roles.saturating_add(1),
            crate::backend::SpacePatchOp::AddMember(_) => {
                add_members = add_members.saturating_add(1);
            }
            crate::backend::SpacePatchOp::AddChannel(_) => {
                add_channels = add_channels.saturating_add(1);
            }
            crate::backend::SpacePatchOp::AddCategory(_) => {
                add_categories = add_categories.saturating_add(1);
            }
            _ => {}
        }
    }
    (add_roles, add_members, add_channels, add_categories)
}
