//! Private helper utilities shared across handler modules.

use jmap_types::{Id, JmapError, JmapObject, State};
use serde_json::{Map, Value, json};

use crate::ContactsBackend;
use jmap_server::{HandlerOutput, server_fail_from_backend};

pub(crate) use jmap_server::{enforce_max_objects_in_set, extract_account_id};

/// Resolve an Id-typed property value that may be a `#creationId`
/// reference (RFC 8620 §5.3) through [`jmap_server::resolve_creation_ref`].
///
/// A literal id is checked with [`jmap_server::parse_wire_id`] (RFC 8620
/// §1.2) and returned unchanged.
///
/// # Errors
///
/// A boxed `invalidProperties` [`jmap_server::SetError`] naming `property` when
/// a literal is not a valid Id, or when the reference matches no record
/// created earlier in the request.
pub(crate) fn resolve_ref(
    raw: &str,
    property: &str,
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<jmap_types::Id, Box<jmap_server::SetError>> {
    if !raw.starts_with('#') {
        return jmap_server::parse_wire_id(raw, property);
    }
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

/// Replace `#creationId` references among the ids of the `property` map
/// (an `Id[Boolean]` set such as `addressBookIds`) in a create or patch
/// with the ids they name (RFC 8620 §5.3): the keys of a `property` object
/// and the id segment of a `property/<id>` patch path. A literal key or
/// path segment that is not an RFC 8620 §1.2 Id is rejected.
///
/// # Errors
///
/// As [`resolve_ref`], naming `property`.
pub(crate) fn resolve_map_key_refs(
    obj: &mut serde_json::Value,
    property: &str,
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<(), Box<jmap_server::SetError>> {
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
        .filter(|k| k.starts_with(&prefix))
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
/// Both `/set` handlers in this crate (`AddressBook/set`, `ContactCard/set`)
/// end with the same boilerplate: refresh the state token if `mutated`, then
/// emit a `HandlerOutput` wrapping the canonical RFC 8620
/// §5.3 envelope. Centralising it here keeps the two sites in lockstep — if
/// a future revision changes which keys are emitted (e.g. RFC 8620 §5.3.1
/// may flip a key from `null` to omitted), both handlers update at once.
///
/// The `O` type parameter is the JMAP object type for the state token
/// (e.g. `AddressBook`, `ContactCard`); its only role is to disambiguate the
/// `get_state::<O>` call inside the helper.
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
    B: ContactsBackend,
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

/// Reject a `/query` or `/queryChanges` `sort` naming a property this server
/// does not sort on: RFC 8620 §5.5 `unsupportedSort` ("The sort is
/// syntactically valid, but includes a property the server does not support
/// sorting on"). A non-array `sort` or a comparator that fails to decode is
/// not syntactically valid, so it is left for the typed decode to reject as
/// `invalidArguments`.
pub(crate) fn reject_unsupported_sort<C: serde::de::DeserializeOwned + KnownSort>(
    sort: Option<&Value>,
) -> Result<(), JmapError> {
    if let Some(Value::Array(comparators)) = sort {
        for c in comparators {
            if matches!(C::deserialize(c), Ok(c) if !c.known()) {
                return Err(JmapError::unsupported_sort());
            }
        }
    }
    Ok(())
}

/// A comparator whose `property` decodes an unknown name to `Other`
/// (bd JMAP-wf92.11), so [`reject_unsupported_sort`] must check it.
pub(crate) trait KnownSort {
    /// `false` when `property` is `Other`, a name the draft does not list.
    fn known(&self) -> bool;
}

macro_rules! known_sort {
    ($($cmp:ty => $prop:path),+ $(,)?) => {$(
        impl KnownSort for $cmp {
            fn known(&self) -> bool {
                !matches!(self.property, $prop(_))
            }
        }
    )+};
}

known_sort!(
    jmap_contacts_types::ContactCardComparator => jmap_contacts_types::ContactCardComparatorProperty::Other,
);

/// draft-ietf-jmap-metadata-04 checks a `Foo/set` handler applies to one
/// create object or update patch (`entry`) before the backend sees it.
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
