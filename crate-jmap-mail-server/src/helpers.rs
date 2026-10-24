//! Private helper utilities — re-exported from jmap_server.
use std::collections::HashSet;

use jmap_types::{Id, JmapError, JmapObject, State};
use serde_json::{Map, Value, json};

use crate::MailBackend;
use jmap_server::{HandlerOutput, server_fail_from_backend};

pub(crate) use jmap_server::{
    MetadataProjection, enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    not_found_json, now_utc_string, reject_metadata_filter_without_capability, serialize_value,
    uses_metadata,
};

/// Return only the keys in `prop_set` from the JSON object `obj`.
///
/// Used by all `*/get` handlers to enforce the RFC 8620 §5.1 rule that when
/// `properties` is specified the server MUST return only those fields (plus
/// `id`, which callers must include in `prop_set` if they want it).
///
/// The caller is responsible for building the `HashSet` once before iterating
/// over multiple objects so the set is not rebuilt on every call.
///
/// Takes `&Value` and clones surviving entries because the same `val` may be
/// needed after this call (e.g. for `header:` extraction in `handle_email_get`).
pub(crate) fn filter_properties(obj: &Value, prop_set: &HashSet<&str>) -> Value {
    match obj {
        Value::Object(map) => {
            let filtered: serde_json::Map<String, Value> = map
                .iter()
                .filter(|(k, _)| prop_set.contains(k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Value::Object(filtered)
        }
        _ => obj.clone(),
    }
}

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
    created_ids: &std::collections::BTreeMap<Id, Id>,
) -> Result<Id, Box<jmap_server::SetError>> {
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
/// of the maps is non-empty — e.g. `VacationResponse/set` never creates or
/// destroys, so only `updated` / `not_updated` are populated.
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
/// Five `/set` handlers in this crate (`Email/set`, `Mailbox/set`,
/// `Identity/set`, `SieveScript/set`, `VacationResponse/set`) end with the
/// same boilerplate: refresh the state token if `mutated`, then emit a
/// `HandlerOutput` wrapping the canonical RFC 8620 §5.3
/// envelope. Centralising it here keeps the five sites in lockstep — if a
/// future revision changes which keys are emitted (e.g. RFC 8620 §5.3.1 may
/// flip a key from `null` to omitted), all five handlers update at once.
///
/// `EmailSubmission/set` deliberately does NOT use this helper: it pushes
/// follow-up `Email/set` invocations from `onSuccessUpdateEmail` /
/// `onSuccessDestroyEmail` (RFC 8621 §7.4) and therefore needs to control
/// the returned `extra_invocations` rather than relying on the helper's
/// empty list.
///
/// The `O` type parameter is the JMAP object type for the state token
/// (e.g. `Email`, `Mailbox`); its only role is to disambiguate the
/// `get_state::<O>` call inside the helper.
///
/// Empty maps/arrays serialize as `null` (JMAP convention). The `Invocation`
/// vector is always empty for the five call sites — no `/set` call generates
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
    B: MailBackend,
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

/// draft-ietf-jmap-metadata-04 checks a `Foo/set` handler applies to one
/// create object or update patch (`entry`) before the backend sees it.
///
/// - -04 §3: for a request whose `using` lacks
///   `urn:ietf:params:jmap:metadata`, "Foo/set MUST reject any write to
///   them with an "invalidProperties" SetError".
/// - -04 §3.2: on create, "A value of null for either property MUST be
///   rejected with an "invalidProperties" SetError".
///
/// The backend validates the rest (-04 §3.2 rules 1 to 4).
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
    let uses_metadata = uses_metadata(using);
    for (key, value) in entry {
        let root = key.split_once('/').map_or(key.as_str(), |(r, _)| r);
        if root != "metadata" && root != "privateMetadata" {
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
pub(crate) fn create_metadata(entry: &Map<String, Value>) -> Result<MetadataMaps, Value> {
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
