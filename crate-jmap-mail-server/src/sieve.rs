//! [`SieveBackend`] trait for `SieveScript/get`, `SieveScript/set`,
//! `SieveScript/query`, and `SieveScript/validate` operations
//! (RFC 9661).
//!
//! This module is unconditionally compiled when the `sieve` feature is enabled
//! on `jmap-mail-server`. The feature gate lives in `lib.rs`
//! (`#[cfg(feature = "sieve")]`), not here — this file contains no
//! `#[cfg(…)]` attributes.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/set` → §5.3, `/query` → §5.5), with the
//! type-specific arguments defined by RFC 9661. The returned `Value` is
//! the corresponding method-response object per the same section refs.
//! `SieveScript/validate` (RFC 9661 §2.6) is a Sieve-specific method
//! with its own request/response shape (`accountId`, `blobId` → `error`).
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty** —
//! activation side effects in `SieveScript/set` are applied inline, not
//! as separate invocations.
//!
//! Each handler returns `Err(JmapError)` for method-level failures (RFC 8620
//! §3.6.2 and §5):
//!
//! - every handler: `accountNotFound`, `invalidArguments`, `serverFail`;
//! - `/get`, `/set`: `requestTooLarge` past `maxObjectsInGet` /
//!   `maxObjectsInSet`;
//! - `/set`: `stateMismatch`;
//! - `/query`: `unsupportedFilter`, `unsupportedSort`, `anchorNotFound`.
//!
//! Per-target failures inside `/set` (including the `invalidSieve` and
//! `sieveIsActive` Sieve-specific SetError types defined by RFC 9661) surface
//! in the `notCreated` / `notUpdated` / `notDestroyed` maps within
//! `Ok((Value, ...))`, not as `Err`.

/// IANA-registered JMAP Sieve error type for script validation failures.
pub(crate) const SIEVE_ERR_INVALID: &str = "invalidSieve";
/// IANA-registered JMAP Sieve error type for active-script destroy attempts.
pub(crate) const SIEVE_ERR_IS_ACTIVE: &str = "sieveIsActive";

use jmap_mail_types::{SieveScript, SieveScriptComparator, SieveScriptComparatorProperty};
use jmap_types::{Filter, Id, JmapError, PatchObject};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use crate::MailBackend;
use crate::helpers::{
    SetAccumulators, enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    filter_properties, finalize_set_response,
};
use jmap_server::{BackendSetError, SetError, SetErrorType};
use jmap_server::{
    HandlerOutput, backend_set_error_value, filter_arg, limit_or_max, reject_read_only_patch,
    reject_server_set, resolve_query_offset, server_fail_from_backend,
    server_fail_value_from_backend, take_bool_arg,
};

/// Backend trait for `SieveScript/get`, `SieveScript/set`, `SieveScript/query`,
/// and `SieveScript/validate` operations (RFC 9661).
///
/// `MailBackend` is a supertrait, so sieve stays opt-in: existing
/// `MailBackend` implementors do not need to change.
///
/// # Script access
///
/// The handlers read the script blob with [`MailBackend::read_blob`].
///
/// [`MailBackend`]: crate::MailBackend
/// [`register_sieve_handlers`]: crate::register_sieve_handlers
pub trait SieveBackend: MailBackend {
    /// Validate the syntax of a Sieve script.
    ///
    /// `script` is the blob content the handler already fetched with
    /// [`MailBackend::read_blob`], so each create, update or validate
    /// reads the blob once. The handler has already rejected a missing blob
    /// and one larger than [`SieveBackend::max_sieve_script_bytes`].
    ///
    /// Returns `Ok(None)` if the script is valid, `Ok(Some(description))` if
    /// the script fails validation — the description SHOULD include the line
    /// number of the first error (RFC 9661 §2.6).
    ///
    /// # Errors
    ///
    /// `Err` only for backend failures (for example an unavailable parser
    /// service); an invalid script is `Ok(Some(_))`.
    fn validate_sieve_script(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        script: &[u8],
    ) -> impl std::future::Future<Output = Result<Option<String>, Self::Error>> + Send;

    /// Maximum script size in bytes for this account.
    ///
    /// Used to enforce the `maxSizeScript` capability field. Returns
    /// `Ok(None)` if there is no configured limit (the default).
    ///
    /// When `Ok(Some(n))` is returned, the handler will reject any
    /// create or update whose script blob exceeds `n` bytes with a
    /// `tooLarge` [`SetError`].
    fn max_sieve_script_bytes(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<u64>, Self::Error>> + Send {
        let _ = caller;
        let _ = account_id;
        async move { Ok(None) }
    }

    /// Maximum number of Sieve scripts permitted in this account.
    ///
    /// Used to enforce a per-account script-count quota at create time.
    /// When the next create would push the total above this value, the
    /// handler rejects the create with an `overQuota` [`SetError`]
    /// (RFC 8620 §5.3).
    ///
    /// The default implementation returns `100` — a conservative
    /// reference-impl-grade ceiling that preserves prior in-handler
    /// behaviour. Production backends SHOULD override this with the
    /// site's actual per-account quota; backends that wish to enforce
    /// no count cap may return [`usize::MAX`].
    ///
    /// RFC 9661 does not define a script-count capability field; this
    /// cap is workspace-policy "backend-configurable, not
    /// spec-advertised" (workspace AGENTS.md "Backend caps and
    /// limits"). Consumers that need to surface remaining quota to
    /// clients should use JMAP Quota
    /// (`urn:ietf:params:jmap:quotas`).
    fn max_sieve_scripts_per_account(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<usize, Self::Error>> + Send {
        let _ = caller;
        let _ = account_id;
        async move { Ok(100) }
    }

    /// Returns the [`Id`] of the Sieve script that backs the `VacationResponse`
    /// for this account, if the server implements vacation responses as a stored
    /// Sieve script (RFC 9661 §4).
    ///
    /// Returns `Ok(None)` if the server does not use a VR-backed Sieve script
    /// (the default). Return `Ok(Some(id))` if the account has a VR-backed
    /// script — the handler will then reject destroy and `blobId` updates for
    /// that script with a `forbidden` [`SetError`].
    ///
    /// Returns `Err` only for storage failures.
    fn vacation_response_script_id(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<jmap_types::Id>, Self::Error>> + Send {
        let _ = caller;
        let _ = account_id;
        async move { Ok(None) }
    }
}

/// Filter arguments for `SieveScript/query` (RFC 9661 §4.2).
///
/// Only `name` (substring match) and `isActive` (exact match) are defined by
/// the spec. Both are optional.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SieveScriptFilter {
    name: Option<String>,
    is_active: Option<bool>,
}

/// `true` if `script` matches one `SieveScript/query` FilterCondition.
///
/// `name` is a case-sensitive substring match (a script without a name
/// never matches it); `isActive` is an exact match.
fn sieve_script_matches_condition(script: &SieveScript, cond: &SieveScriptFilter) -> bool {
    cond.name
        .as_deref()
        .is_none_or(|needle| script.name.as_deref().is_some_and(|n| n.contains(needle)))
        && cond
            .is_active
            .is_none_or(|active| script.is_active == active)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate a script name per RFC 9661 §2.1.
///
/// Returns `Some(SetError)` if the name is invalid, `None` if it is acceptable.
fn validate_script_name(name: &str) -> Option<SetError> {
    if name.is_empty() {
        return Some(
            SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["name"])
                .with_description("name must be at least 1 character"),
        );
    }
    for ch in name.chars() {
        let cp = u32::from(ch);
        if cp <= 0x1F || (0x7F..=0x9F).contains(&cp) || cp == 0x2028 || cp == 0x2029 {
            return Some(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["name"])
                    .with_description(
                        "name contains a prohibited character \
                         (spec §2.1: U+0000-U+001F, U+007F-U+009F, U+2028, U+2029 not allowed)",
                    ),
            );
        }
    }
    None
}

// ---------------------------------------------------------------------------
// SieveScript/get handler
// ---------------------------------------------------------------------------

/// Handle a `SieveScript/get` request (RFC 9661 §2.3).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// # notFound contract
///
/// Per RFC 8620 §5.1, `notFound` is always an `Id[]`. When empty it serializes
/// as `[]`, never as `null`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty
/// — `SieveScript/get` is a read-only operation with no side effects.
pub async fn handle_sieve_get<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    // Step 1: Parse request args.
    let (account_id, args) = extract_account_id(args)?;

    let ids: Option<Vec<Id>> = match args.get("ids").unwrap_or(&Value::Null) {
        Value::Null => None,
        v => Some(
            Vec::<Id>::deserialize(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let properties: Option<Vec<String>> = match args.get("properties").unwrap_or(&Value::Null) {
        Value::Null => None,
        v => Some(
            Vec::<String>::deserialize(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    // Step 2: Verify account exists (RFC 8620 §3.6.2).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    enforce_max_objects_in_get(&args, backend.max_objects_in_get(caller, &account_id))?;

    // Step 3: Fetch objects from backend.
    let (list, not_found) = backend
        .get_objects::<SieveScript>(caller, &account_id, ids.as_deref(), None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Step 4: Get state.
    let state = backend
        .get_state::<SieveScript>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Step 5: Serialize list with optional property filtering.
    // RFC 8620 §5.1: when properties is specified, id MUST always be included.
    let list_json: Vec<Value> = if let Some(ref props) = properties {
        let mut prop_set: HashSet<&str> = props.iter().map(String::as_str).collect();
        prop_set.insert("id");
        list.iter()
            .map(|script| {
                let obj = serde_json::to_value(script).map_err(|e| server_fail_from_backend(&e))?;
                Ok(filter_properties(&obj, &prop_set))
            })
            .collect::<Result<Vec<_>, JmapError>>()?
    } else {
        list.iter()
            .map(|script| serde_json::to_value(script).map_err(|e| server_fail_from_backend(&e)))
            .collect::<Result<Vec<_>, JmapError>>()?
    };

    // Step 6: Build response.
    // notFound is always an array ([] not null) per RFC 8620 §5.1.
    let not_found_json: Vec<Value> = not_found
        .iter()
        .map(|id| Value::String(id.as_ref().to_owned()))
        .collect();

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json,
    });

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// SieveScript/set handler
// ---------------------------------------------------------------------------

/// Handle a `SieveScript/set` method call (RFC 9661 §2.4).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps),
/// augmented with the RFC 9661 §2.4 `onSuccessActivateScript` argument;
/// the returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are always
/// empty — activation side effects are applied inline, not as separate
/// invocations.
pub async fn handle_sieve_set<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    // Step 1: Parse request args.
    let (account_id, mut args) = extract_account_id(args)?;

    // Step 2: Verify account exists.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.2). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    // Fetch VacationResponse-backed script id once for the lifetime of this call
    // (RFC 9661 §4). The default impl returns Ok(None).
    let vr_script_id: Option<Id> = backend
        .vacation_response_script_id(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Fetch maxSizeScript limit once for size enforcement (spec §2.4).
    // The default impl returns Ok(None) (no limit).
    let max_script_bytes: Option<u64> = backend
        .max_sieve_script_bytes(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Step 3: ifInState check — always read old_state first.
    let old_state = backend
        .get_state::<SieveScript>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    // Extract the activation side-effect args before consuming the map.
    let on_success_activate_script: Option<String> = match args.remove("onSuccessActivateScript") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "onSuccessActivateScript: expected a string or null, got {}",
                jmap_server::json_type_name(&v)
            )));
        }
    };
    let on_success_deactivate_script: Option<bool> = match args.remove("onSuccessDeactivateScript")
    {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| {
            JmapError::invalid_arguments("onSuccessDeactivateScript: expected a boolean or null")
        })?),
    };

    let mut created: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut not_created: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut updated: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut not_updated: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut destroyed: Vec<Value> = Vec::new();
    let mut not_destroyed: serde_json::Map<String, Value> = serde_json::Map::new();

    // Map from creation_id to the server-assigned Id (for onSuccessActivateScript ref resolution).
    let mut created_id_map: HashMap<String, Id> = HashMap::new();
    // Reverse map: assigned Id → creation_id (for R5: patching isActive into created entries).
    let mut created_id_reverse_map: HashMap<Id, String> = HashMap::new();

    // Step 4: Process destroys.
    if let Some(destroy_ids) = args.remove("destroy").and_then(|v| match v {
        Value::Array(a) => Some(a),
        _ => None,
    }) {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        let id_strs: Vec<&str> = destroy_ids
            .iter()
            .map(|v| {
                v.as_str().ok_or_else(|| {
                    JmapError::invalid_arguments(format!(
                        "destroy: every element must be a string Id; got {}",
                        jmap_server::json_type_name(v)
                    ))
                })
            })
            .collect::<Result<_, _>>()?;
        for id_str in id_strs {
            let id = match jmap_server::parse_wire_id(id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str.to_owned(), serde_json::Value::from(&*err));
                    continue;
                }
            };

            // Check existence and isActive before destroying.
            let (existing, not_found_ids) = backend
                .get_objects::<SieveScript>(
                    caller,
                    &account_id,
                    Some(std::slice::from_ref(&id)),
                    None,
                )
                .await
                .map_err(|e| server_fail_from_backend(&e))?;

            if !not_found_ids.is_empty() || existing.is_empty() {
                not_destroyed.insert(
                    id_str.to_owned(),
                    serde_json::Value::from(&SetError::new(SetErrorType::NotFound)),
                );
                continue;
            }

            if existing[0].is_active {
                not_destroyed.insert(
                    id_str.to_owned(),
                    serde_json::Value::from(
                        &SetError::new(SetErrorType::from(SIEVE_ERR_IS_ACTIVE))
                            .with_description("cannot destroy the active Sieve script"),
                    ),
                );
                continue;
            }

            // VR-backed script guard (after sieveIsActive — active VR script takes
            // priority; inactive VR script gets forbidden per draft §4).
            if vr_script_id.as_ref() == Some(&id) {
                not_destroyed.insert(
                    id_str.to_owned(),
                    serde_json::Value::from(
                        &SetError::new(SetErrorType::Forbidden).with_description(
                            "this script is managed by VacationResponse/set and cannot be \
                             destroyed via SieveScript/set",
                        ),
                    ),
                );
                continue;
            }

            match backend
                .destroy_object::<SieveScript>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    not_destroyed.insert(id_str.to_owned(), backend_set_error_value(e));
                }
            }
        }
    }

    // Step 5: Process creates.
    //
    // R7: Hoist get_objects call before the loop so we can check overQuota
    // and uniqueness without an extra backend round-trip per create.
    let (all_scripts_before_create, _) = backend
        .get_objects::<SieveScript>(caller, &account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Per-account script-count cap. Hoisted once for the same reason as
    // get_objects above — the cap is a backend property, not a per-create
    // decision, so a single fetch suffices for the whole loop.
    let max_scripts = backend
        .max_sieve_scripts_per_account(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let mut successful_creates: usize = 0;
    // Track names successfully created within this call to detect intra-call duplicates.
    let mut names_created_this_call: HashSet<String> = HashSet::new();

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (creation_id, obj_val) in create_map {
            if let Some(err) = obj_val
                .as_object()
                .and_then(reject_server_set::<SieveScript>)
            {
                not_created.insert(creation_id, serde_json::Value::from(&err));
                continue;
            }
            // R7: overQuota check.
            if all_scripts_before_create.len() + successful_creates >= max_scripts {
                not_created.insert(
                    creation_id,
                    serde_json::Value::from(&SetError::new(SetErrorType::OverQuota)),
                );
                continue;
            }

            // a. blob_id is required.
            let Some(blob_id_str) = obj_val.get("blobId").and_then(|v| v.as_str()) else {
                not_created.insert(
                    creation_id,
                    serde_json::Value::from(
                        &SetError::new(SetErrorType::InvalidProperties)
                            .with_properties(["blobId"])
                            .with_description("blobId is required"),
                    ),
                );
                continue;
            };
            let blob_id = match jmap_server::parse_wire_id(blob_id_str, "blobId") {
                Ok(id) => id,
                Err(err) => {
                    not_created.insert(creation_id, serde_json::Value::from(&*err));
                    continue;
                }
            };

            // b. Name validation and uniqueness check (if name is provided and non-null).
            let name: Option<String> = obj_val
                .get("name")
                .and_then(|v| if v.is_null() { None } else { v.as_str() })
                .map(|s| s.to_owned());

            if let Some(ref name_str) = name {
                // R2: validate name characters.
                if let Some(name_err) = validate_script_name(name_str) {
                    not_created.insert(creation_id, serde_json::Value::from(&name_err));
                    continue;
                }

                // Intra-call duplicate: another create in this same request already
                // succeeded with this name (we don't have the new id yet, so omit
                // existingId — the spec only requires it for DB-level collisions).
                if names_created_this_call.contains(name_str.as_str()) {
                    not_created.insert(
                        creation_id,
                        serde_json::Value::from(
                            &SetError::new(SetErrorType::AlreadyExists).with_description(
                                "a script with this name was already created in this request",
                            ),
                        ),
                    );
                    continue;
                }

                // R7 + R6: uniqueness check using the pre-fetched all_scripts list.
                if let Some(existing) = all_scripts_before_create
                    .iter()
                    .find(|s| s.name.as_deref() == Some(name_str.as_str()))
                {
                    not_created.insert(
                        creation_id,
                        serde_json::Value::from(
                            &SetError::new(SetErrorType::AlreadyExists)
                                .with_existing_id(existing.id.clone()),
                        ),
                    );
                    continue;
                }
            }

            // c. Fetch the script once; check maxSizeScript, then syntax.
            match check_script(backend, caller, &account_id, &blob_id, max_script_bytes).await {
                Ok(None) => {}
                Ok(Some(err)) => {
                    not_created.insert(creation_id, serde_json::Value::from(&err));
                    continue;
                }
                Err(e) => {
                    not_created.insert(creation_id, server_fail_value_from_backend(&e));
                    continue;
                }
            }

            // d. Build SieveScript object — is_active always false on creation.
            let script_name = name.clone();
            let build = move |id: &Id| {
                let mut script = SieveScript::new(id.clone(), blob_id, false);
                script.name = script_name;
                script
            };

            // e. Persist via backend.
            match backend
                .create_object::<SieveScript, _>(caller, &account_id, &creation_id, build)
                .await
            {
                Ok((assigned_id, created_obj)) => {
                    // f. Record creation_id → assigned_id for onSuccessActivateScript.
                    created_id_map.insert(creation_id.clone(), assigned_id.clone());
                    created_id_reverse_map.insert(assigned_id, creation_id.clone());

                    // Track name for intra-call duplicate detection.
                    if let Some(ref n) = name {
                        names_created_this_call.insert(n.clone());
                    }

                    // R9: propagate serialization errors via ? instead of silently swallowing.
                    let obj_json = serde_json::to_value(&created_obj)
                        .map_err(|e| server_fail_from_backend(&e))?;
                    created.insert(creation_id, obj_json);
                    successful_creates += 1;
                }
                Err(e) => {
                    not_created.insert(creation_id, backend_set_error_value(e));
                }
            }
        }
    }

    // Step 6: Process updates.
    if let Some(Value::Object(update_map)) = args.remove("update") {
        // For name uniqueness checks on update, fetch current scripts once.
        let (all_scripts_for_update, _) = backend
            .get_objects::<SieveScript>(caller, &account_id, None, None)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        for (id_str, patch_val) in update_map {
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            // R4: Reject patches to server-set properties: `id` and
            // `isActive` (RFC 9661 §2), which onSuccessActivateScript changes.
            if let Some(err) = reject_read_only_patch::<SieveScript>(patch.as_map(), None) {
                not_updated.insert(id_str, serde_json::Value::from(&err));
                continue;
            }

            // VR-backed script guard: reject blobId changes on the VR-backed script
            // (RFC 9661 §4). isActive changes are handled separately
            // by the activation state machine and are not blocked here.
            if let Some(ref vr_id) = vr_script_id
                && vr_id == &id
                && patch.as_map().contains_key("blobId")
            {
                not_updated.insert(
                    id_str,
                    serde_json::Value::from(
                        &SetError::new(SetErrorType::Forbidden).with_description(
                            "blobId of a VacationResponse-backed script cannot be \
                                 updated via SieveScript/set",
                        ),
                    ),
                );
                continue;
            }

            // R2: validate name character if patch contains "name".
            if let Some(name_val) = patch.as_map().get("name")
                && let Some(name_str) = name_val.as_str()
            {
                if let Some(name_err) = validate_script_name(name_str) {
                    not_updated.insert(id_str, serde_json::Value::from(&name_err));
                    continue;
                }

                // R6: uniqueness check — exclude the script being updated.
                if let Some(existing) = all_scripts_for_update
                    .iter()
                    .find(|s| s.name.as_deref() == Some(name_str) && s.id != id)
                {
                    not_updated.insert(
                        id_str,
                        serde_json::Value::from(
                            &SetError::new(SetErrorType::AlreadyExists)
                                .with_existing_id(existing.id.clone()),
                        ),
                    );
                    continue;
                }
            }

            // If the patch includes blobId, check size then validate the new blob.
            if let Some(new_blob_id_str) = patch.as_map().get("blobId").and_then(|v| v.as_str()) {
                let new_blob_id = match jmap_server::parse_wire_id(new_blob_id_str, "blobId") {
                    Ok(id) => id,
                    Err(err) => {
                        not_updated.insert(id_str, serde_json::Value::from(&*err));
                        continue;
                    }
                };

                // Fetch the script once; check maxSizeScript, then syntax.
                match check_script(backend, caller, &account_id, &new_blob_id, max_script_bytes)
                    .await
                {
                    Ok(None) => {}
                    Ok(Some(err)) => {
                        not_updated.insert(id_str, serde_json::Value::from(&err));
                        continue;
                    }
                    Err(e) => {
                        not_updated.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            match backend
                .update_object::<SieveScript>(caller, &account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    // R9: propagate serialization errors via ? instead of silently swallowing.
                    let obj_json =
                        serde_json::to_value(&obj).map_err(|e| server_fail_from_backend(&e))?;
                    updated.insert(id_str, obj_json);
                }
                Ok(None) => {
                    updated.insert(id_str, Value::Null);
                }
                Err(e) => {
                    not_updated.insert(id_str, backend_set_error_value(e));
                }
            }
        }
    }

    // Step 7: Activation state machine (draft §2.4).
    //
    // R1: Per spec §2.4, activation side-effects only run if ALL operations succeeded.
    let any_failure =
        !not_created.is_empty() || !not_updated.is_empty() || !not_destroyed.is_empty();

    if !any_failure {
        // Step A: onSuccessDeactivateScript
        if on_success_deactivate_script == Some(true) {
            let (all_scripts, _) = backend
                .get_objects::<SieveScript>(caller, &account_id, None, None)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;
            if let Some(active_script) = all_scripts.iter().find(|s| s.is_active) {
                let active_id = active_script.id.clone();
                let active_id_str = active_id.as_ref().to_owned();
                // Build a one-key PatchObject {"isActive": false}.
                let mut patch_map = serde_json::Map::new();
                patch_map.insert("isActive".to_owned(), Value::Bool(false));
                let patch = PatchObject::from_map(patch_map);
                match backend
                    .update_object::<SieveScript>(caller, &account_id, &active_id, patch)
                    .await
                {
                    Ok(_) => {
                        updated.insert(active_id_str, json!({ "isActive": false }));
                    }
                    Err(e) => {
                        not_updated.insert(active_id_str, backend_set_error_value(e));
                    }
                }
            }
        }

        // Step B: onSuccessActivateScript
        if let Some(ref activate_ref) = on_success_activate_script {
            // Resolve the target Id: "#creation_id" or bare Id.
            let target_id: Option<Id> = if let Some(creation_id) = activate_ref.strip_prefix('#') {
                // Look up in the created map; silently ignore if not found (spec §2.4).
                created_id_map.get(creation_id).cloned()
            } else {
                Id::new_validated(activate_ref.as_str()).ok()
            };

            if let Some(ref target_id) = target_id {
                // Deactivate the current active script if it's different from target
                // and hasn't already been deactivated in Step A.
                let (all_scripts, _) = backend
                    .get_objects::<SieveScript>(caller, &account_id, None, None)
                    .await
                    .map_err(|e| server_fail_from_backend(&e))?;

                // Track whether deactivation failed so we can abort the activate step
                // (Fix R2_3: deactivate failure aborts activate to preserve one-active invariant).
                let mut deactivation_failed = false;

                if let Some(currently_active) = all_scripts.iter().find(|s| s.is_active)
                    && &currently_active.id != target_id
                {
                    let active_id = currently_active.id.clone();
                    let active_id_str = active_id.as_ref().to_owned();
                    // Only deactivate if Step A hasn't already done so for this script.
                    if !updated.contains_key(&active_id_str) {
                        let mut patch_map = serde_json::Map::new();
                        patch_map.insert("isActive".to_owned(), Value::Bool(false));
                        let patch = PatchObject::from_map(patch_map);
                        match backend
                            .update_object::<SieveScript>(caller, &account_id, &active_id, patch)
                            .await
                        {
                            Ok(_) => {
                                updated.insert(active_id_str, json!({ "isActive": false }));
                            }
                            Err(e) => {
                                not_updated.insert(active_id_str, backend_set_error_value(e));
                                deactivation_failed = true;
                            }
                        }
                    }
                }

                // If deactivation failed, abort activation to preserve the one-active invariant.
                if !deactivation_failed {
                    // Activate the target script.
                    let target_id_str = target_id.as_ref().to_owned();
                    let mut patch_map = serde_json::Map::new();
                    patch_map.insert("isActive".to_owned(), Value::Bool(true));
                    let patch = PatchObject::from_map(patch_map);
                    match backend
                        .update_object::<SieveScript>(caller, &account_id, target_id, patch)
                        .await
                    {
                        Ok(_) => {
                            // R5: If the target was just created in this same set call,
                            // reflect isActive:true in the `created` map entry and do NOT
                            // add it to `updated` (spec §2.4 example: activated-on-create
                            // scripts appear in `created` with isActive:true, not `updated`).
                            if let Some(creation_id) = created_id_reverse_map.get(target_id) {
                                // Patch the created entry to show isActive:true.
                                if let Some(entry) = created.get_mut(creation_id) {
                                    if let Some(obj) = entry.as_object_mut() {
                                        obj.insert("isActive".to_owned(), json!(true));
                                    } else {
                                        *entry = json!({ "isActive": true });
                                    }
                                }
                                // Do NOT insert into `updated` — the script is new to the client.
                            } else {
                                // Existing script: merge with or insert into updated.
                                let entry =
                                    updated.entry(target_id_str).or_insert_with(|| json!({}));
                                if let Some(obj) = entry.as_object_mut() {
                                    obj.insert("isActive".to_owned(), json!(true));
                                } else {
                                    *entry = json!({ "isActive": true });
                                }
                            }
                        }
                        // Fix R2_1: nonexistent bare id → spec §2.4 says silently ignore.
                        Err(BackendSetError::SetError(ref se))
                            if se.error_type == SetErrorType::NotFound =>
                        {
                            // Silently skip — no entry in not_updated.
                        }
                        Err(e) => {
                            not_updated.insert(target_id_str, backend_set_error_value(e));
                        }
                    }
                }
            }
        }
    }

    // Step 8: Build response.
    //
    // `mutated` is computed from the result accumulators rather than tracked
    // through the handler body — equivalent in effect, since the only way an
    // entry lands in `created`/`updated`/`destroyed` is on a successful
    // backend mutation. Lets the helper skip the `get_state` round-trip when
    // every operation failed (matches the gating in calendars-server).
    let mutated = !created.is_empty() || !updated.is_empty() || !destroyed.is_empty();
    finalize_set_response::<B, SieveScript>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        SetAccumulators {
            created,
            updated,
            destroyed,
            not_created,
            not_updated,
            not_destroyed,
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// SieveScript/query handler
// ---------------------------------------------------------------------------

/// Handle a `SieveScript/query` method call (RFC 9661 §4.2).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// `SieveScript` implements `GetObject` but not `QueryObject`, so the backend
/// `query_objects` generic is unavailable. We fetch all scripts and apply
/// filter/sort/pagination in-handler. Accounts typically have very few scripts
/// (O(10)), so a full scan is cheap.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_sieve_query<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    // Step 1: extract and verify account.
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // Step 2: parse filter (optional). RFC 8620 §5.5: the filter is
    // "FilterOperator|FilterCondition|null", so AND/OR/NOT trees decode too.
    let filter: Option<Filter<SieveScriptFilter>> = filter_arg(&mut args)?;

    // Step 3: parse pagination arguments.
    let position: i64 = match args.remove("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "position: expected an integer, got {}",
                jmap_server::json_type_name(&v)
            ))
        })?,
    };

    let limit: Option<u64> = match args.remove("limit") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "limit: expected a non-negative integer, got {}",
                jmap_server::json_type_name(&v)
            ))
        })?),
    };

    let anchor: Option<Id> = match args.remove("anchor") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            Id::new_validated(s.as_str())
                .map_err(|e| JmapError::invalid_arguments(format!("anchor: {e}")))?,
        ),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "anchor: expected an Id string or null, got {}",
                jmap_server::json_type_name(&v)
            )));
        }
    };

    let anchor_offset: i64 = match args.remove("anchorOffset") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "anchorOffset: expected an integer, got {}",
                jmap_server::json_type_name(&v)
            ))
        })?,
    };

    let calculate_total: bool = take_bool_arg(&mut args, "calculateTotal", false);

    // RFC 8620 §5.5: a sort property the server does not support, or "a
    // collation method it does not recognise", is unsupportedSort; RFC 9661
    // §2.5 requires `name` and `isActive`. Names compare byte-wise, so
    // `i;octet` (RFC 4790 §9.3) is the only collation accepted.
    let sort: Vec<SieveScriptComparator> = match args.remove("sort") {
        None | Some(Value::Null) => vec![],
        Some(v) => serde_json::from_value(v)
            .map_err(|e| JmapError::invalid_arguments(format!("sort: {e}")))?,
    };
    if sort.iter().any(|c| {
        matches!(c.property, SieveScriptComparatorProperty::Other(_))
            || c.collation.as_deref().is_some_and(|k| k != "i;octet")
    }) {
        return Err(JmapError::unsupported_sort());
    }

    // Step 4: fetch all scripts for the account.
    let (all_scripts, _not_found) = backend
        .get_objects::<SieveScript>(caller, &account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Step 5: apply filter.
    let filtered: Vec<&SieveScript> = all_scripts
        .iter()
        .filter(|s| {
            filter
                .as_ref()
                .is_none_or(|f| f.matches(&|c| sieve_script_matches_condition(s, c)))
        })
        .collect();

    // Step 6: sort by the comparators in order; with no `sort`, by name
    // ascending (the order is server-dependent, RFC 8620 §5.5). A null name
    // sorts before any string; `false` sorts before `true`.
    let mut sorted: Vec<&SieveScript> = filtered;
    if sort.is_empty() {
        sorted.sort_by(|a, b| a.name.as_deref().cmp(&b.name.as_deref()));
    } else {
        sorted.sort_by(|a, b| {
            sort.iter()
                .map(|c| {
                    let ord = match c.property {
                        SieveScriptComparatorProperty::IsActive => a.is_active.cmp(&b.is_active),
                        SieveScriptComparatorProperty::Name => {
                            a.name.as_deref().cmp(&b.name.as_deref())
                        }
                        // `Other` was rejected above; the enum is non_exhaustive.
                        _ => std::cmp::Ordering::Equal,
                    };
                    if c.is_ascending { ord } else { ord.reverse() }
                })
                .find(|o| o.is_ne())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // Build the ordered Id list.
    let all_ids: Vec<&Id> = sorted.iter().map(|s| &s.id).collect();
    let total_count = all_ids.len();

    // Step 7: resolve start position (anchor overrides position).
    let start: usize = if let Some(ref anchor_id) = anchor {
        let anchor_idx = all_ids
            .iter()
            .position(|id| *id == anchor_id)
            .ok_or_else(JmapError::anchor_not_found)?;
        // RFC 8620 §5.5: anchorOffset is relative to the anchor; a
        // negative result clamps to 0 (not end-relative), then to len.
        let raw = i64::try_from(anchor_idx)
            .unwrap_or(i64::MAX)
            .saturating_add(anchor_offset)
            .max(0);
        resolve_query_offset(raw, total_count)
    } else {
        // bd:JMAP-qz9v.48 — centralized in jmap_server::resolve_query_offset.
        resolve_query_offset(position, total_count)
    };

    // Step 8: apply limit.
    let page_ids: Vec<&Id> = all_ids
        .iter()
        .skip(start)
        .take(limit_or_max(limit))
        .copied()
        .collect();

    // Step 9: get query state.
    let query_state = backend
        .get_state::<SieveScript>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Step 10: build response.
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": query_state.as_ref(),
        "canCalculateChanges": false,
        "position": start,
        "ids": page_ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });

    // RFC 8620 §5.5: include total only when calculateTotal=true.
    if calculate_total {
        resp["total"] = json!(total_count);
    }

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// SieveScript/validate handler
// ---------------------------------------------------------------------------

/// Fetch a script blob once and check it for create, update and validate.
///
/// Returns `Ok(None)` when the script is acceptable, or the [`SetError`]
/// to report: `invalidSieve` for a missing blob or bad syntax, `tooLarge`
/// when it exceeds `max_bytes` (the `maxSizeScript` limit, RFC 9661 §2.4).
/// The size check runs before the backend parser sees the bytes.
async fn check_script<B: SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    blob_id: &Id,
    max_bytes: Option<u64>,
) -> Result<Option<SetError>, B::Error> {
    let Some(script) = backend.read_blob(caller, account_id, blob_id).await? else {
        return Ok(Some(
            SetError::new(SetErrorType::from(SIEVE_ERR_INVALID)).with_description("blob not found"),
        ));
    };
    if let Some(max) = max_bytes
        && script.len() as u64 > max
    {
        return Ok(Some(
            SetError::new(SetErrorType::TooLarge)
                .with_description(format!("script exceeds maxSizeScript limit of {max} bytes")),
        ));
    }
    Ok(backend
        .validate_sieve_script(caller, account_id, &script)
        .await?
        .map(|desc| SetError::new(SetErrorType::from(SIEVE_ERR_INVALID)).with_description(desc)))
}

/// Handle a `SieveScript/validate` method call (RFC 9661 §2.6).
///
/// `args` is the RFC 9661 §2.6 `SieveScript/validate` request shape
/// (`accountId`, `blobId`); the returned `Value` is the §2.6 response
/// shape (`accountId`, `error` — either `null` for valid scripts or
/// a `SetError` describing why the script is invalid).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_sieve_validate<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    // Step 1: Extract accountId.
    let (account_id, args) = crate::helpers::extract_account_id(args)?;

    // Step 2: Extract blobId.
    let blob_id: Id = match args.get("blobId").and_then(|v| v.as_str()) {
        Some(s) => Id::new_validated(s)
            .map_err(|e| JmapError::invalid_arguments(format!("blobId: {e}")))?,
        None => return Err(JmapError::invalid_arguments("blobId is required")),
    };

    // Step 3: Verify account exists (RFC 8620 §3.6.2).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // Step 4: Fetch the script once; enforce maxSizeScript (the same check
    // as the set paths), then delegate syntax validation to the backend.
    let max_script_bytes: Option<u64> = backend
        .max_sieve_script_bytes(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let rejection = check_script(backend, caller, &account_id, &blob_id, max_script_bytes)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Step 5: Build response — "error" field MUST be present as null when valid
    // (RFC 9661 §2.6).
    let error_value = match rejection {
        None => Value::Null,
        Some(err) => serde_json::Value::from(&err),
    };
    let resp = json!({
        "accountId": account_id.as_ref(),
        "error": error_value,
    });
    Ok(HandlerOutput::new(resp))
}
