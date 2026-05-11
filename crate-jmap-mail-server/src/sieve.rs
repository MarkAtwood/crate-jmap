//! [`SieveBackend`] trait for `SieveScript/get`, `SieveScript/set`,
//! `SieveScript/query`, and `SieveScript/validate` operations
//! (RFC 9661).
//!
//! This module is unconditionally compiled when the `sieve` feature is enabled
//! on `jmap-mail-server`. The feature gate lives in `lib.rs`
//! (`#[cfg(feature = "sieve")]`), not here — this file contains no
//! `#[cfg(…)]` attributes.

/// IANA-registered JMAP Sieve error type for script validation failures.
pub(crate) const SIEVE_ERR_INVALID: &str = "invalidSieve";
/// IANA-registered JMAP Sieve error type for active-script destroy attempts.
pub(crate) const SIEVE_ERR_IS_ACTIVE: &str = "sieveIsActive";

/// Maximum number of Sieve scripts per account.
///
/// A real backend would derive this from the account's `SieveScripts` capability,
/// but for the test backend a hard ceiling of 100 is a reasonable default.
const MAX_SIEVE_SCRIPTS: usize = 100;

use jmap_mail_types::SieveScript;
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use crate::backend::{BackendSetError, MailBackend, SetError, SetErrorType};
use crate::helpers::{
    extract_account_id, filter_properties, finalize_set_response, set_error_value, SetAccumulators,
};

/// Backend trait for `SieveScript/get`, `SieveScript/set`, `SieveScript/query`,
/// and `SieveScript/validate` operations (RFC 9661).
///
/// Implementors also implement [`MailBackend`] — the generic bounds on
/// [`register_sieve_handlers`] require both. This separation keeps sieve opt-in.
///
/// # Script access
///
/// `SieveScript/validate` needs raw UTF-8 bytes for the script blob.
/// [`SieveBackend::get_sieve_blob`] exposes this at the trait level.
///
/// [`MailBackend`]: crate::backend::MailBackend
/// [`register_sieve_handlers`]: crate::register_sieve_handlers
pub trait SieveBackend: jmap_server::JmapBackend {
    /// Fetch raw bytes for a script blob by ID.
    ///
    /// Returns `Ok(Some(bytes))` if found, `Ok(None)` if the blob does not
    /// exist in this account, and `Err` for storage failures.
    fn get_sieve_blob(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;

    /// Validate the content of a Sieve script blob.
    ///
    /// Returns `Ok(None)` if the script is valid, `Ok(Some(description))` if
    /// the script fails validation — the description SHOULD include the line
    /// number of the first error (per draft §2.6).
    ///
    /// Returns `Err` only for storage failures. If the blob does not exist,
    /// return `Ok(Some("blob not found"))`.
    ///
    /// Note: this method validates script syntax only. Script blob size should
    /// be checked separately using [`SieveBackend::max_sieve_script_bytes`]
    /// before calling this method.
    fn validate_sieve_script(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
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
#[serde(rename_all = "camelCase")]
struct SieveScriptFilter {
    name: Option<String>,
    is_active: Option<bool>,
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
        let cp = ch as u32;
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
/// Returns `(response_args, extra_invocations)`. Extra invocations are always
/// empty — `SieveScript/get` is a read-only operation with no side effects.
///
/// # notFound contract
///
/// Per RFC 8620 §5.1, `notFound` is always an `Id[]`. When empty it serializes
/// as `[]`, never as `null`.
pub async fn handle_sieve_get<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
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
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

    // Step 3: Fetch objects from backend.
    let (list, not_found) = backend
        .get_objects::<SieveScript>(caller, &account_id, ids.as_deref(), None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // Step 4: Get state.
    let state = backend
        .get_state::<SieveScript>(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // Step 5: Serialize list with optional property filtering.
    // RFC 8620 §5.1: when properties is specified, id MUST always be included.
    let list_json: Vec<Value> = if let Some(ref props) = properties {
        let mut prop_set: HashSet<&str> = props.iter().map(String::as_str).collect();
        prop_set.insert("id");
        list.iter()
            .map(|script| {
                let obj = serde_json::to_value(script)
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;
                Ok(filter_properties(&obj, &prop_set))
            })
            .collect::<Result<Vec<_>, JmapError>>()?
    } else {
        list.iter()
            .map(|script| {
                serde_json::to_value(script).map_err(|e| JmapError::server_fail(e.to_string()))
            })
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

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// SieveScript/set handler
// ---------------------------------------------------------------------------

/// Handle a `SieveScript/set` method call (RFC 9661 §2.4).
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are always
/// empty — activation side effects are applied inline, not as separate
/// invocations.
pub async fn handle_sieve_set<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // Step 1: Parse request args.
    let (account_id, mut args) = extract_account_id(args)?;

    // Step 2: Verify account exists.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

    // Fetch VacationResponse-backed script id once for the lifetime of this call
    // (RFC 9661 §4). The default impl returns Ok(None).
    let vr_script_id: Option<Id> = backend
        .vacation_response_script_id(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // Fetch maxSizeScript limit once for size enforcement (spec §2.4).
    // The default impl returns Ok(None) (no limit).
    let max_script_bytes: Option<u64> = backend
        .max_sieve_script_bytes(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // Step 3: ifInState check — always read old_state first.
    let old_state = backend
        .get_state::<SieveScript>(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    // Extract the activation side-effect args before consuming the map.
    let on_success_activate_script: Option<String> = match args.remove("onSuccessActivateScript") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "onSuccessActivateScript: expected a string or null, got {v}"
            )))
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
        if let Some(bad) = destroy_ids.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy: every element must be a string Id; got {bad}"
            )));
        }
        for id_val in &destroy_ids {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue, // unreachable: validated above
            };
            let id = Id::from(id_str);

            // Check existence and isActive before destroying.
            let (existing, not_found_ids) = backend
                .get_objects::<SieveScript>(
                    caller,
                    &account_id,
                    Some(std::slice::from_ref(&id)),
                    None,
                )
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;

            if !not_found_ids.is_empty() || existing.is_empty() {
                not_destroyed.insert(
                    id_str.to_owned(),
                    set_error_value(&SetError::new(SetErrorType::NotFound)),
                );
                continue;
            }

            if existing[0].is_active {
                not_destroyed.insert(
                    id_str.to_owned(),
                    set_error_value(
                        &SetError::new(SetErrorType::custom(SIEVE_ERR_IS_ACTIVE))
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
                    set_error_value(&SetError::new(SetErrorType::Forbidden).with_description(
                        "this script is managed by VacationResponse/set and cannot be \
                             destroyed via SieveScript/set",
                    )),
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
                Err(BackendSetError::SetError(se)) => {
                    not_destroyed.insert(id_str.to_owned(), set_error_value(&se));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
                Err(_) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
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
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let mut successful_creates: usize = 0;
    // Track names successfully created within this call to detect intra-call duplicates.
    let mut names_created_this_call: HashSet<String> = HashSet::new();

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (creation_id, obj_val) in create_map {
            // R7: overQuota check.
            if all_scripts_before_create.len() + successful_creates >= MAX_SIEVE_SCRIPTS {
                not_created.insert(
                    creation_id,
                    set_error_value(&SetError::new(SetErrorType::OverQuota)),
                );
                continue;
            }

            // a. blob_id is required.
            let blob_id_str = match obj_val.get("blobId").and_then(|v| v.as_str()) {
                Some(s) => s.to_owned(),
                None => {
                    not_created.insert(
                        creation_id,
                        set_error_value(
                            &SetError::new(SetErrorType::InvalidProperties)
                                .with_properties(["blobId"])
                                .with_description("blobId is required"),
                        ),
                    );
                    continue;
                }
            };
            let blob_id = Id::from(blob_id_str.as_str());

            // b. Name validation and uniqueness check (if name is provided and non-null).
            let name: Option<String> = obj_val
                .get("name")
                .and_then(|v| if v.is_null() { None } else { v.as_str() })
                .map(|s| s.to_owned());

            if let Some(ref name_str) = name {
                // R2: validate name characters.
                if let Some(name_err) = validate_script_name(name_str) {
                    not_created.insert(creation_id, set_error_value(&name_err));
                    continue;
                }

                // Intra-call duplicate: another create in this same request already
                // succeeded with this name (we don't have the new id yet, so omit
                // existingId — the spec only requires it for DB-level collisions).
                if names_created_this_call.contains(name_str.as_str()) {
                    not_created.insert(
                        creation_id,
                        set_error_value(
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
                        set_error_value(
                            &SetError::new(SetErrorType::AlreadyExists)
                                .with_existing_id(existing.id.clone()),
                        ),
                    );
                    continue;
                }
            }

            // c. Size check: enforce maxSizeScript before syntax validation.
            // Per spec §2.4: if the script exceeds the limit, reject with tooLarge.
            if let Some(max_bytes) = max_script_bytes {
                match backend.get_sieve_blob(caller, &account_id, &blob_id).await {
                    Ok(Some(ref bytes)) if bytes.len() as u64 > max_bytes => {
                        not_created.insert(
                            creation_id,
                            set_error_value(
                                &SetError::new(SetErrorType::TooLarge).with_description(format!(
                                    "script exceeds maxSizeScript limit of {max_bytes} bytes"
                                )),
                            ),
                        );
                        continue;
                    }
                    Ok(_) => {} // size OK or blob missing (missing blob handled by validate_sieve_script)
                    Err(e) => {
                        not_created.insert(
                            creation_id,
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                        continue;
                    }
                }
            }

            // d. Validate the Sieve script syntax.
            if let Some(err_desc) = backend
                .validate_sieve_script(caller, &account_id, &blob_id)
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?
            {
                not_created.insert(
                    creation_id,
                    set_error_value(
                        &SetError::new(SetErrorType::custom(SIEVE_ERR_INVALID))
                            .with_description(err_desc),
                    ),
                );
                continue;
            }

            // e. Build SieveScript object — is_active always false on creation.
            let mut script = SieveScript::new(Id::from("placeholder"), blob_id, false);
            script.name = name.clone();

            // f. Persist via backend.
            match backend
                .create_object::<SieveScript>(caller, &account_id, &creation_id, script)
                .await
            {
                Ok((assigned_id, created_obj)) => {
                    // g. Record creation_id → assigned_id for onSuccessActivateScript.
                    created_id_map.insert(creation_id.clone(), assigned_id.clone());
                    created_id_reverse_map.insert(assigned_id, creation_id.clone());

                    // Track name for intra-call duplicate detection.
                    if let Some(ref n) = name {
                        names_created_this_call.insert(n.clone());
                    }

                    // R9: propagate serialization errors via ? instead of silently swallowing.
                    let obj_json = serde_json::to_value(&created_obj)
                        .map_err(|e| JmapError::server_fail(e.to_string()))?;
                    created.insert(creation_id, obj_json);
                    successful_creates += 1;
                }
                Err(BackendSetError::SetError(se)) => {
                    not_created.insert(creation_id, set_error_value(&se));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(
                        creation_id,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
                Err(_) => {
                    not_created.insert(
                        creation_id,
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
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
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

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

            // R4: Reject direct isActive patches — it is server-set.
            if patch.as_map().contains_key("isActive") {
                not_updated.insert(
                    id_str,
                    set_error_value(
                        &SetError::new(SetErrorType::InvalidProperties)
                            .with_properties(["isActive"])
                            .with_description(
                                "isActive is server-set and must not be patched directly; \
                                 use onSuccessActivateScript",
                            ),
                    ),
                );
                continue;
            }

            // VR-backed script guard: reject blobId changes on the VR-backed script
            // (RFC 9661 §4). isActive changes are handled separately
            // by the activation state machine and are not blocked here.
            if let Some(ref vr_id) = vr_script_id {
                if vr_id == &id && patch.as_map().contains_key("blobId") {
                    not_updated.insert(
                        id_str,
                        set_error_value(&SetError::new(SetErrorType::Forbidden).with_description(
                            "blobId of a VacationResponse-backed script cannot be \
                                 updated via SieveScript/set",
                        )),
                    );
                    continue;
                }
            }

            // R2: validate name character if patch contains "name".
            if let Some(name_val) = patch.as_map().get("name") {
                if let Some(name_str) = name_val.as_str() {
                    if let Some(name_err) = validate_script_name(name_str) {
                        not_updated.insert(id_str, set_error_value(&name_err));
                        continue;
                    }

                    // R6: uniqueness check — exclude the script being updated.
                    if let Some(existing) = all_scripts_for_update
                        .iter()
                        .find(|s| s.name.as_deref() == Some(name_str) && s.id != id)
                    {
                        not_updated.insert(
                            id_str,
                            set_error_value(
                                &SetError::new(SetErrorType::AlreadyExists)
                                    .with_existing_id(existing.id.clone()),
                            ),
                        );
                        continue;
                    }
                }
            }

            // If the patch includes blobId, check size then validate the new blob.
            if let Some(new_blob_id_str) = patch.as_map().get("blobId").and_then(|v| v.as_str()) {
                let new_blob_id = Id::from(new_blob_id_str);

                // Size check: enforce maxSizeScript before syntax validation (spec §2.4).
                if let Some(max_bytes) = max_script_bytes {
                    match backend
                        .get_sieve_blob(caller, &account_id, &new_blob_id)
                        .await
                    {
                        Ok(Some(ref bytes)) if bytes.len() as u64 > max_bytes => {
                            not_updated.insert(
                                id_str,
                                set_error_value(
                                    &SetError::new(SetErrorType::TooLarge).with_description(
                                        format!(
                                        "script exceeds maxSizeScript limit of {max_bytes} bytes"
                                    ),
                                    ),
                                ),
                            );
                            continue;
                        }
                        Ok(_) => {} // size OK or blob missing (missing blob handled by validate_sieve_script)
                        Err(e) => {
                            not_updated.insert(
                                id_str,
                                json!({ "type": "serverFail", "description": e.to_string() }),
                            );
                            continue;
                        }
                    }
                }

                if let Some(err_desc) = backend
                    .validate_sieve_script(caller, &account_id, &new_blob_id)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?
                {
                    not_updated.insert(
                        id_str,
                        set_error_value(
                            &SetError::new(SetErrorType::custom(SIEVE_ERR_INVALID))
                                .with_description(err_desc),
                        ),
                    );
                    continue;
                }
            }

            match backend
                .update_object::<SieveScript>(caller, &account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    // R9: propagate serialization errors via ? instead of silently swallowing.
                    let obj_json = serde_json::to_value(&obj)
                        .map_err(|e| JmapError::server_fail(e.to_string()))?;
                    updated.insert(id_str, obj_json);
                }
                Ok(None) => {
                    updated.insert(id_str, Value::Null);
                }
                Err(BackendSetError::SetError(se)) => {
                    not_updated.insert(id_str, set_error_value(&se));
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
                .map_err(|e| JmapError::server_fail(e.to_string()))?;
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
                    Err(BackendSetError::SetError(se)) => {
                        not_updated.insert(active_id_str, set_error_value(&se));
                    }
                    Err(BackendSetError::Other(e)) => {
                        not_updated.insert(
                            active_id_str,
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                    }
                    Err(_) => {
                        not_updated.insert(
                            active_id_str,
                            json!({
                                "type": "serverFail",
                                "description": "unhandled backend error variant",
                            }),
                        );
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
                Some(Id::from(activate_ref.as_str()))
            };

            if let Some(ref target_id) = target_id {
                // Deactivate the current active script if it's different from target
                // and hasn't already been deactivated in Step A.
                let (all_scripts, _) = backend
                    .get_objects::<SieveScript>(caller, &account_id, None, None)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;

                // Track whether deactivation failed so we can abort the activate step
                // (Fix R2_3: deactivate failure aborts activate to preserve one-active invariant).
                let mut deactivation_failed = false;

                if let Some(currently_active) = all_scripts.iter().find(|s| s.is_active) {
                    if &currently_active.id != target_id {
                        let active_id = currently_active.id.clone();
                        let active_id_str = active_id.as_ref().to_owned();
                        // Only deactivate if Step A hasn't already done so for this script.
                        if !updated.contains_key(&active_id_str) {
                            let mut patch_map = serde_json::Map::new();
                            patch_map.insert("isActive".to_owned(), Value::Bool(false));
                            let patch = PatchObject::from_map(patch_map);
                            match backend
                                .update_object::<SieveScript>(
                                    caller,
                                    &account_id,
                                    &active_id,
                                    patch,
                                )
                                .await
                            {
                                Ok(_) => {
                                    updated.insert(active_id_str, json!({ "isActive": false }));
                                }
                                Err(BackendSetError::SetError(se)) => {
                                    not_updated.insert(active_id_str, set_error_value(&se));
                                    deactivation_failed = true;
                                }
                                Err(BackendSetError::Other(e)) => {
                                    not_updated.insert(
                                        active_id_str,
                                        json!({ "type": "serverFail", "description": e.to_string() }),
                                    );
                                    deactivation_failed = true;
                                }
                                Err(_) => {
                                    not_updated.insert(
                                        active_id_str,
                                        json!({
                                            "type": "serverFail",
                                            "description": "unhandled backend error variant",
                                        }),
                                    );
                                    deactivation_failed = true;
                                }
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
                        Err(BackendSetError::SetError(se)) => {
                            not_updated.insert(target_id_str, set_error_value(&se));
                        }
                        Err(BackendSetError::Other(e)) => {
                            not_updated.insert(
                                target_id_str,
                                json!({ "type": "serverFail", "description": e.to_string() }),
                            );
                        }
                        Err(_) => {
                            not_updated.insert(
                                target_id_str,
                                json!({
                                    "type": "serverFail",
                                    "description": "unhandled backend error variant",
                                }),
                            );
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
/// `SieveScript` implements `GetObject` but not `QueryObject`, so the backend
/// `query_objects` generic is unavailable. We fetch all scripts and apply
/// filter/sort/pagination in-handler. Accounts typically have very few scripts
/// (O(10)), so a full scan is cheap.
pub async fn handle_sieve_query<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // Step 1: extract and verify account.
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

    // Step 2: parse filter (optional).
    let filter: Option<SieveScriptFilter> = match args.remove("filter") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("filter: {e}")))?,
        ),
    };

    // Step 3: parse pagination arguments.
    let position: i64 = match args.remove("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    let limit: Option<u64> = match args.remove("limit") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("limit: expected a non-negative integer, got {v}"))
        })?),
    };

    let anchor: Option<Id> = match args.remove("anchor") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "anchor: expected an Id string or null, got {v}"
            )))
        }
    };

    let anchor_offset: i64 = match args.remove("anchorOffset") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("anchorOffset: expected an integer, got {v}"))
        })?,
    };

    let calculate_total: bool = args
        .remove("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Step 4: fetch all scripts for the account.
    let (all_scripts, _not_found) = backend
        .get_objects::<SieveScript>(caller, &account_id, None, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // Step 5: apply filter.
    let filtered: Vec<&SieveScript> = all_scripts
        .iter()
        .filter(|s| {
            if let Some(ref f) = filter {
                // name filter: substring match (case-sensitive per spec default).
                if let Some(ref name_needle) = f.name {
                    match &s.name {
                        Some(n) => {
                            if !n.contains(name_needle.as_str()) {
                                return false;
                            }
                        }
                        // Script with no name does not match a name filter.
                        None => return false,
                    }
                }
                // isActive filter: exact boolean match.
                if let Some(active) = f.is_active {
                    if s.is_active != active {
                        return false;
                    }
                }
            }
            true
        })
        .collect();

    // Step 6: sort by name ascending (None sorts before Some, then alphabetic).
    // This is the default and only supported sort order for this handler.
    let mut sorted: Vec<&SieveScript> = filtered;
    sorted.sort_by(|a, b| a.name.as_deref().cmp(&b.name.as_deref()));

    // Build the ordered Id list.
    let all_ids: Vec<&Id> = sorted.iter().map(|s| &s.id).collect();
    let total_count = all_ids.len();

    // Step 7: resolve start position (anchor overrides position).
    let start: usize = if let Some(ref anchor_id) = anchor {
        let anchor_idx = all_ids
            .iter()
            .position(|id| *id == anchor_id)
            .ok_or_else(JmapError::anchor_not_found)?;
        // RFC 8620 §5.5: clamp to [0, len].
        let raw = anchor_idx as i64 + anchor_offset;
        raw.max(0).min(total_count as i64) as usize
    } else if position >= 0 {
        (position as usize).min(total_count)
    } else {
        // Negative position: offset from the end.
        let neg = position.saturating_neg() as usize;
        total_count.saturating_sub(neg)
    };

    // Step 8: apply limit.
    let page_ids: Vec<&Id> = match limit {
        Some(n) => all_ids
            .iter()
            .skip(start)
            .take(n as usize)
            .copied()
            .collect(),
        None => all_ids.iter().skip(start).copied().collect(),
    };

    // Step 9: get query state.
    let query_state = backend
        .get_state::<SieveScript>(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // Step 10: build response.
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": query_state.as_ref(),
        "canCalculateChanges": false,
        "position": start as i64,
        "ids": page_ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });

    // RFC 8620 §5.5: include total only when calculateTotal=true.
    if calculate_total {
        resp["total"] = json!(total_count as u64);
    }

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// SieveScript/validate handler
// ---------------------------------------------------------------------------

/// Handle a `SieveScript/validate` method call (RFC 9661 §2.6).
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are always
/// empty.
pub async fn handle_sieve_validate<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // Step 1: Extract accountId.
    let (account_id, args) = crate::helpers::extract_account_id(args)?;

    // Step 2: Extract blobId.
    let blob_id: Id = match args.get("blobId").and_then(|v| v.as_str()) {
        Some(s) => Id::from(s.to_owned()),
        None => return Err(JmapError::invalid_arguments("blobId is required")),
    };

    // Step 3: Verify account exists (RFC 8620 §3.6.2).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

    // Step 4: Enforce maxSizeScript before delegating to the backend's parser.
    // Mirrors the set-path size-check (sieve.rs:505-529 create, 680-705 update);
    // the trait doc on validate_sieve_script (sieve.rs:68-70) tells implementors
    // size is checked at the handler layer.
    let max_script_bytes: Option<u64> = backend
        .max_sieve_script_bytes(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;
    if let Some(max_bytes) = max_script_bytes {
        match backend.get_sieve_blob(caller, &account_id, &blob_id).await {
            Ok(Some(ref bytes)) if bytes.len() as u64 > max_bytes => {
                let resp = json!({
                    "accountId": account_id.as_ref(),
                    "error": json!({
                        "type": "tooLarge",
                        "description": format!(
                            "script exceeds maxSizeScript limit of {max_bytes} bytes"
                        ),
                    }),
                });
                return Ok((resp, vec![]));
            }
            Ok(_) => {} // size OK or blob missing (missing blob handled by validate_sieve_script)
            Err(e) => return Err(JmapError::server_fail(e.to_string())),
        }
    }

    // Step 5: Delegate validation to the backend.
    let validation_error = backend
        .validate_sieve_script(caller, &account_id, &blob_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // Step 6: Build response — "error" field MUST be present as null when valid
    // (RFC 9661 §2.6).
    let error_value = match validation_error {
        None => Value::Null,
        Some(desc) => json!({ "type": SIEVE_ERR_INVALID, "description": desc }),
    };
    let resp = json!({
        "accountId": account_id.as_ref(),
        "error": error_value,
    });
    Ok((resp, vec![]))
}
