//! Private helper utilities shared across handler modules.

use std::collections::HashSet;

use jmap_types::{Id, JmapError, JmapObject, State};
use serde_json::{Map, Value, json};

use crate::ChatBackend;
use jmap_server::{HandlerOutput, server_fail_from_backend};

pub(crate) use jmap_server::{
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id, not_found_json,
    now_utc_string, optional_arg, serialize_value,
};

/// JSON type that a client-supplied optional `/set` create property must
/// carry when present and non-null.
#[derive(Clone, Copy)]
pub(crate) enum PropType {
    /// `String`, and the String-shaped `Id` and `UTCDate`. The caller
    /// still checks any finer format.
    Str,
    /// `Boolean`.
    Bool,
    /// RFC 8620 §1.3 `UnsignedInt`: an integer in `0..=2^53-1`.
    UnsignedInt,
    /// A JSON array; the caller validates the elements.
    Array,
}

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

/// The `metadata` and `privateMetadata` values [`create_metadata`] returns.
pub(crate) type MetadataMaps = (Option<Map<String, Value>>, Option<Map<String, Value>>);

/// The `metadata` and `privateMetadata` values of a create object, to
/// store on the new object (draft-ietf-jmap-metadata-04 §3.2: "each
/// property's value MUST be a complete JSON object"). Call after
/// [`check_metadata_entry`], which rejects `null`.
///
/// # Errors
///
/// The `invalidProperties` SetError value naming a property whose value
/// is not an object.
pub(crate) fn create_metadata(entry: &Value) -> Result<MetadataMaps, Value> {
    let take = |property: &str| match entry.get(property) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(m)) => Ok(Some(m.clone())),
        Some(_) => Err(Value::from(
            &jmap_server::SetError::new(jmap_server::SetErrorType::InvalidProperties)
                .with_properties([property])
                .with_description("value must be an object"),
        )),
    };
    Ok((take("metadata")?, take("privateMetadata")?))
}

/// Largest RFC 8620 §1.3 `UnsignedInt`: 2^53-1.
const MAX_UNSIGNED_INT: u64 = (1 << 53) - 1;

/// Return an `invalidProperties` SetError naming every property in
/// `props` whose value in the create object `obj` has the wrong JSON type,
/// or `None` when all of them are absent, `null`, or correctly typed.
///
/// RFC 8620 §5.3 defines `invalidProperties` as "The record given is
/// invalid in some way", and its `properties` as the list of invalid
/// properties. A handler that read these properties with
/// `get(k).and_then(Value::as_str)` alone would silently treat a
/// wrongly-typed value as absent. `null` is read as absent, so the
/// property takes its default.
pub(crate) fn mistyped_create_props(obj: &Value, props: &[(&str, PropType)]) -> Option<Value> {
    let bad: Vec<&str> = props
        .iter()
        .filter(|(key, ty)| match obj.get(*key) {
            None | Some(Value::Null) => false,
            Some(v) => !match ty {
                PropType::Str => v.is_string(),
                PropType::Bool => v.is_boolean(),
                PropType::UnsignedInt => v.as_u64().is_some_and(|n| n <= MAX_UNSIGNED_INT),
                PropType::Array => v.is_array(),
            },
        })
        .map(|(key, _)| *key)
        .collect();
    (!bad.is_empty()).then(|| json!({ "type": "invalidProperties", "properties": bad }))
}

/// Use `default` as the `/query` `sort` when the request gives none
/// (absent, `null` or `[]`).
///
/// RFC 8620 §5.5 leaves the order of an unsorted query to the server;
/// draft-atwood-jmap-chat-00 fixes a "Default sort" for each chat
/// `/query`, so the handler supplies it and every backend sees it.
pub(crate) fn default_sort(args: &mut Map<String, Value>, default: Value) {
    let unsorted = match args.get("sort") {
        None | Some(Value::Null) => true,
        Some(Value::Array(a)) => a.is_empty(),
        Some(_) => false,
    };
    if unsorted {
        args.insert("sort".to_owned(), default);
    }
}

/// Reject a `/query` `sort` property this server does not define: RFC 8620
/// §5.5 `unsupportedSort`.
///
/// `C` is the object's comparator type. A value that is not an array is
/// left for the typed decode to reject. Unknown `filter` keys need no check
/// here: the chat filter types carry `#[serde(deny_unknown_fields)]`, so
/// the typed decode fails and maps to `unsupportedFilter`.
pub(crate) fn reject_unsupported_sort<C: serde::de::DeserializeOwned + KnownSort>(
    sort: Option<&Value>,
) -> Result<(), JmapError> {
    if let Some(Value::Array(comparators)) = sort {
        for c in comparators {
            if !<C as serde::Deserialize>::deserialize(c).is_ok_and(|c| c.known()) {
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
    jmap_chat_types::ChatContactComparator => jmap_chat_types::ChatContactComparatorProperty::Other,
    jmap_chat_types::ChatComparator => jmap_chat_types::ChatComparatorProperty::Other,
    jmap_chat_types::MessageComparator => jmap_chat_types::MessageComparatorProperty::Other,
    jmap_chat_types::SpaceComparator => jmap_chat_types::SpaceComparatorProperty::Other,
    jmap_chat_types::CustomEmojiComparator => jmap_chat_types::CustomEmojiComparatorProperty::Other,
);

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
/// `HandlerOutput` wrapping the canonical RFC 8620 §5.3
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
) -> Result<HandlerOutput, JmapError>
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
/// patch, that is a `#creationId` reference, or an array element that is
/// one, to the id it names, through [`resolve_ref`]. Other values are left
/// for the caller to validate.
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
        let values: Vec<&mut serde_json::Value> = match m.get_mut(*p) {
            Some(serde_json::Value::Array(items)) => items.iter_mut().collect(),
            Some(v) => vec![v],
            None => Vec::new(),
        };
        for v in values {
            if let serde_json::Value::String(raw) = v {
                *raw = resolve_ref(raw, p, created_ids)?.as_ref().to_owned();
            }
        }
    }
    Ok(())
}

/// Rewrite each string `field` of an object entry in the array property
/// `prop` of `obj`, a create or an update patch, that is a `#creationId`
/// reference to the id it names, through [`resolve_ref`]; a failure names
/// `named`. For example `("addMembers", "id", "addMembers")` resolves
/// `addMembers[].id`. Other values are left for the caller to validate.
///
/// # Errors
///
/// As [`resolve_ref`], for the first unresolvable entry.
pub(crate) fn resolve_entry_refs(
    obj: &mut serde_json::Value,
    entries: &[(&str, &str, &str)],
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<(), Box<jmap_server::SetError>> {
    for &(prop, field, named) in entries {
        let Some(serde_json::Value::Array(items)) = obj.get_mut(prop) else {
            continue;
        };
        for item in items {
            if let Some(serde_json::Value::String(raw)) = item.get_mut(field) {
                *raw = resolve_ref(raw, named, created_ids)?.as_ref().to_owned();
            }
        }
    }
    Ok(())
}

/// A copy of the create `obj` with its `props` resolved by
/// [`resolve_refs`], for a handler that iterates the create map by
/// reference.
///
/// # Errors
///
/// As [`resolve_refs`].
pub(crate) fn resolved_create(
    obj: &serde_json::Value,
    props: &[&str],
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<serde_json::Value, Box<jmap_server::SetError>> {
    let mut obj = obj.clone();
    resolve_refs(&mut obj, props, created_ids)?;
    Ok(obj)
}

/// Order a `/set` create map so each create whose `props` value is `#k`
/// comes after the create `k` of the same call, as RFC 8620 §5.3 requires
/// for "records with references to the same type". A cycle or a
/// self-reference keeps its map order and fails reference resolution.
///
/// ponytail: repeated passes, O(n^2) in the create count, which
/// `maxObjectsInSet` bounds; a topological sort is the upgrade path.
pub(crate) fn parents_first<'a>(
    creates: &'a serde_json::Map<String, serde_json::Value>,
    props: &[&str],
) -> Vec<(&'a String, &'a serde_json::Value)> {
    let mut pending: Vec<(&String, &serde_json::Value)> = creates.iter().collect();
    let mut ordered: Vec<(&String, &serde_json::Value)> = Vec::with_capacity(pending.len());
    loop {
        let before = pending.len();
        pending.retain(|&(create_id, obj)| {
            let waits = props.iter().any(|p| {
                obj.get(*p)
                    .and_then(serde_json::Value::as_str)
                    .and_then(|r| r.strip_prefix('#'))
                    .is_some_and(|parent| {
                        parent != create_id.as_str()
                            && creates.contains_key(parent)
                            && !ordered.iter().any(|(done, _)| done.as_str() == parent)
                    })
            });
            if !waits {
                ordered.push((create_id, obj));
            }
            waits
        });
        if pending.is_empty() || pending.len() == before {
            ordered.extend(pending);
            return ordered;
        }
    }
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
pub(crate) fn count_add_ops(ops: &[crate::SpacePatchOp]) -> (u32, u32, u32, u32) {
    let mut add_roles: u32 = 0;
    let mut add_members: u32 = 0;
    let mut add_channels: u32 = 0;
    let mut add_categories: u32 = 0;
    for op in ops {
        match op {
            crate::SpacePatchOp::AddRole(_) => add_roles = add_roles.saturating_add(1),
            crate::SpacePatchOp::AddMember(_) => {
                add_members = add_members.saturating_add(1);
            }
            crate::SpacePatchOp::AddChannel(_) => {
                add_channels = add_channels.saturating_add(1);
            }
            crate::SpacePatchOp::AddCategory(_) => {
                add_categories = add_categories.saturating_add(1);
            }
            _ => {}
        }
    }
    (add_roles, add_members, add_channels, add_categories)
}
