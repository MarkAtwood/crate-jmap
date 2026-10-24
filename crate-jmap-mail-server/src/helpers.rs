//! Private helper utilities — re-exported from jmap_server.
use std::collections::{BTreeSet, HashSet};

use jmap_types::{Id, JmapError, JmapObject, State};
use serde_json::{Map, Value, json};

use crate::MailBackend;
use jmap_server::{HandlerOutput, server_fail_from_backend};

pub(crate) use jmap_server::{
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id, not_found_json,
    now_utc_string, serialize_value,
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

/// `true` if `using` includes `urn:ietf:params:jmap:metadata`.
pub(crate) fn uses_metadata(using: &[String]) -> bool {
    using
        .iter()
        .any(|u| u == jmap_metadata_types::JMAP_METADATA_URI)
}

/// draft-ietf-jmap-metadata-04 §3: the §3.6 filter conditions extend
/// `Foo/query` only for "requests whose using includes
/// urn:ietf:params:jmap:metadata". Without it a metadata leaf anywhere in
/// `filter` is `unsupportedFilter` (RFC 8620 §5.5: "The filter is
/// syntactically valid, but the server cannot process it").
///
/// # Errors
///
/// `unsupportedFilter` as above.
pub(crate) fn reject_metadata_filter_without_capability<T>(
    filter: Option<&jmap_types::Filter<T>>,
    is_metadata: fn(&T) -> bool,
    using: &[String],
) -> Result<(), JmapError> {
    fn any_leaf<T>(f: &jmap_types::Filter<T>, is_metadata: fn(&T) -> bool) -> bool {
        match f {
            jmap_types::Filter::Condition(c) => is_metadata(c),
            jmap_types::Filter::Operator(op) => {
                op.conditions.iter().any(|f| any_leaf(f, is_metadata))
            }
            _ => false,
        }
    }
    if uses_metadata(using) || !filter.is_some_and(|f| any_leaf(f, is_metadata)) {
        return Ok(());
    }
    let mut err = JmapError::unsupported_filter();
    err.description = Some(
        "metadata filter conditions require urn:ietf:params:jmap:metadata in using".to_owned(),
    );
    Err(err)
}

/// Which part of one metadata property a `Foo/get` returns.
#[derive(Debug, Default)]
enum Selection {
    /// Not returned.
    #[default]
    Omit,
    /// The whole stored object.
    All,
    /// Only these namespaces (-04 §3.1 subselectors, combined by union).
    Namespaces(BTreeSet<String>),
}

impl Selection {
    fn add_namespace(&mut self, ns: &str) {
        match self {
            Self::All => {}
            Self::Omit => *self = Self::Namespaces(BTreeSet::from([ns.to_owned()])),
            Self::Namespaces(set) => {
                set.insert(ns.to_owned());
            }
        }
    }

    /// The value to return for a property stored as `stored`; absent and
    /// `null` are `{}` (-04 §2: "When returned, its value MUST be an
    /// object ({} when no metadata is set), never null").
    fn select(&self, stored: Option<&Value>) -> Option<Value> {
        let stored = stored.and_then(Value::as_object);
        match self {
            Self::Omit => None,
            Self::All => Some(Value::Object(stored.cloned().unwrap_or_default())),
            Self::Namespaces(set) => Some(Value::Object(
                stored
                    .into_iter()
                    .flatten()
                    .filter(|(k, _)| set.contains(k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )),
        }
    }
}

/// How a `Foo/get` handler returns `metadata` and `privateMetadata`
/// (draft-ietf-jmap-metadata-04 §2 and §3.1).
#[derive(Debug, Default)]
pub(crate) struct MetadataProjection {
    metadata: Selection,
    private_metadata: Selection,
}

impl MetadataProjection {
    /// Build the projection for one `Foo/get` request.
    ///
    /// `properties` is the request's `properties` (`None` for the default
    /// list), `default_includes` is `false` for `Email/get`, whose
    /// "default property list ... is unchanged" (-04 §3.1), and `info` is
    /// the type's DataTypeMetadataInfo, `None` when the type is not listed
    /// and so "does not gain the metadata or privateMetadata properties"
    /// (-04 §1.2.1). The rules:
    ///
    /// - Default list: both properties with the capability in `using`
    ///   ("the server MUST include metadata and, where supportsPrivate is
    ///   true, privateMetadata"), neither without it ("not default
    ///   properties of Foo/get") (-04 §3, §3.1).
    /// - A root name returns the whole property, with or without the
    ///   capability ("still returned when requested by name").
    /// - With the capability, `metadata/<namespace>` and
    ///   `privateMetadata/<namespace>` return only the stored namespaces
    ///   named; several combine by union and a root entry supersedes them.
    ///   A namespace with no stored data is left out, which covers "A
    ///   subselector naming a namespace that is not supported ... MUST be
    ///   silently omitted ... unless the object has data stored under that
    ///   namespace". Without the capability these entries are ignored like
    ///   any other unknown property.
    /// - `privateMetadata` is never returned when `supportsPrivate` is
    ///   false (-04 §2: "this property MUST NOT be returned").
    ///
    /// # Errors
    ///
    /// `invalidArguments` for an entry that begins with `metadata/` or
    /// `privateMetadata/` and is not followed by a namespace identifier
    /// (-04 §3.1: "Any other properties entry beginning with metadata/ or
    /// privateMetadata/ MUST be rejected with an "invalidArguments"
    /// method-level error").
    pub(crate) fn new(
        properties: Option<&[String]>,
        default_includes: bool,
        using: &[String],
        info: Option<&jmap_metadata_types::DataTypeMetadataInfo>,
    ) -> Result<Self, JmapError> {
        let Some(info) = info else {
            return Ok(Self::default());
        };
        let uses = uses_metadata(using);
        let mut projection = Self::default();
        match properties {
            None if uses && default_includes => {
                projection.metadata = Selection::All;
                projection.private_metadata = Selection::All;
            }
            None => {}
            Some(props) => {
                for p in props {
                    let (root, ns) = p
                        .split_once('/')
                        .map_or((p.as_str(), None), |(r, n)| (r, Some(n)));
                    let sel = match root {
                        "metadata" => &mut projection.metadata,
                        "privateMetadata" => &mut projection.private_metadata,
                        _ => continue,
                    };
                    match ns {
                        None => *sel = Selection::All,
                        Some(_) if !uses => {}
                        // A subselector names one namespace: nothing below
                        // it ("metadata/<namespace>/<key>" is an error, and
                        // no namespace identifier contains '/').
                        Some(ns) if jmap_metadata_types::is_valid_namespace(ns) => {
                            sel.add_namespace(ns);
                        }
                        Some(_) => {
                            return Err(JmapError::invalid_arguments(format!(
                                "properties: {p:?} is not a metadata subselector"
                            )));
                        }
                    }
                }
            }
        }
        if !info.supports_private {
            projection.private_metadata = Selection::Omit;
        }
        Ok(projection)
    }

    /// Set or remove `metadata` and `privateMetadata` on the response
    /// object `target` from the stored object `source`.
    pub(crate) fn apply(&self, source: &Value, target: &mut Value) {
        let Value::Object(target) = target else {
            return;
        };
        for (root, sel) in [
            ("metadata", &self.metadata),
            ("privateMetadata", &self.private_metadata),
        ] {
            match sel.select(source.get(root)) {
                Some(v) => {
                    target.insert(root.to_owned(), v);
                }
                None => {
                    target.remove(root);
                }
            }
        }
    }
}
