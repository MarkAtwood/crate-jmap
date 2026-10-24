//! Shared helper utilities for FileNode method handlers.

use jmap_types::{Id, JmapError, JmapObject, State};
use serde_json::{Map, Value, json};

use crate::FileNodeBackend;
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
/// The single `/set` handler in this crate (`FileNode/set`) ends with the
/// same boilerplate as its siblings: refresh the state token if `mutated`,
/// then emit a `HandlerOutput` wrapping the canonical RFC
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
    caller: &B::CallerCtx,
    account_id: &Id,
    old_state: State,
    mutated: bool,
    acc: SetAccumulators,
) -> Result<HandlerOutput, JmapError>
where
    B: FileNodeBackend,
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
/// A boxed `invalidProperties` [`jmap_server::SetError`] naming `property` when
/// the reference matches no record created earlier in the request.
pub(crate) fn resolve_ref(
    raw: &str,
    property: &str,
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<jmap_types::Id, Box<jmap_server::SetError>> {
    jmap_server::resolve_creation_ref(raw, created_ids).ok_or_else(|| {
        Box::new(
            jmap_server::SetError::new(jmap_server::SetErrorType::InvalidProperties)
                .with_properties([property])
                .with_description(
                    "creation id reference matches no record created earlier in this request",
                ),
        )
    })
}

/// Rewrite each top-level `props` value of `obj`, a create or an update
/// patch, that is a `#creationId` reference to the id it names, through
/// [`resolve_ref`]. Other values are left for the caller to validate.
///
/// # Errors
///
/// As [`resolve_ref`], for the first unresolvable property.
pub(crate) fn resolve_refs(
    obj: &mut serde_json::Value,
    props: &[&str],
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<(), Box<jmap_server::SetError>> {
    let Some(m) = obj.as_object_mut() else {
        return Ok(());
    };
    for p in props {
        if let Some(serde_json::Value::String(raw)) = m.get_mut(*p) {
            *raw = resolve_ref(raw, p, created_ids)?.as_ref().to_owned();
        }
    }
    Ok(())
}

/// Order a `/set` create map so each create whose `property` is `#k`
/// comes after the create `k` of the same call, as RFC 8620 §5.3 requires
/// for "records with references to the same type". A cycle or a
/// self-reference keeps its map order and fails reference resolution.
///
/// ponytail: repeated passes, O(n^2) in the create count, which
/// `maxObjectsInSet` bounds; a topological sort is the upgrade path.
pub(crate) fn parents_first(
    creates: serde_json::Map<String, serde_json::Value>,
    property: &str,
) -> Vec<(String, serde_json::Value)> {
    let keys: std::collections::BTreeSet<String> = creates.keys().cloned().collect();
    let mut pending: Vec<(String, serde_json::Value)> = creates.into_iter().collect();
    let mut ordered: Vec<(String, serde_json::Value)> = Vec::with_capacity(pending.len());
    loop {
        let before = pending.len();
        let (waiting, ready): (Vec<_>, Vec<_>) = std::mem::take(&mut pending)
            .into_iter()
            .partition(|(create_id, props)| {
                props
                    .get(property)
                    .and_then(serde_json::Value::as_str)
                    .and_then(|p| p.strip_prefix('#'))
                    .is_some_and(|parent| {
                        parent != create_id.as_str()
                            && keys.contains(parent)
                            && !ordered.iter().any(|(done, _)| done.as_str() == parent)
                    })
            });
        ordered.extend(ready);
        pending = waiting;
        if pending.is_empty() || pending.len() == before {
            ordered.extend(pending);
            return ordered;
        }
    }
}

/// draft-ietf-jmap-metadata-04 checks a `FileNode/set` handler applies to
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

/// Check that every key of the `property` map (an `Id[...]` such as
/// `shareWith`) in a create or patch, and the id segment of every
/// `property/<id>` patch path, is an RFC 8620 §1.2 Id.
///
/// # Errors
///
/// A boxed `invalidProperties` [`jmap_server::SetError`] naming `property`.
pub(crate) fn check_map_key_ids(
    entry: &Map<String, Value>,
    property: &str,
) -> Result<(), Box<jmap_server::SetError>> {
    let prefix = format!("{property}/");
    let map_keys = entry
        .get(property)
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|m| m.keys().map(String::as_str));
    let path_ids = entry
        .keys()
        .filter_map(|k| k.strip_prefix(&prefix))
        .map(|rest| rest.split_once('/').map_or(rest, |(id, _)| id));
    for raw in map_keys.chain(path_ids) {
        jmap_server::parse_wire_id(raw, property)?;
    }
    Ok(())
}

/// Whether a `/set` key writes draft-ietf-jmap-metadata-04 `metadata` or
/// `privateMetadata`: the property itself or a patch path below it.
pub(crate) fn is_metadata_key(key: &str) -> bool {
    let root = key.split_once('/').map_or(key, |(r, _)| r);
    root == "metadata" || root == "privateMetadata"
}
