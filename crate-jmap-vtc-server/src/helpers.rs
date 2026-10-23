//! Private helper utilities for the handler modules.

use jmap_types::{GetObject, Id, JmapError, JmapObject, State, UTCDate};
use serde_json::{Map, Value, json};

use crate::{SetError, SetErrorType, VtcBackend};
use jmap_server::{HandlerOutput, server_fail_from_backend, server_fail_value_from_backend};

pub(crate) use jmap_server::{enforce_max_objects_in_set, extract_account_id};

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
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<jmap_types::Id, Box<crate::SetError>> {
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
) -> Result<(), Box<crate::SetError>> {
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

/// An `invalidProperties` SetError naming `properties` (none when empty).
pub(crate) fn invalid(properties: &[&str], description: &str) -> Value {
    let err = SetError::new(SetErrorType::InvalidProperties).with_description(description);
    serde_json::Value::from(&if properties.is_empty() {
        err
    } else {
        err.with_properties(properties.iter().copied())
    })
}

/// The `invalidProperties` SetError naming `state` for a transition that
/// [`VtcBackend::update_object_if`] refused: the stored state changed
/// after the handler read it, and the draft does not list the transition
/// from the new state.
pub(crate) fn state_refused() -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties(["state"])
        .with_description("this state transition is not permitted from the current state")
}

/// A `forbidden` SetError.
pub(crate) fn forbidden(description: &str) -> Value {
    serde_json::Value::from(&SetError::new(SetErrorType::Forbidden).with_description(description))
}

/// A `notFound` SetError.
pub(crate) fn not_found() -> Value {
    serde_json::Value::from(&SetError::new(SetErrorType::NotFound))
}

/// Fetch one object of type `O`, or `None` if the backend does not have it.
pub(crate) async fn fetch_one<O: GetObject + Send + Sync, B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<Option<O>, Value> {
    backend
        .get_objects::<O>(caller, account_id, Some(std::slice::from_ref(id)), None)
        .await
        .map(|(found, _)| found.into_iter().next())
        .map_err(|e| server_fail_value_from_backend(&e))
}

/// The sortable part of a UTCDate: `YYYY-MM-DDTHH:MM:SS`.
///
/// Fractional seconds are dropped so that values with and without them
/// compare chronologically; for well-formed UTC timestamps the ASCII
/// order of this prefix is chronological order.
pub(crate) fn utc_key(d: &UTCDate) -> &str {
    let s: &str = d.as_ref();
    s.get(..19).unwrap_or(s)
}

/// Use `default` as the `/query` `sort` when the request gives none
/// (absent, `null` or `[]`), so every backend sees the draft's default.
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
/// §5.5 `unsupportedSort`. A value that is not an array is left for the
/// typed decode to reject.
/// Decode the `filter` argument without consuming it, for a pre-check that
/// runs before the generic `/query` or `/queryChanges` handler. Delegates to
/// [`jmap_server::filter_arg`]: RFC 8620 §5.5 types `filter` as
/// "FilterOperator|FilterCondition|null", so a non-object is
/// `invalidArguments`; an object that fails to decode is `unsupportedFilter`.
pub(crate) fn peek_filter<T: serde::de::DeserializeOwned>(
    args: &Value,
) -> Result<Option<T>, JmapError> {
    let mut probe = Map::new();
    if let Some(f) = args.get("filter") {
        probe.insert("filter".into(), f.clone());
    }
    jmap_server::filter_arg(&mut probe)
}

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
    jmap_vtc_types::VtcCallComparator => jmap_vtc_types::VtcCallSortProperty::Other,
    jmap_vtc_types::VtcParticipantComparator => jmap_vtc_types::VtcParticipantSortProperty::Other,
    jmap_vtc_types::VtcMediaComparator => jmap_vtc_types::VtcMediaSortProperty::Other,
);

/// Per-`/set` accumulators emitted in the RFC 8620 §5.3 response envelope.
#[derive(Debug, Default)]
pub(crate) struct SetAccumulators {
    pub created: Map<String, Value>,
    pub updated: Map<String, Value>,
    pub destroyed: Vec<Value>,
    pub not_created: Map<String, Value>,
    pub not_updated: Map<String, Value>,
    pub not_destroyed: Map<String, Value>,
}

/// Build the final `/set` response, re-reading `newState` when anything
/// was mutated. Empty maps and arrays serialize as `null`.
pub(crate) async fn finalize_set_response<B, O>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    old_state: State,
    mutated: bool,
    acc: SetAccumulators,
) -> Result<HandlerOutput, JmapError>
where
    B: VtcBackend,
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
