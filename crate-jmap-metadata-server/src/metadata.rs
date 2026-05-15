//! Metadata/* method handlers (draft-ietf-jmap-metadata-01 §3).
//!
//! Provides all five JMAP Metadata method handlers:
//! - [`handle_metadata_get`]
//! - [`handle_metadata_changes`]
//! - [`handle_metadata_set`]
//! - [`handle_metadata_query`]
//! - [`handle_metadata_query_changes`]

use jmap_metadata_types::{Metadata, MetadataFilterCondition};
use jmap_types::{Filter, Id, Invocation, JmapError, PatchObject, State};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, MetadataBackend};
use crate::helpers::{extract_account_id, finalize_set_response, set_error_value, SetAccumulators};
use jmap_server::{server_fail_from_backend, SetError, SetErrorType};

// ---------------------------------------------------------------------------
// Metadata/get
// ---------------------------------------------------------------------------

/// Handle a `Metadata/get` method call (draft-ietf-jmap-metadata-01 §3.2).
///
/// Standard JMAP `/get` per RFC 8620 §5.1. The `ids` argument MAY be `null`
/// to fetch all Metadata objects in the account at once (draft §3.2).
pub async fn handle_metadata_get<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<Metadata, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Metadata/changes
// ---------------------------------------------------------------------------

/// Handle a `Metadata/changes` method call (draft-ietf-jmap-metadata-01 §3.3).
///
/// Standard JMAP `/changes` per RFC 8620 §5.2, plus two Metadata-specific
/// optional arguments:
///
/// - `filterRelatedType: String|null` — restrict the response's `created`
///   and `updated` arrays to Metadata objects with the given `relatedType`.
///   Does not affect the returned state.
/// - `filterMetadataType: String[]|null` — restrict to Metadata objects
///   whose `@type` value is in the array. Combined with `filterRelatedType`
///   via logical AND.
///
/// # Conformance split (default impl vs override)
///
/// Backends that override [`MetadataBackend::get_metadata_changes`] get
/// strict §3.3 conformance for all three arrays (`created`, `updated`,
/// `destroyed`) by pre-filtering at the storage layer. The workspace's
/// reference `MemoryBackend` (gated behind `feature = "memory"`)
/// achieves strict §3.3 conformance via this override (bd:JMAP-06zp.3.5.2).
///
/// The default impl on the trait delegates to
/// `JmapBackend::get_changes::<Metadata>` (turbofish form not link-resolvable
/// by rustdoc) and ignores the filter args.
/// In that case this handler post-filters `created` and `updated` by
/// re-fetching each Id via `get_objects::<Metadata>` and inspecting
/// `relatedType` / `@type`. The `destroyed` array is **not** post-filtered
/// under the default impl because destroyed objects no longer exist and
/// the standard `get_changes` return value does not carry per-Id metadata
/// for destroyed entries. Clients that need precise destroyed filtering
/// against a default-impl backend can remember each Id's `relatedType` /
/// `@type` from prior `/get` responses and filter client-side.
pub async fn handle_metadata_changes<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // Resolve accountId and verify the account exists FIRST, before any
    // method-specific argument parsing. RFC 8620 §3.6.2 lists both
    // `accountNotFound` and `invalidArguments` as method-level errors;
    // the canonical jmap-server generic handlers all check account
    // existence before parsing other args (jmap-server/src/handlers.rs
    // `handle_get`, `handle_changes`, `handle_query`,
    // `handle_query_changes`). Sibling extension-server crates mirror
    // that ordering. Parsing filter args first would make
    // `{accountId: "bogus", filterRelatedType: 42}` return
    // `invalidArguments` instead of `accountNotFound` — contributors
    // debugging an unknown account would chase the filter complaint
    // instead of their accountId. bd:JMAP-826m.34.
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // Extract the Metadata-specific filter args. We remove them from the
    // arg map even though we no longer delegate to the generic /changes
    // handler — keeping the wire validation strict (unknown args
    // remaining after this point are still ignored silently, matching
    // RFC 8620 §1.6 forgiveness for unknown fields).
    //
    // Recognized fields with the wrong shape (e.g. `filterRelatedType: 42`,
    // `filterMetadataType: "Email"` instead of an array) MUST surface as
    // `invalidArguments` — silently coercing to `None` would return a
    // result set that does not match the client's filter with no error
    // signal. We use typed serde deserialize per arg to propagate shape
    // errors with the standard JMAP error type. Wire-level Null deserialises
    // to `Option::None`, matching the spec's "filter absent" semantics.
    let (filter_related_type, filter_metadata_type) = {
        let filter_related_type: Option<String> = match args.remove("filterRelatedType") {
            None => None,
            Some(v) => serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("filterRelatedType: {e}")))?,
        };
        let filter_metadata_type: Option<Vec<String>> = match args.remove("filterMetadataType") {
            None => None,
            Some(v) => serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("filterMetadataType: {e}")))?,
        };
        (filter_related_type, filter_metadata_type)
    };

    let since_state: State = match args.get("sinceState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceState is required")),
    };

    let max_changes: Option<u64> = match args.get("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().filter(|&n| n > 0).ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let result = backend
        .get_metadata_changes(
            caller,
            &account_id,
            &since_state,
            max_changes,
            filter_related_type.as_deref(),
            filter_metadata_type.as_deref(),
        )
        .await
        .map_err(JmapError::from)?;

    // Build the wire-format /changes response (mirrors
    // jmap_server::handlers::handle_changes — the standard RFC 8620 §5.2
    // shape). `updatedProperties: null` per the same rationale documented
    // there (server cannot claim per-property change detail it does not
    // track).
    let mut response = json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "updatedProperties": Value::Null,
        "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });

    // Short-circuit: no filter args, no post-filter needed.
    if filter_related_type.is_none() && filter_metadata_type.is_none() {
        return Ok((response, vec![]));
    }

    // Post-filter the `created` and `updated` arrays for the default-impl
    // path. Backends that overrode `get_metadata_changes` have already
    // pre-filtered, in which case these calls are no-ops on already-pruned
    // arrays. The cost (per-Id re-fetch) is only paid by default-impl
    // backends; override backends pay zero overhead.
    filter_changes_array(
        backend,
        caller,
        &account_id,
        &mut response,
        "created",
        filter_related_type.as_deref(),
        filter_metadata_type.as_deref(),
    )
    .await?;
    filter_changes_array(
        backend,
        caller,
        &account_id,
        &mut response,
        "updated",
        filter_related_type.as_deref(),
        filter_metadata_type.as_deref(),
    )
    .await?;

    Ok((response, vec![]))
}

/// Re-fetch each Id in `response[key]` via `get_objects::<Metadata>` and
/// retain only those whose `relatedType` and `@type` match the supplied
/// filters.
///
/// **TOCTOU policy** (bd:JMAP-ayoz.4). If a Metadata object the change
/// log named for this window can no longer be fetched — typically because
/// a concurrent /set destroyed it between the `get_metadata_changes` call
/// and the `get_objects` call inside this function — its Id is **kept**
/// in the filtered array, not dropped.
///
/// Rationale: dropping the Id is the worst of both worlds. The state
/// token covers the window inclusive of the original change-log entry,
/// but the response arrays would no longer carry it; a client diffing
/// against the response would believe the object was never
/// created/updated in this window and the next /changes call from the
/// new state would not re-report it (the entry is on the wrong side of
/// the state cursor). The Id would be silently lost. Keeping the Id
/// matches the conservative interpretation of draft §3.3: the filter
/// applies to which entries appear, not to which entries the server is
/// allowed to forget.
///
/// Note that the default-impl path is still best-effort wrt filter
/// fidelity for objects that disappear mid-flight: a kept Id may not
/// actually match the filter the client requested (because we cannot
/// inspect its `relatedType` / `@type` post-destroy). Backends that
/// override `get_metadata_changes` and pre-filter from the change log
/// directly do not pay this cost.
///
/// # Batch-size warning for default-impl backends
///
/// This helper makes one `get_objects::<Metadata>(..., Some(&ids), None)`
/// call carrying every change-window id. There is NO max-batch cap,
/// NO chunking, NO size guard (bd:JMAP-826m.22). For a default-impl
/// database-backed `MetadataBackend` this can mean:
///
/// - An `IN (...)` clause with thousands of items, exceeding common
///   driver limits (PostgreSQL ~32k, SQLite ~999).
/// - A single network round-trip pulling every changed object's
///   payload — expensive when `/changes` was supposed to be the
///   lightweight cursor-style read.
/// - Memory pressure: the `id → Metadata` lookup table holds every
///   returned object plus a clone of each.
///
/// **Production backends MUST override `MetadataBackend::get_metadata_changes`**
/// to pre-filter from their change log directly. Backends that ship the
/// default impl unmodified will pay O(window_size) cost on every
/// `/changes` call. The override pattern is the only way to fully avoid
/// this helper's batch behaviour; chunking inside this helper would
/// still issue O(window_size / chunk_size) round-trips for the same
/// data volume.
async fn filter_changes_array<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    response: &mut Value,
    key: &str,
    filter_related_type: Option<&str>,
    filter_metadata_type: Option<&[String]>,
) -> Result<(), JmapError> {
    let Some(Value::Array(ids)) = response.get_mut(key) else {
        return Ok(());
    };

    if ids.is_empty() {
        return Ok(());
    }

    // Collect string Ids from the response array, preserving order.
    let id_strs: Vec<String> = ids
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_owned()))
        .collect();
    let ids_for_fetch: Vec<Id> = id_strs.iter().map(|s| Id::from(s.as_str())).collect();

    // Fetch the actual Metadata objects so we can inspect relatedType / @type.
    let (objects, _not_found) = backend
        .get_objects::<Metadata>(caller, account_id, Some(&ids_for_fetch), None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Build lookup: id -> Metadata for O(1) filter lookups.
    let lookup: std::collections::HashMap<String, &Metadata> = objects
        .iter()
        .filter_map(|m| m.id().map(|id| (id.as_ref().to_owned(), m)))
        .collect();

    let filtered: Vec<Value> = id_strs
        .into_iter()
        .filter(|id_str| {
            let Some(meta) = lookup.get(id_str) else {
                // TOCTOU policy (see function rustdoc): the object
                // disappeared between the change-log read and this
                // re-fetch. Keep the Id rather than silently drop it.
                return true;
            };
            if let Some(rt) = filter_related_type {
                if meta.related_type() != rt {
                    return false;
                }
            }
            if let Some(types) = filter_metadata_type {
                if !types.iter().any(|t| t == meta.type_name()) {
                    return false;
                }
            }
            true
        })
        .map(Value::String)
        .collect();

    *ids = filtered;
    Ok(())
}

// ---------------------------------------------------------------------------
// Metadata/set
// ---------------------------------------------------------------------------

/// Handle a `Metadata/set` method call (draft-ietf-jmap-metadata-01 §3.1).
///
/// Standard JMAP `/set` per RFC 8620 §5.3 with the following Metadata-specific
/// server-side constraints (enforced by the backend, surfaced via
/// `BackendSetError::SetError`):
///
/// - **Uniqueness** (§3.1): (relatedType, relatedId, `@type`, isPrivate)
///   tuple MUST be unique within the user's visible set →
///   `alreadyExists{ existingId: ... }` on conflict.
/// - **`maySetPrivate` gating** (§1.2.1): if the account does not permit
///   private metadata and the client supplies `isPrivate: true` →
///   `forbidden`.
/// - **Quota** (§6): `overQuota` if the operation would exceed account quota.
/// - **Related-object validation** (§3.1): `invalidProperties` listing
///   `relatedType` / `relatedId` if the referenced object does not exist.
///
/// All four error categories are reported per-entry in `notCreated` /
/// `notUpdated`; the handler itself is generic across these.
pub async fn handle_metadata_set<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let old_state = backend
        .get_state::<Metadata>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj_val) in create_map {
            // Deserialize the client-supplied wire object into a Metadata
            // variant. The `@type` tag is the discriminator; missing or
            // unknown values produce an invalidProperties SetError.
            //
            // RFC 8620 §5.3: invalidProperties SHOULD carry a `properties`
            // String[] listing the invalid property names. A whole-struct
            // serde deserialize failure cannot reliably name the offending
            // property (the error message embeds Rust type names and is
            // not stable wire output), so we construct the SetError without
            // `with_properties` rather than fabricate an inaccurate list.
            // The description preserves the serde error text for debugging
            // but is non-localised and not intended for end-user display.
            let metadata: Metadata = match serde_json::from_value(obj_val) {
                Ok(m) => m,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        set_error_value(
                            &SetError::new(SetErrorType::InvalidProperties)
                                .with_description(e.to_string()),
                        ),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<Metadata>(caller, &account_id, &create_id, metadata)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    created.insert(
                        create_id,
                        serde_json::to_value(&created_obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
                Err(_) => {
                    not_created.insert(
                        create_id,
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError. Use the typed
            // SetError builder so the wire shape matches every other
            // /set per-entry error in this handler.
            let patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str,
                        set_error_value(
                            &SetError::new(SetErrorType::InvalidPatch)
                                .with_description(e.to_string()),
                        ),
                    );
                    continue;
                }
            };

            match backend
                .update_object::<Metadata>(caller, &account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Ok(None) => {
                    mutated = true;
                    updated.insert(id_str, Value::Null);
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
                Err(_) => {
                    not_updated.insert(
                        id_str,
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string
        // Id. Validate-and-collect in one pass. Reject the whole request
        // on a non-string element rather than silently skipping it, which
        // would produce a misleading response.
        let mut id_strs: Vec<String> = Vec::with_capacity(destroy_arr.len());
        for id_val in &destroy_arr {
            match id_val.as_str() {
                Some(s) => id_strs.push(s.to_owned()),
                None => {
                    return Err(JmapError::invalid_arguments(format!(
                        "destroy: every element must be a string Id; got {id_val}"
                    )));
                }
            }
        }
        for id_str in id_strs {
            let id = Id::from(id_str.as_str());

            match backend
                .destroy_object::<Metadata>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
                Err(_) => {
                    not_destroyed.insert(
                        id_str,
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    finalize_set_response::<B, Metadata>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        SetAccumulators {
            created,
            updated,
            destroyed: destroyed_list,
            not_created,
            not_updated,
            not_destroyed,
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Metadata/query and Metadata/queryChanges
// ---------------------------------------------------------------------------

/// Known wire-format field names of [`MetadataFilterCondition`] leaf nodes,
/// per draft-ietf-jmap-metadata-01 §3.4.1. Used by
/// [`walk_filter_for_unknown_keys`] to reject filters that carry typos,
/// snake_case field names, vendor extensions, or future-spec fields.
///
/// **MAINTENANCE:** This list must mirror the wire-format field names of
/// [`MetadataFilterCondition`] in `jmap-metadata-types`. If a new field is
/// added to that struct, add its wire name here. Drift causes valid filter
/// conditions to be rejected with `unsupportedFilter` instead of being
/// applied — a protocol misbehavior that is not a compile error.
const KNOWN_CONDITION_KEYS: &[&str] = &[
    "@type",
    "relatedType",
    "relatedIds",
    "isPrivate",
    "textMatch",
];

/// Validate a filter tree before delegating to the generic /query or
/// /queryChanges handler.
///
/// Performs two passes:
///
/// 1. **Unknown-keys pass** ([`walk_filter_for_unknown_keys`]): walks the
///    raw filter JSON tree and rejects any object node whose keys do not
///    match the known filter schema (operator nodes: `operator`,
///    `conditions`; condition leaves: the
///    [`MetadataFilterCondition`] wire-format fields enumerated in
///    [`KNOWN_CONDITION_KEYS`]). Returns `unsupportedFilter` per
///    RFC 8620 §5.5.
///
///    This catches typos (`related_ids` vs `relatedIds`), vendor
///    extensions, and future-spec fields — all of which would otherwise
///    deserialize into [`MetadataFilterCondition::default`] and silently
///    return every object in the account.
///
///    Filter-algebra extensibility is **excluded** from the workspace
///    extras-preservation policy — silent-drop of an unknown filter
///    clause is a query-correctness bug. Vendors that need filterable
///    custom fields must use the draft-ietf-jmap-metadata `Annotation`
///    object or fork [`MetadataFilterCondition`]. See workspace
///    AGENTS.md "Filter-algebra exclusion".
///
/// 2. **Cross-field pass** ([`walk_filter_for_related_ids_constraint`]):
///    draft-ietf-jmap-metadata-01 §3.4.1 mandates *"relatedIds MUST only
///    be specified when relatedType is also specified. If relatedIds is
///    specified without relatedType, the server MUST reject the query
///    with an invalidArguments error."* Returns `invalidArguments`.
///
/// Returns `Ok(())` when the filter is absent or null. A filter whose
/// JSON shape is malformed in a way the unknown-keys walker does not
/// detect (e.g. a numeric where a string is expected) falls through to
/// the generic handler, which surfaces the deserialize failure as
/// `unsupportedFilter` via `optional_arg`.
fn validate_metadata_filter(args: &serde_json::Map<String, Value>) -> Result<(), JmapError> {
    let Some(filter_val) = args.get("filter") else {
        return Ok(());
    };
    if filter_val.is_null() {
        return Ok(());
    }
    walk_filter_for_unknown_keys(filter_val)?;
    // The unknown-keys walk already ensured every object node carries
    // only keys we recognise, so typed deserialize cannot silently drop
    // a typo'd field. A residual deserialize failure here means the
    // value of a known field is the wrong JSON type (e.g.
    // `relatedIds: 42`) — let the generic handler surface that as
    // `unsupportedFilter` per its own contract.
    let filter: Filter<MetadataFilterCondition> = match serde_json::from_value(filter_val.clone()) {
        Ok(f) => f,
        Err(_) => return Ok(()),
    };
    walk_filter_for_related_ids_constraint(&filter)
}

/// Recursively walk the raw filter JSON tree and reject any object node
/// whose keys are not in the known schema (RFC 8620 §5.5 filter algebra
/// + [`MetadataFilterCondition`] leaf fields).
///
/// Operator nodes are identified by the presence of an `operator` key
/// (matching the [`Filter`] untagged-union discrimination). Allowed keys
/// on operator nodes: `operator`, `conditions`. Condition leaf nodes are
/// any object without an `operator` key; allowed keys are
/// [`KNOWN_CONDITION_KEYS`].
///
/// Non-object filter nodes (arrays, strings, etc.) are passed through
/// untouched — the typed deserialize pass that runs after this walk will
/// surface those as `unsupportedFilter` via the generic handler.
fn walk_filter_for_unknown_keys(node: &Value) -> Result<(), JmapError> {
    let Some(obj) = node.as_object() else {
        return Ok(());
    };
    if obj.contains_key("operator") {
        // Operator node: allowed keys are `operator` and `conditions`.
        for key in obj.keys() {
            if key != "operator" && key != "conditions" {
                return Err(unsupported_filter_unknown_key(key));
            }
        }
        if let Some(conditions) = obj.get("conditions").and_then(|v| v.as_array()) {
            for child in conditions {
                walk_filter_for_unknown_keys(child)?;
            }
        }
        Ok(())
    } else {
        // Condition leaf: allowed keys are MetadataFilterCondition wire fields.
        for key in obj.keys() {
            if !KNOWN_CONDITION_KEYS.contains(&key.as_str()) {
                return Err(unsupported_filter_unknown_key(key));
            }
        }
        Ok(())
    }
}

/// Construct an `unsupportedFilter` error carrying the offending key name
/// in the description.
fn unsupported_filter_unknown_key(key: &str) -> JmapError {
    let mut err = JmapError::unsupported_filter();
    err.description = Some(format!(
        "filter: unknown field {key:?} (RFC 8620 §5.5; draft-ietf-jmap-metadata-01 §3.4.1)"
    ));
    err
}

/// Recursively walk a `Filter<MetadataFilterCondition>` tree and return
/// `invalidArguments` if any leaf has `relatedIds` set while `relatedType`
/// is `None`. AND/OR/NOT operator nodes recurse into their conditions.
fn walk_filter_for_related_ids_constraint(
    filter: &Filter<MetadataFilterCondition>,
) -> Result<(), JmapError> {
    match filter {
        Filter::Operator(op) => {
            for child in &op.conditions {
                walk_filter_for_related_ids_constraint(child)?;
            }
            Ok(())
        }
        Filter::Condition(cond) => {
            if cond.related_ids.is_some() && cond.related_type.is_none() {
                Err(JmapError::invalid_arguments(
                    "filter: relatedIds MUST only be specified when relatedType is also specified (draft-ietf-jmap-metadata-01 §3.4.1)",
                ))
            } else {
                Ok(())
            }
        }
        // Filter<T> is #[non_exhaustive]; new variants in jmap-types are
        // not yet known here. Be conservative: a variant we cannot inspect
        // cannot be validated, but it is also not a violation of the
        // §3.4.1 leaf constraint we are enforcing.
        _ => Ok(()),
    }
}

/// Handle a `Metadata/query` method call (draft-ietf-jmap-metadata-01 §3.4).
///
/// Standard JMAP `/query` per RFC 8620 §5.5. The
/// [`MetadataFilterCondition`]
/// supports `@type`, `relatedType`, `relatedId`/`relatedIds`, `isPrivate`,
/// and `textMatch` operators (§3.4.1). Per §3.4.2 the result is sortable on
/// `id`, `@type`, `relatedType`, `relatedId`, and `isPrivate`.
///
/// Cross-field validation per §3.4.1 (`relatedIds` requires `relatedType`)
/// happens here, before delegating to the generic handler. Other validation
/// (account existence, sort comparator shape, etc.) is handled by the
/// generic.
pub async fn handle_metadata_query<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let args = preflight_query_account_and_filter(backend, caller, args).await?;
    jmap_server::handlers::handle_query::<Metadata, B>(backend, caller, args).await
}

/// Handle a `Metadata/queryChanges` method call
/// (draft-ietf-jmap-metadata-01 §3.5).
///
/// Standard JMAP `/queryChanges` per RFC 8620 §5.6. Same Metadata-specific
/// cross-field filter validation as [`handle_metadata_query`].
pub async fn handle_metadata_query_changes<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let args = preflight_query_account_and_filter(backend, caller, args).await?;
    jmap_server::handlers::handle_query_changes::<Metadata, B>(backend, caller, args).await
}

/// Verify the account exists, then run the Metadata-specific filter
/// validation, then return the args untouched for the generic handler to
/// re-parse.
///
/// **Ordering** (bd:JMAP-826m.34): RFC 8620 §3.6.2 lists
/// `accountNotFound` and `invalidArguments` / `unsupportedFilter` as
/// method-level errors without explicit ordering, but the canonical
/// jmap-server generic handlers all check account existence before
/// parsing method-specific args. Mirror that ordering here so a request
/// like `{"accountId": "bogus", "filter": {"related_ids": [...]}}`
/// returns `accountNotFound` (the user's actual problem) rather than
/// `unsupportedFilter` (which would send the contributor chasing the
/// filter complaint instead).
///
/// The generic handler will re-run `account_exists` after we return; the
/// duplicate call is idempotent and cheap (a HashMap lookup in the
/// reference MemoryBackend). Production backends that find the duplicate
/// expensive can short-circuit `account_exists` with a one-shot cache.
async fn preflight_query_account_and_filter<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<Value, JmapError> {
    let Some(args_map) = args.as_object() else {
        return Err(JmapError::invalid_arguments(
            "arguments must be an object containing accountId",
        ));
    };
    // Peek at accountId without consuming `args` so we can hand the same
    // value to the generic handler below.
    let account_id_str = args_map
        .get("accountId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JmapError::invalid_arguments("accountId is required"))?;
    let account_id = Id::new_validated(account_id_str)
        .map_err(|e| JmapError::invalid_arguments(format!("accountId is not a valid Id: {e}")))?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    validate_metadata_filter(args_map)?;
    Ok(args)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;
    use jmap_server::{SetError, SetErrorType};

    // -----------------------------------------------------------------------
    // Metadata/set tests
    // -----------------------------------------------------------------------

    /// Oracle: draft-ietf-jmap-metadata-01 §3.1 — a valid Annotation create
    /// completes successfully and appears in `created`.
    #[tokio::test]
    async fn set_create_annotation_succeeds() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "@type": "Annotation",
                    "relatedType": "Email",
                    "relatedId": "EM1",
                    "isPrivate": true,
                    "acme.example.com:workflowState": "pending-review"
                }
            }
        });
        let (resp, _) = handle_metadata_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        assert!(
            resp["created"].is_object(),
            "created must be present on success: {resp}"
        );
        let c1 = &resp["created"]["c1"];
        assert!(c1.is_object(), "c1 must be in created map: {resp}");
        assert_eq!(c1["@type"], "Annotation");
        assert_eq!(c1["relatedType"], "Email");
        assert!(
            c1.get("id").is_some(),
            "server-assigned id must be echoed: {c1}"
        );
        // Vendor property survives the round-trip via the extras flatten.
        assert_eq!(c1["acme.example.com:workflowState"], "pending-review");
    }

    /// Oracle: §3.1 — uniqueness violation produces `alreadyExists` in
    /// `notCreated`. The handler is generic — the backend reports the
    /// constraint via `BackendSetError::SetError`.
    #[tokio::test]
    async fn set_create_uniqueness_violation_returns_already_exists() {
        let backend = MockBackend::new_with_account("acc1");
        backend.force_create_error(
            "acc1",
            SetError::new(SetErrorType::AlreadyExists)
                .with_description("Metadata for (Email, EM1, Annotation, true) already exists"),
        );

        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "@type": "Annotation",
                    "relatedType": "Email",
                    "relatedId": "EM1",
                    "isPrivate": true
                }
            }
        });
        let (resp, _) = handle_metadata_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        assert_eq!(
            resp["notCreated"]["c1"]["type"], "alreadyExists",
            "uniqueness collision must report alreadyExists: {resp}"
        );
    }

    /// Oracle: §1.2.1 — `maySetPrivate: false` plus `isPrivate: true` →
    /// `forbidden`. Same generic handler path as the uniqueness case;
    /// backend signals via `Forbidden`.
    #[tokio::test]
    async fn set_create_private_when_not_permitted_returns_forbidden() {
        let backend = MockBackend::new_with_account("acc1");
        backend.force_create_error(
            "acc1",
            SetError::new(SetErrorType::Forbidden)
                .with_description("Account does not permit private metadata"),
        );

        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "@type": "Annotation",
                    "relatedType": "Email",
                    "relatedId": "EM1",
                    "isPrivate": true
                }
            }
        });
        let (resp, _) = handle_metadata_set(&backend, &(), args).await.unwrap();

        assert_eq!(resp["notCreated"]["c1"]["type"], "forbidden");
    }

    /// Oracle: bd:JMAP-826m.15 — `force_create_error` is one-shot: the
    /// next create_object call consumes the slot and subsequent creates
    /// on the same backend succeed. Pre-fix the error was sticky,
    /// silently affecting every later create.
    #[tokio::test]
    async fn force_create_error_is_one_shot_and_auto_resets() {
        let backend = MockBackend::new_with_account("acc1");
        backend.force_create_error(
            "acc1",
            SetError::new(SetErrorType::AlreadyExists).with_description("one-shot"),
        );

        // First create — forced to fail.
        let args1 = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "@type": "Annotation",
                    "relatedType": "Email",
                    "relatedId": "EM1"
                }
            }
        });
        let (resp1, _) = handle_metadata_set(&backend, &(), args1).await.unwrap();
        assert_eq!(
            resp1["notCreated"]["c1"]["type"], "alreadyExists",
            "first create must hit the forced error: {resp1}",
        );

        // Second create on the same backend — forced error should be
        // gone, the create should succeed normally.
        let args2 = json!({
            "accountId": "acc1",
            "create": {
                "c2": {
                    "@type": "Annotation",
                    "relatedType": "Email",
                    "relatedId": "EM2"
                }
            }
        });
        let (resp2, _) = handle_metadata_set(&backend, &(), args2).await.unwrap();
        assert!(
            resp2["created"]["c2"].is_object(),
            "second create must succeed after forced error consumed: {resp2}",
        );
        assert!(
            resp2["notCreated"].as_object().is_none_or(|m| m.is_empty()),
            "second create must not be in notCreated: {resp2}",
        );
    }

    /// Oracle: bd:JMAP-826m.19 — MockBackend hard-errors on
    /// create_object for an unknown account, aligning with the public
    /// MemoryBackend. Pre-fix the two backends diverged: MockBackend
    /// silently auto-registered an account via `.entry(...).or_default()`,
    /// MemoryBackend hard-errored with `unknown account`. Two backends
    /// with different unknown-account behavior were a future-bug magnet
    /// for contributors writing dual-target tests.
    #[tokio::test]
    async fn mock_backend_create_against_unknown_account_errors() {
        use crate::test_support::MockBackend;
        let backend = MockBackend::new(); // No accounts registered.
        let meta: Metadata = serde_json::from_value(json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1"
        }))
        .unwrap();
        let err = backend
            .create_object::<Metadata>(&(), &Id::from("bogus"), "c1", meta)
            .await
            .expect_err("must reject create against unknown account");
        match err {
            BackendSetError::Other(e) => {
                assert!(
                    e.description().contains("unknown account"),
                    "expected 'unknown account' in error: {}",
                    e.description(),
                );
            }
            other => panic!("expected BackendSetError::Other, got: {other:?}"),
        }
    }

    /// Oracle: RFC 8620 §5.3 — malformed Annotation create (missing
    /// required `relatedType`) → `invalidProperties` in notCreated. No
    /// backend call is made. The SetError carries a non-empty
    /// `description`; the `properties` array is intentionally absent
    /// because a whole-struct serde deserialize failure cannot reliably
    /// name the offending property without fabricating a list.
    /// Regression test for bd:JMAP-ayoz.3.
    #[tokio::test]
    async fn set_create_missing_required_field_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "@type": "Annotation",
                    "relatedId": "EM1"
                    // relatedType deliberately missing
                }
            }
        });
        let (resp, _) = handle_metadata_set(&backend, &(), args).await.unwrap();

        let err = &resp["notCreated"]["c1"];
        assert_eq!(
            err["type"], "invalidProperties",
            "missing relatedType must produce invalidProperties: {resp}",
        );
        // Description is present and non-empty (RFC 8620 §5.3 description
        // field — non-localised, includes the underlying serde error).
        let desc = err["description"]
            .as_str()
            .expect("description must be a string");
        assert!(!desc.is_empty(), "description must be non-empty: {resp}",);
        // `properties` MUST be absent or null. Asserting absence guards
        // against a future change that fabricates an inaccurate property
        // list from the serde error text.
        assert!(
            err.get("properties").map_or(true, Value::is_null),
            "properties must be absent or null for whole-struct deserialize failure: {resp}",
        );
    }

    /// Oracle: RFC 8620 §5.3 — destroying an unknown id returns `notFound`.
    #[tokio::test]
    async fn set_destroy_nonexistent_returns_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": ["md-does-not-exist"]
        });
        let (resp, _) = handle_metadata_set(&backend, &(), args).await.unwrap();

        assert_eq!(
            resp["notDestroyed"]["md-does-not-exist"]["type"], "notFound",
            "destroying unknown id must produce notFound: {resp}"
        );
    }

    /// Oracle: RFC 8620 §5.3 — destroy a previously-created id; succeeds.
    #[tokio::test]
    async fn set_destroy_existing_succeeds() {
        let backend = MockBackend::new_with_account("acc1");
        backend.add_metadata(
            "acc1",
            "md1",
            json!({
                "@type": "Annotation",
                "id": "md1",
                "relatedType": "Email",
                "relatedId": "EM1"
            }),
        );

        let args = json!({
            "accountId": "acc1",
            "destroy": ["md1"]
        });
        let (resp, _) = handle_metadata_set(&backend, &(), args).await.unwrap();

        assert!(
            resp["destroyed"].is_array(),
            "destroyed must be array: {resp}"
        );
        assert_eq!(resp["destroyed"][0], "md1");
    }

    /// Oracle: RFC 8620 §5.3 — non-string destroy element → top-level
    /// `invalidArguments`.
    #[tokio::test]
    async fn set_destroy_null_element_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": [null]
        });
        let result = handle_metadata_set(&backend, &(), args).await;
        let err = result.expect_err("must return top-level error");
        assert_eq!(err.error_type.as_str(), "invalidArguments");
    }

    /// Oracle: RFC 8620 §5.3 — update with a non-object patch →
    /// `invalidPatch`. The SetError carries a non-empty `description`
    /// produced via the typed builder; the wire shape matches every
    /// other /set per-entry error in this handler. Regression test
    /// for bd:JMAP-ayoz.3.
    #[tokio::test]
    async fn set_update_non_object_patch_returns_invalid_patch() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "update": {
                "md1": "not-an-object"
            }
        });
        let (resp, _) = handle_metadata_set(&backend, &(), args).await.unwrap();
        let err = &resp["notUpdated"]["md1"];
        assert_eq!(err["type"], "invalidPatch");
        let desc = err["description"]
            .as_str()
            .expect("description must be a string");
        assert!(!desc.is_empty(), "description must be non-empty: {resp}",);
    }

    // -----------------------------------------------------------------------
    // Metadata/changes filter tests
    // -----------------------------------------------------------------------

    /// Oracle: draft §3.3 — `filterRelatedType: "Email"` retains only
    /// Metadata objects whose `relatedType` equals "Email" in the
    /// `created` array. Objects with other related types are dropped.
    #[tokio::test]
    async fn changes_filter_related_type_drops_non_matching_created() {
        let backend = MockBackend::new_with_account("acc1");
        // Pre-populate three Metadata objects across two relatedTypes,
        // then mark them all as created since state "0".
        backend.add_metadata(
            "acc1",
            "md1",
            json!({
                "@type": "Annotation",
                "id": "md1",
                "relatedType": "Email",
                "relatedId": "EM1"
            }),
        );
        backend.add_metadata(
            "acc1",
            "md2",
            json!({
                "@type": "Annotation",
                "id": "md2",
                "relatedType": "Mailbox",
                "relatedId": "MB1"
            }),
        );
        backend.add_metadata(
            "acc1",
            "md3",
            json!({
                "@type": "Annotation",
                "id": "md3",
                "relatedType": "Email",
                "relatedId": "EM2"
            }),
        );
        // Force the mock's change log to claim those three were created
        // and bump the state so /changes reports them.
        {
            let mut guard = backend.state_for_test();
            let acct = guard.get_mut("acc1").unwrap();
            acct.created = vec![Id::from("md1"), Id::from("md2"), Id::from("md3")];
            acct.state = 1;
        }

        let args = json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterRelatedType": "Email"
        });
        let (resp, _) = handle_metadata_changes(&backend, &(), args).await.unwrap();

        let created = resp["created"].as_array().expect("created must be array");
        let created_ids: Vec<&str> = created.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            created_ids.contains(&"md1"),
            "md1 (Email) must survive filter: {resp}"
        );
        assert!(
            created_ids.contains(&"md3"),
            "md3 (Email) must survive filter: {resp}"
        );
        assert!(
            !created_ids.contains(&"md2"),
            "md2 (Mailbox) must be dropped by filterRelatedType=Email: {resp}"
        );
    }

    /// Oracle: draft §3.3 — `filterMetadataType: ["Annotation"]` retains
    /// only Metadata objects whose `@type` is in the list.
    #[tokio::test]
    async fn changes_filter_metadata_type_drops_non_matching_created() {
        let backend = MockBackend::new_with_account("acc1");
        backend.add_metadata(
            "acc1",
            "md1",
            json!({
                "@type": "Annotation",
                "id": "md1",
                "relatedType": "Email",
                "relatedId": "EM1"
            }),
        );
        backend.add_metadata(
            "acc1",
            "md2",
            json!({
                "@type": "ImapMetadata",
                "id": "md2",
                "relatedType": "Mailbox",
                "relatedId": "MB1",
                "metadata": {}
            }),
        );
        {
            let mut guard = backend.state_for_test();
            let acct = guard.get_mut("acc1").unwrap();
            acct.created = vec![Id::from("md1"), Id::from("md2")];
            acct.state = 1;
        }

        let args = json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterMetadataType": ["Annotation"]
        });
        let (resp, _) = handle_metadata_changes(&backend, &(), args).await.unwrap();

        let created_ids: Vec<&str> = resp["created"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(created_ids, vec!["md1"], "only Annotation survives: {resp}");
    }

    /// Oracle: draft §3.3 — both filters present combine via logical AND.
    #[tokio::test]
    async fn changes_both_filters_combine_as_and() {
        let backend = MockBackend::new_with_account("acc1");
        backend.add_metadata(
            "acc1",
            "md1",
            json!({
                "@type": "Annotation",
                "id": "md1",
                "relatedType": "Email",
                "relatedId": "EM1"
            }),
        );
        backend.add_metadata(
            "acc1",
            "md2",
            json!({
                "@type": "Annotation",
                "id": "md2",
                "relatedType": "Mailbox",
                "relatedId": "MB1"
            }),
        );
        backend.add_metadata(
            "acc1",
            "md3",
            json!({
                "@type": "ImapMetadata",
                "id": "md3",
                "relatedType": "Email",
                "relatedId": "EM1",
                "metadata": {}
            }),
        );
        {
            let mut guard = backend.state_for_test();
            let acct = guard.get_mut("acc1").unwrap();
            acct.created = vec![Id::from("md1"), Id::from("md2"), Id::from("md3")];
            acct.state = 1;
        }

        let args = json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterRelatedType": "Email",
            "filterMetadataType": ["Annotation"]
        });
        let (resp, _) = handle_metadata_changes(&backend, &(), args).await.unwrap();

        let created_ids: Vec<&str> = resp["created"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            created_ids,
            vec!["md1"],
            "only Email + Annotation survives both filters: {resp}"
        );
    }

    /// Oracle: draft §3.3 — when neither filter is set the response is
    /// returned unchanged by the post-filter path.
    #[tokio::test]
    async fn changes_without_filters_passes_through_unchanged() {
        let backend = MockBackend::new_with_account("acc1");
        backend.add_metadata(
            "acc1",
            "md1",
            json!({
                "@type": "Annotation",
                "id": "md1",
                "relatedType": "Email",
                "relatedId": "EM1"
            }),
        );
        {
            let mut guard = backend.state_for_test();
            let acct = guard.get_mut("acc1").unwrap();
            acct.created = vec![Id::from("md1")];
            acct.state = 1;
        }

        let args = json!({
            "accountId": "acc1",
            "sinceState": "0"
        });
        let (resp, _) = handle_metadata_changes(&backend, &(), args).await.unwrap();

        let created_ids: Vec<&str> = resp["created"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(created_ids, vec!["md1"]);
    }

    /// Oracle: bd:JMAP-ayoz.8 — recognized filter args with the wrong
    /// wire shape must surface as `invalidArguments`, not silently coerce
    /// to None. A client sending `filterRelatedType: 42` previously got a
    /// result set ignoring the filter; now the request is rejected.
    #[tokio::test]
    async fn changes_filter_related_type_non_string_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterRelatedType": 42
        });
        let err = handle_metadata_changes(&backend, &(), args)
            .await
            .expect_err("non-string filterRelatedType must return Err");
        assert_eq!(err.error_type, "invalidArguments", "got: {err:?}");
    }

    /// Oracle: bd:JMAP-ayoz.8 — same shape enforcement applies to
    /// `filterMetadataType`. A non-array value (here a bare string)
    /// must surface as invalidArguments, not silently become None.
    #[tokio::test]
    async fn changes_filter_metadata_type_non_array_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterMetadataType": "Annotation"
        });
        let err = handle_metadata_changes(&backend, &(), args)
            .await
            .expect_err("non-array filterMetadataType must return Err");
        assert_eq!(err.error_type, "invalidArguments", "got: {err:?}");
    }

    /// Oracle: bd:JMAP-ayoz.8 — within filterMetadataType, every element
    /// must be a string. A non-string element (here an integer) surfaces
    /// as invalidArguments rather than being silently filter_map-dropped.
    #[tokio::test]
    async fn changes_filter_metadata_type_non_string_element_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterMetadataType": ["Annotation", 42]
        });
        let err = handle_metadata_changes(&backend, &(), args)
            .await
            .expect_err("non-string element in filterMetadataType must return Err");
        assert_eq!(err.error_type, "invalidArguments", "got: {err:?}");
    }

    /// Oracle: bd:JMAP-ayoz.4 — filter_changes_array's post-fetch path
    /// MUST NOT silently drop an Id whose Metadata object disappeared
    /// between the change-log read and the get_objects re-fetch (TOCTOU
    /// with a concurrent destroyer). The Id is kept; per draft §3.3 the
    /// filter applies to which entries appear, not to which entries the
    /// server is allowed to forget.
    ///
    /// Setup: seed the change log with two created Ids (md_present,
    /// md_gone), but only register md_present's actual Metadata object
    /// in the mock. The re-fetch returns md_gone in `not_found`,
    /// simulating a concurrent destroy.
    #[tokio::test]
    async fn changes_filter_keeps_id_when_object_disappeared_mid_flight() {
        let backend = MockBackend::new_with_account("acc1");
        // Only one of the two created-id targets actually exists in the
        // mock store — the other simulates a concurrent destroy.
        backend.add_metadata(
            "acc1",
            "md_present",
            json!({
                "@type": "Annotation",
                "id": "md_present",
                "relatedType": "Email",
                "relatedId": "EM1"
            }),
        );
        {
            let mut guard = backend.state_for_test();
            let acct = guard.get_mut("acc1").unwrap();
            acct.created = vec![Id::from("md_present"), Id::from("md_gone")];
            acct.state = 1;
        }

        // Use a filter so filter_changes_array runs (it short-circuits
        // when both filter args are None).
        let args = json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterRelatedType": "Email"
        });
        let (resp, _) = handle_metadata_changes(&backend, &(), args).await.unwrap();

        let created_ids: Vec<&str> = resp["created"]
            .as_array()
            .expect("created must be array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(
            created_ids.contains(&"md_present"),
            "md_present (Email) must survive filter: {resp}",
        );
        assert!(
            created_ids.contains(&"md_gone"),
            "md_gone (disappeared mid-flight) MUST be kept, not silently dropped: {resp}",
        );
    }

    // -----------------------------------------------------------------------
    // Metadata/query cross-field filter validation (bd:JMAP-ayoz.6.1)
    // -----------------------------------------------------------------------

    /// Oracle: draft §3.4.1 — `relatedIds` MUST only be specified when
    /// `relatedType` is also specified. A /query call carrying a leaf
    /// `MetadataFilterCondition` with `relatedIds: [...]` and
    /// `relatedType: null` MUST be rejected with `invalidArguments`.
    #[tokio::test]
    async fn query_related_ids_without_related_type_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "filter": {
                "relatedIds": ["EM1", "EM2"]
                // relatedType deliberately omitted
            }
        });
        let err = handle_metadata_query(&backend, &(), args)
            .await
            .expect_err("must reject relatedIds-without-relatedType");
        assert_eq!(
            err.error_type, "invalidArguments",
            "expected invalidArguments, got: {err:?}",
        );
    }

    /// Oracle: draft §3.4.1 — the constraint applies inside nested
    /// FilterOperator (AND/OR/NOT) trees. A relatedIds-without-
    /// relatedType leaf inside an OR branch is still rejected.
    #[tokio::test]
    async fn query_nested_related_ids_without_related_type_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "filter": {
                "operator": "OR",
                "conditions": [
                    { "relatedType": "Email" },
                    { "relatedIds": ["EM1"] }
                ]
            }
        });
        let err = handle_metadata_query(&backend, &(), args)
            .await
            .expect_err("must reject relatedIds-without-relatedType nested in OR");
        assert_eq!(err.error_type, "invalidArguments", "got: {err:?}");
    }

    /// Oracle: draft §3.4.1 — when `relatedType` is supplied alongside
    /// `relatedIds`, the constraint is satisfied and the call proceeds
    /// to the generic /query handler. Negative control for the
    /// validation logic.
    #[tokio::test]
    async fn query_related_ids_with_related_type_passes_validation() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "filter": {
                "relatedType": "Email",
                "relatedIds": ["EM1", "EM2"]
            }
        });
        // The MockBackend does not implement /query and returns an empty
        // result. What matters here is that the call does NOT return an
        // invalidArguments error — the validation must pass through.
        let result = handle_metadata_query(&backend, &(), args).await;
        match result {
            Ok(_) => {} // validation passed, generic handler ran
            Err(e) => {
                assert_ne!(
                    e.error_type, "invalidArguments",
                    "filter with both relatedType and relatedIds must pass validation: {e:?}",
                );
            }
        }
    }

    /// Oracle: same cross-field constraint applies to /queryChanges
    /// per §3.5 (which references §3.4.1 for filter semantics).
    #[tokio::test]
    async fn query_changes_related_ids_without_related_type_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "sinceQueryState": "0",
            "filter": {
                "relatedIds": ["EM1"]
            }
        });
        let err = handle_metadata_query_changes(&backend, &(), args)
            .await
            .expect_err("must reject relatedIds-without-relatedType");
        assert_eq!(err.error_type, "invalidArguments", "got: {err:?}");
    }

    // -----------------------------------------------------------------------
    // Metadata/query unknown-filter-key rejection (bd:JMAP-826m.1)
    //
    // MetadataFilterCondition has all-Option fields and the workspace
    // canonical-template rule forbids `#[serde(deny_unknown_fields)]` on
    // FilterCondition types (see crate-jmap-mail-types/src/query.rs:32-33).
    // The validator must instead walk the raw filter JSON tree and reject
    // any unknown key before typed deserialize silently drops it.
    //
    // RFC 8620 §5.5 prescribes `unsupportedFilter` for filters the server
    // cannot process. The cross-field §3.4.1 check is unchanged
    // (`invalidArguments`).
    // -----------------------------------------------------------------------

    /// Oracle: snake_case typo `related_ids` (instead of camelCase
    /// `relatedIds`) MUST be rejected with `unsupportedFilter`. Before the
    /// fix, the typo'd field was silently dropped, the validator saw a
    /// default-constructed condition, and the /query returned ALL objects
    /// — a query-correctness bug.
    #[tokio::test]
    async fn query_snake_case_typo_returns_unsupported_filter() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "filter": {
                "related_ids": ["EM1"]
            }
        });
        let err = handle_metadata_query(&backend, &(), args)
            .await
            .expect_err("must reject snake_case typo");
        assert_eq!(
            err.error_type, "unsupportedFilter",
            "expected unsupportedFilter, got: {err:?}",
        );
    }

    /// Oracle: an unknown leaf field (vendor extension, future-spec field,
    /// or typo) MUST be rejected with `unsupportedFilter` rather than
    /// silently dropped.
    #[tokio::test]
    async fn query_unknown_leaf_key_returns_unsupported_filter() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "filter": {
                "relatedType": "Email",
                "acmeVendorField": "anything"
            }
        });
        let err = handle_metadata_query(&backend, &(), args)
            .await
            .expect_err("must reject unknown leaf field");
        assert_eq!(err.error_type, "unsupportedFilter", "got: {err:?}");
    }

    /// Oracle: an unknown key on an operator node (e.g. an attempted
    /// "metadata" sidecar on a FilterOperator) MUST be rejected with
    /// `unsupportedFilter`. Operator nodes allow only `operator` and
    /// `conditions`.
    #[tokio::test]
    async fn query_unknown_operator_key_returns_unsupported_filter() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "filter": {
                "operator": "AND",
                "conditions": [{ "relatedType": "Email" }],
                "extraOperatorField": true
            }
        });
        let err = handle_metadata_query(&backend, &(), args)
            .await
            .expect_err("must reject unknown operator-node field");
        assert_eq!(err.error_type, "unsupportedFilter", "got: {err:?}");
    }

    /// Oracle: an unknown leaf key nested inside a FilterOperator
    /// (AND/OR/NOT) tree MUST also be rejected with `unsupportedFilter`.
    /// The walk is recursive.
    #[tokio::test]
    async fn query_nested_unknown_key_returns_unsupported_filter() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "filter": {
                "operator": "OR",
                "conditions": [
                    { "relatedType": "Email" },
                    {
                        "operator": "AND",
                        "conditions": [
                            { "vendorTypo": "x" }
                        ]
                    }
                ]
            }
        });
        let err = handle_metadata_query(&backend, &(), args)
            .await
            .expect_err("must reject deeply-nested unknown key");
        assert_eq!(err.error_type, "unsupportedFilter", "got: {err:?}");
    }

    /// Oracle: every documented `MetadataFilterCondition` wire field
    /// (`@type`, `relatedType`, `relatedIds`, `isPrivate`, `textMatch`)
    /// MUST pass the unknown-key check. Negative control for the
    /// `KNOWN_CONDITION_KEYS` allow-list — if any wire field is removed
    /// from the list, this test catches it.
    #[tokio::test]
    async fn query_all_known_leaf_keys_pass_validation() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "filter": {
                "@type": ["Annotation"],
                "relatedType": "Email",
                "relatedIds": ["EM1"],
                "isPrivate": false,
                "textMatch": "needle"
            }
        });
        // MockBackend does not implement /query and returns empty. What
        // matters is that the call does NOT return `unsupportedFilter` —
        // the validation must pass through.
        let result = handle_metadata_query(&backend, &(), args).await;
        if let Err(e) = result {
            assert_ne!(
                e.error_type, "unsupportedFilter",
                "all known wire fields must pass validation: {e:?}",
            );
        }
    }

    /// Oracle: same unknown-key rejection applies to /queryChanges per
    /// §3.5 (which references §3.4.1 for filter semantics).
    #[tokio::test]
    async fn query_changes_snake_case_typo_returns_unsupported_filter() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "sinceQueryState": "0",
            "filter": {
                "related_ids": ["EM1"]
            }
        });
        let err = handle_metadata_query_changes(&backend, &(), args)
            .await
            .expect_err("must reject snake_case typo");
        assert_eq!(err.error_type, "unsupportedFilter", "got: {err:?}");
    }

    // -----------------------------------------------------------------------
    // Method-level error ordering — account check runs first (bd:JMAP-826m.34)
    //
    // RFC 8620 §3.6.2 lists both `accountNotFound` and `invalidArguments` /
    // `unsupportedFilter` as method-level errors. The canonical
    // jmap-server generic handlers all resolve accountId and verify the
    // account exists BEFORE parsing method-specific args. The
    // jmap-metadata-server wrappers must mirror that ordering so a
    // request with both an unknown account AND a malformed filter / arg
    // returns `accountNotFound` (the user's actual problem) rather than
    // the filter complaint.
    // -----------------------------------------------------------------------

    /// Oracle: `Metadata/changes` with an unknown accountId AND a
    /// malformed `filterRelatedType` MUST return `accountNotFound`, not
    /// `invalidArguments`. The reviewer's debugging-experience argument:
    /// a contributor with a wrong accountId should see "your accountId
    /// is unknown", not "your filterRelatedType is malformed".
    #[tokio::test]
    async fn changes_unknown_account_with_malformed_filter_returns_account_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "bogus",
            "sinceState": "0",
            "filterRelatedType": 42  // wrong shape — should NOT win the error race
        });
        let err = handle_metadata_changes(&backend, &(), args)
            .await
            .expect_err("must reject unknown account");
        assert_eq!(
            err.error_type, "accountNotFound",
            "expected accountNotFound to win the error race, got: {err:?}",
        );
    }

    /// Oracle: `Metadata/changes` with an unknown accountId AND a
    /// malformed `filterMetadataType` MUST also return `accountNotFound`.
    #[tokio::test]
    async fn changes_unknown_account_with_malformed_metadata_type_filter_returns_account_not_found()
    {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "bogus",
            "sinceState": "0",
            "filterMetadataType": "Email"  // wrong shape (must be array)
        });
        let err = handle_metadata_changes(&backend, &(), args)
            .await
            .expect_err("must reject unknown account");
        assert_eq!(err.error_type, "accountNotFound", "got: {err:?}");
    }

    /// Oracle: a known accountId with a malformed `filterRelatedType`
    /// STILL returns `invalidArguments` — the ordering fix did not
    /// remove the filter-shape check, only re-ordered it. Negative
    /// control for the ordering fix.
    #[tokio::test]
    async fn changes_known_account_with_malformed_filter_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "sinceState": "0",
            "filterRelatedType": 42
        });
        let err = handle_metadata_changes(&backend, &(), args)
            .await
            .expect_err("must reject malformed filter shape");
        assert_eq!(
            err.error_type, "invalidArguments",
            "filter-shape check must still run for a known account: {err:?}",
        );
    }

    /// Oracle: `Metadata/query` with an unknown accountId AND a
    /// malformed filter MUST return `accountNotFound`. Before
    /// bd:JMAP-826m.1 + .34, this path would have returned
    /// `unsupportedFilter` because `validate_metadata_filter` ran before
    /// the generic handler's account check.
    #[tokio::test]
    async fn query_unknown_account_with_malformed_filter_returns_account_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "bogus",
            "filter": {
                "related_ids": ["EM1"]  // snake_case typo — unsupportedFilter under .1
            }
        });
        let err = handle_metadata_query(&backend, &(), args)
            .await
            .expect_err("must reject unknown account");
        assert_eq!(
            err.error_type, "accountNotFound",
            "expected accountNotFound to win the error race, got: {err:?}",
        );
    }

    /// Oracle: a known accountId with a malformed filter STILL returns
    /// `unsupportedFilter`. Negative control for the query ordering fix.
    #[tokio::test]
    async fn query_known_account_with_malformed_filter_returns_unsupported_filter() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "filter": {
                "related_ids": ["EM1"]
            }
        });
        let err = handle_metadata_query(&backend, &(), args)
            .await
            .expect_err("must reject malformed filter for known account");
        assert_eq!(err.error_type, "unsupportedFilter", "got: {err:?}");
    }

    /// Oracle: `Metadata/queryChanges` with an unknown accountId AND a
    /// malformed filter MUST return `accountNotFound`.
    #[tokio::test]
    async fn query_changes_unknown_account_with_malformed_filter_returns_account_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "bogus",
            "sinceQueryState": "0",
            "filter": {
                "related_ids": ["EM1"]
            }
        });
        let err = handle_metadata_query_changes(&backend, &(), args)
            .await
            .expect_err("must reject unknown account");
        assert_eq!(err.error_type, "accountNotFound", "got: {err:?}");
    }

    /// Oracle: also validates the §3.4.1 cross-field check (relatedIds
    /// without relatedType) loses to the account check. A malformed
    /// `accountId` should not be masked by a separately-invalid filter.
    #[tokio::test]
    async fn query_unknown_account_with_invalid_filter_cross_field_returns_account_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "bogus",
            "filter": {
                "relatedIds": ["EM1"]  // §3.4.1 violation — invalidArguments under existing path
            }
        });
        let err = handle_metadata_query(&backend, &(), args)
            .await
            .expect_err("must reject unknown account");
        assert_eq!(err.error_type, "accountNotFound", "got: {err:?}");
    }
}
