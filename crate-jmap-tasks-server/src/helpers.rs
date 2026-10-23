//! Private helper utilities shared across handler modules.

use jmap_types::{Id, JmapError, JmapObject, State};
use serde_json::{Map, Value, json};

use crate::TasksBackend;
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
/// All three `/set` handlers in this crate (`TaskList/set`, `Task/set`,
/// `TaskNotification/set`) end with the same boilerplate: refresh the state
/// token if `mutated`, then emit a `HandlerOutput` wrapping
/// the canonical RFC 8620 §5.3 envelope. Centralising it here keeps the three
/// sites in lockstep — if a future revision changes which keys are emitted
/// (e.g. RFC 8620 §5.3.1 may flip a key from `null` to omitted), all three
/// handlers update at once.
///
/// The `O` type parameter is the JMAP object type for the state token
/// (e.g. `Task`, `TaskList`, `TaskNotification`); its only role is to
/// disambiguate the `get_state::<O>` call inside the helper.
///
/// Empty maps/arrays serialize as `null` (JMAP convention). The `Invocation`
/// vector is always empty for the three call sites — no `/set` call generates
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
    B: TasksBackend,
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
    jmap_tasks_types::TaskComparator => jmap_tasks_types::TaskComparatorProperty::Other,
    jmap_tasks_types::TaskNotificationComparator =>
        jmap_tasks_types::TaskNotificationComparatorProperty::Other,
);
