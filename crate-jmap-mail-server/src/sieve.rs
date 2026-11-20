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
use std::collections::{BTreeMap, HashSet};

use crate::MailBackend;
use crate::helpers::{
    SetAccumulators, enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    filter_properties, finalize_set_response, resolve_blob_ref,
};
use jmap_server::{BackendSetError, SetError, SetErrorType};
use jmap_server::{
    HandlerOutput, backend_set_error_value, check_read_only_patch, filter_arg, limit_or_max,
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
    sieve_set(backend, caller, &BTreeMap::new(), args, false).await
}

/// [`handle_sieve_set`]; with `atomic` (an atomic Foo/set,
/// draft-ietf-jmap-conditional-02 §4, from
/// [`jmap_server::CallContext::atomic`]) the script-count (`overQuota`)
/// and name-uniqueness checks are left to the backend's final-state check
/// at commit (§4.2), so a destroy that makes room for a create in the same
/// call succeeds.
///
/// A `#creationId` `blobId` in a create or update resolves against
/// `created_ids` ([`jmap_server::CallContext::created_ids`]; RFC 8620
/// §5.3), so a script uploaded earlier in the request can be stored; one
/// that matches no earlier creation fails `invalidProperties`, as before.
pub(crate) async fn sieve_set<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    atomic: bool,
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
    let activation = take_activation_args(&mut args)?;

    let ctx = SetCtx {
        backend,
        caller,
        account_id: &account_id,
        created_ids,
        atomic,
        vr_script_id: vr_script_id.as_ref(),
        max_script_bytes,
    };
    let mut acc = SetAccumulators::default();
    // Step 4: destroys; Step 5: creates; Step 6: updates.
    set_destroy(&ctx, &mut args, &mut acc).await?;
    let created_scripts = set_create(&ctx, &mut args, &mut acc).await?;
    jmap_server::resolve_set_id_refs(&mut args, created_ids, &acc.created);
    set_update(&ctx, &mut args, &mut acc).await?;

    // Step 7: Activation state machine (draft §2.4).
    //
    // R1: Per spec §2.4, activation side-effects only run if ALL operations succeeded.
    let any_failure =
        !acc.not_created.is_empty() || !acc.not_updated.is_empty() || !acc.not_destroyed.is_empty();
    if !any_failure {
        set_activate(&ctx, &activation, &created_scripts, &mut acc).await?;
    }

    // Step 8: Build response.
    //
    // `mutated` is computed from the result accumulators rather than tracked
    // through the handler body — equivalent in effect, since the only way an
    // entry lands in `created`/`updated`/`destroyed` is on a successful
    // backend mutation. Lets the helper skip the `get_state` round-trip when
    // every operation failed (matches the gating in calendars-server).
    let mutated = !acc.created.is_empty() || !acc.updated.is_empty() || !acc.destroyed.is_empty();
    finalize_set_response::<B, SieveScript>(backend, caller, &account_id, old_state, mutated, acc)
        .await
}

/// What every `SieveScript/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: MailBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
    /// The request-wide `#creationId` map (RFC 8620 §5.3).
    created_ids: &'a BTreeMap<Id, Id>,
    /// An atomic Foo/set (draft-ietf-jmap-conditional-02 §4): count and
    /// name-uniqueness checks are left to the backend's commit.
    atomic: bool,
    /// The VacationResponse-backed script (RFC 9661 §4), if any.
    vr_script_id: Option<&'a Id>,
    /// The `maxSizeScript` limit (RFC 9661 §2.4), if any.
    max_script_bytes: Option<u64>,
}

/// The RFC 9661 §2.4 `onSuccessActivateScript` and
/// `onSuccessDeactivateScript` arguments.
struct ActivationArgs {
    activate: Option<String>,
    deactivate: Option<bool>,
}

/// Remove and check the activation arguments from `args`.
///
/// # Errors
///
/// `invalidArguments` for a value of the wrong type.
fn take_activation_args(
    args: &mut serde_json::Map<String, Value>,
) -> Result<ActivationArgs, JmapError> {
    let activate: Option<String> = match args.remove("onSuccessActivateScript") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "onSuccessActivateScript: expected a string or null, got {}",
                jmap_server::json_type_name(&v)
            )));
        }
    };
    let deactivate: Option<bool> = match args.remove("onSuccessDeactivateScript") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| {
            JmapError::invalid_arguments("onSuccessDeactivateScript: expected a boolean or null")
        })?),
    };
    Ok(ActivationArgs {
        activate,
        deactivate,
    })
}

/// The `destroy` part of `SieveScript/set` (Step 4): the active script
/// and the VacationResponse-backed one cannot be destroyed.
///
/// # Errors
///
/// `invalidArguments` when an element of `destroy` is not a string;
/// `serverFail` when the backend cannot read a script.
async fn set_destroy<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<(), JmapError> {
    let Some(destroy_ids) = args.remove("destroy").and_then(|v| match v {
        Value::Array(a) => Some(a),
        _ => None,
    }) else {
        return Ok(());
    };
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
                acc.not_destroyed
                    .insert(id_str.to_owned(), serde_json::Value::from(&*err));
                continue;
            }
        };

        // Check existence and isActive before destroying.
        let (existing, not_found_ids) = ctx
            .backend
            .get_objects::<SieveScript>(
                ctx.caller,
                ctx.account_id,
                Some(std::slice::from_ref(&id)),
                None,
            )
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        let err = if !not_found_ids.is_empty() || existing.is_empty() {
            SetError::new(SetErrorType::NotFound)
        } else if existing[0].is_active {
            SetError::new(SetErrorType::from(SIEVE_ERR_IS_ACTIVE))
                .with_description("cannot destroy the active Sieve script")
        } else if ctx.vr_script_id == Some(&id) {
            // VR-backed script guard (after sieveIsActive — active VR script takes
            // priority; inactive VR script gets forbidden per draft §4).
            SetError::new(SetErrorType::Forbidden).with_description(
                "this script is managed by VacationResponse/set and cannot be \
                 destroyed via SieveScript/set",
            )
        } else {
            match ctx
                .backend
                .destroy_object::<SieveScript>(ctx.caller, ctx.account_id, &id)
                .await
            {
                Ok(()) => {
                    acc.destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    acc.not_destroyed
                        .insert(id_str.to_owned(), backend_set_error_value(e));
                }
            }
            continue;
        };
        acc.not_destroyed
            .insert(id_str.to_owned(), serde_json::Value::from(&err));
    }
    Ok(())
}

/// The state the `SieveScript/set` create loop carries from one create
/// to the next.
struct CreateRun {
    /// The scripts before any create (R7: one fetch for the whole loop).
    all_scripts_before_create: Vec<SieveScript>,
    /// The per-account script-count cap.
    max_scripts: usize,
    successful_creates: usize,
    /// Names created within this call, to detect intra-call duplicates.
    names_created_this_call: HashSet<String>,
    /// Each successful create's creation id and assigned id, in order.
    created: Vec<(String, Id)>,
}

/// The `create` part of `SieveScript/set` (Step 5). Returns each
/// successful create's creation id and assigned id, in order, for
/// `onSuccessActivateScript`.
///
/// # Errors
///
/// `serverFail` when the backend cannot read the scripts or the cap, or a
/// created script cannot be serialized.
async fn set_create<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<Vec<(String, Id)>, JmapError> {
    // R7: Hoist get_objects call before the loop so we can check overQuota
    // and uniqueness without an extra backend round-trip per create.
    let (all_scripts_before_create, _) = ctx
        .backend
        .get_objects::<SieveScript>(ctx.caller, ctx.account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Per-account script-count cap. Hoisted once for the same reason as
    // get_objects above — the cap is a backend property, not a per-create
    // decision, so a single fetch suffices for the whole loop.
    let max_scripts = ctx
        .backend
        .max_sieve_scripts_per_account(ctx.caller, ctx.account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let mut run = CreateRun {
        all_scripts_before_create,
        max_scripts,
        successful_creates: 0,
        names_created_this_call: HashSet::new(),
        created: Vec::new(),
    };
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (creation_id, obj_val) in create_map {
            create_one(ctx, &mut run, creation_id, &obj_val, acc).await?;
        }
    }
    Ok(run.created)
}

/// One `create` entry of `SieveScript/set`.
async fn create_one<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    run: &mut CreateRun,
    creation_id: String,
    obj_val: &Value,
    acc: &mut SetAccumulators,
) -> Result<(), JmapError> {
    if let Some(err) = obj_val
        .as_object()
        .and_then(reject_server_set::<SieveScript>)
    {
        acc.not_created
            .insert(creation_id, serde_json::Value::from(&err));
        return Ok(());
    }
    // R7: overQuota check (deferred to commit when atomic).
    if !ctx.atomic
        && run.all_scripts_before_create.len() + run.successful_creates >= run.max_scripts
    {
        acc.not_created.insert(
            creation_id,
            serde_json::Value::from(&SetError::new(SetErrorType::OverQuota)),
        );
        return Ok(());
    }

    // a. blob_id is required.
    let blob_id = match create_blob_id(ctx, obj_val) {
        Ok(id) => id,
        Err(err) => {
            acc.not_created.insert(creation_id, err);
            return Ok(());
        }
    };

    // b. Name validation and uniqueness check (if name is provided and non-null).
    let name: Option<String> = obj_val
        .get("name")
        .and_then(|v| if v.is_null() { None } else { v.as_str() })
        .map(|s| s.to_owned());
    if let Some(ref name_str) = name
        && let Some(err) = create_name_error(ctx, run, name_str)
    {
        acc.not_created.insert(creation_id, err);
        return Ok(());
    }

    // c. Fetch the script once; check maxSizeScript, then syntax.
    if let Some(err) = script_error(ctx, &blob_id).await {
        acc.not_created.insert(creation_id, err);
        return Ok(());
    }

    // d. Build SieveScript object — is_active always false on creation.
    let script_name = name.clone();
    let build = move |id: &Id| {
        let mut script = SieveScript::new(id.clone(), blob_id, false);
        script.name = script_name;
        script
    };

    // e. Persist via backend.
    match ctx
        .backend
        .create_object::<SieveScript, _>(ctx.caller, ctx.account_id, &creation_id, build)
        .await
    {
        Ok((assigned_id, created_obj)) => {
            // f. Record creation_id → assigned_id for onSuccessActivateScript.
            run.created.push((creation_id.clone(), assigned_id));

            // Track name for intra-call duplicate detection.
            if let Some(ref n) = name {
                run.names_created_this_call.insert(n.clone());
            }

            // R9: propagate serialization errors via ? instead of silently swallowing.
            let obj_json =
                serde_json::to_value(&created_obj).map_err(|e| server_fail_from_backend(&e))?;
            acc.created.insert(creation_id, obj_json);
            run.successful_creates += 1;
        }
        Err(e) => {
            acc.not_created
                .insert(creation_id, backend_set_error_value(e));
        }
    }
    Ok(())
}

/// A create's `blobId`, required, with a `#creationId` reference
/// resolved, or the SetError value for `notCreated`.
fn create_blob_id<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    obj_val: &Value,
) -> Result<Id, Value> {
    let Some(blob_id_str) = obj_val.get("blobId").and_then(|v| v.as_str()) else {
        return Err(serde_json::Value::from(
            &SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["blobId"])
                .with_description("blobId is required"),
        ));
    };
    jmap_server::parse_wire_id(&resolve_blob_ref(blob_id_str, ctx.created_ids), "blobId")
        .map_err(|err| serde_json::Value::from(&*err))
}

/// Fetch the script in `blob_id` once and check `maxSizeScript`, then
/// syntax. Returns the SetError value, or `None` for a good script.
async fn script_error<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    blob_id: &Id,
) -> Option<Value> {
    match check_script(
        ctx.backend,
        ctx.caller,
        ctx.account_id,
        blob_id,
        ctx.max_script_bytes,
    )
    .await
    {
        Ok(None) => None,
        Ok(Some(err)) => Some(serde_json::Value::from(&err)),
        Err(e) => Some(server_fail_value_from_backend(&e)),
    }
}

/// The SetError value for a create's `name` that is invalid (R2) or
/// already taken (R6, R7), else `None`. Uniqueness is deferred to commit
/// when atomic.
fn create_name_error<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    run: &CreateRun,
    name_str: &str,
) -> Option<Value> {
    // R2: validate name characters.
    if let Some(name_err) = validate_script_name(name_str) {
        return Some(serde_json::Value::from(&name_err));
    }

    // Intra-call duplicate: another create in this same request already
    // succeeded with this name (we don't have the new id yet, so omit
    // existingId — the spec only requires it for DB-level collisions).
    if !ctx.atomic && run.names_created_this_call.contains(name_str) {
        return Some(serde_json::Value::from(
            &SetError::new(SetErrorType::AlreadyExists)
                .with_description("a script with this name was already created in this request"),
        ));
    }

    // R7 + R6: uniqueness check using the pre-fetched all_scripts
    // list (deferred to commit when atomic).
    if !ctx.atomic
        && let Some(existing) = run
            .all_scripts_before_create
            .iter()
            .find(|s| s.name.as_deref() == Some(name_str))
    {
        return Some(serde_json::Value::from(
            &SetError::new(SetErrorType::AlreadyExists).with_existing_id(existing.id.clone()),
        ));
    }
    None
}

/// The `update` part of `SieveScript/set` (Step 6).
///
/// # Errors
///
/// `serverFail` when the backend cannot read the scripts, or an updated
/// script cannot be serialized.
async fn set_update<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<(), JmapError> {
    let Some(Value::Object(update_map)) = args.remove("update") else {
        return Ok(());
    };
    // For name uniqueness checks on update, fetch current scripts once.
    let (all_scripts_for_update, _) = ctx
        .backend
        .get_objects::<SieveScript>(ctx.caller, ctx.account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    for (id_str, patch_val) in update_map {
        update_one(ctx, &all_scripts_for_update, id_str, patch_val, acc).await?;
    }
    Ok(())
}

/// One `update` entry of `SieveScript/set`.
async fn update_one<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    all_scripts_for_update: &[SieveScript],
    id_str: String,
    patch_val: Value,
    acc: &mut SetAccumulators,
) -> Result<(), JmapError> {
    let id = match jmap_server::parse_wire_id(&id_str, "id") {
        Ok(id) => id,
        Err(err) => {
            acc.not_updated
                .insert(id_str, serde_json::Value::from(&*err));
            return Ok(());
        }
    };

    // Convert wire-format Value into a typed PatchObject. RFC 8620
    // §5.3 mandates a PatchObject is a JSON Object; non-object
    // values produce an `invalidPatch` SetError.
    let mut patch = match serde_json::from_value::<PatchObject>(patch_val) {
        Ok(p) => p,
        Err(e) => {
            acc.not_updated.insert(
                id_str,
                json!({ "type": "invalidPatch", "description": e.to_string() }),
            );
            return Ok(());
        }
    };

    // VR-backed script guard: reject blobId changes on the VR-backed script
    // (RFC 9661 §4). isActive changes are handled separately
    // by the activation state machine and are not blocked here.
    // Before the read-only check: permission before shape
    // (DECISIONS.md 44).
    if ctx.vr_script_id == Some(&id) && patch.as_map().contains_key("blobId") {
        acc.not_updated.insert(
            id_str,
            serde_json::Value::from(&SetError::new(SetErrorType::Forbidden).with_description(
                "blobId of a VacationResponse-backed script cannot be \
                                 updated via SieveScript/set",
            )),
        );
        return Ok(());
    }

    // R4: `id` and `isActive` (RFC 9661 §2), which
    // onSuccessActivateScript changes, are server-set: a changed
    // value is rejected, an identical one dropped (RFC 8620 §5.3).
    // Nothing left to apply is a no-op success.
    match check_read_only_patch::<_, SieveScript>(
        ctx.backend,
        ctx.caller,
        ctx.account_id,
        &id,
        patch.as_map_mut(),
    )
    .await
    {
        Ok(false) => {}
        Ok(true) => {
            acc.updated.insert(id_str, Value::Null);
            return Ok(());
        }
        Err(err) => {
            acc.not_updated.insert(id_str, err);
            return Ok(());
        }
    }

    if let Some(err) = update_name_error(ctx, all_scripts_for_update, &id, &patch) {
        acc.not_updated.insert(id_str, err);
        return Ok(());
    }
    if let Some(err) = update_blob_error(ctx, &mut patch).await {
        acc.not_updated.insert(id_str, err);
        return Ok(());
    }

    match ctx
        .backend
        .update_object::<SieveScript>(ctx.caller, ctx.account_id, &id, patch)
        .await
    {
        Ok(Some(obj)) => {
            // R9: propagate serialization errors via ? instead of silently swallowing.
            let obj_json = serde_json::to_value(&obj).map_err(|e| server_fail_from_backend(&e))?;
            acc.updated.insert(id_str, obj_json);
        }
        Ok(None) => {
            acc.updated.insert(id_str, Value::Null);
        }
        Err(e) => {
            acc.not_updated.insert(id_str, backend_set_error_value(e));
        }
    }
    Ok(())
}

/// The SetError value for an update `patch` of script `id` whose `name`
/// is invalid (R2) or taken by another script (R6, deferred to commit
/// when atomic), else `None`.
fn update_name_error<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    all_scripts_for_update: &[SieveScript],
    id: &Id,
    patch: &PatchObject,
) -> Option<Value> {
    let name_str = patch.as_map().get("name")?.as_str()?;
    if let Some(name_err) = validate_script_name(name_str) {
        return Some(serde_json::Value::from(&name_err));
    }
    // R6: uniqueness check — exclude the script being updated.
    if !ctx.atomic
        && let Some(existing) = all_scripts_for_update
            .iter()
            .find(|s| s.name.as_deref() == Some(name_str) && &s.id != id)
    {
        return Some(serde_json::Value::from(
            &SetError::new(SetErrorType::AlreadyExists).with_existing_id(existing.id.clone()),
        ));
    }
    None
}

/// If `patch` sets `blobId`: resolve a `#creationId` reference into the
/// patch, then check the new script's size and syntax. Returns the
/// SetError value for `notUpdated`, or `None`.
async fn update_blob_error<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    patch: &mut PatchObject,
) -> Option<Value> {
    let new_blob_id_str = patch.as_map().get("blobId").and_then(|v| v.as_str())?;
    let new_blob_id = match jmap_server::parse_wire_id(
        &resolve_blob_ref(new_blob_id_str, ctx.created_ids),
        "blobId",
    ) {
        Ok(id) => id,
        Err(err) => return Some(serde_json::Value::from(&*err)),
    };
    // The stored script names the resolved blob, not the reference.
    patch.as_map_mut().insert(
        "blobId".to_owned(),
        Value::String(new_blob_id.as_ref().to_owned()),
    );

    // Fetch the script once; check maxSizeScript, then syntax.
    script_error(ctx, &new_blob_id).await
}

/// The activation part of `SieveScript/set` (Step 7, RFC 9661 §2.4), run
/// only when every create, update and destroy succeeded.
async fn set_activate<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    activation: &ActivationArgs,
    created_scripts: &[(String, Id)],
    acc: &mut SetAccumulators,
) -> Result<(), JmapError> {
    // Step A: onSuccessDeactivateScript
    if activation.deactivate == Some(true) {
        let (all_scripts, _) = ctx
            .backend
            .get_objects::<SieveScript>(ctx.caller, ctx.account_id, None, None)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        if let Some(active_script) = all_scripts.iter().find(|s| s.is_active) {
            let active_id = active_script.id.clone();
            let active_id_str = active_id.as_ref().to_owned();
            match ctx
                .backend
                .update_object::<SieveScript>(
                    ctx.caller,
                    ctx.account_id,
                    &active_id,
                    is_active_patch(false),
                )
                .await
            {
                Ok(_) => {
                    acc.updated
                        .insert(active_id_str, json!({ "isActive": false }));
                }
                Err(e) => {
                    acc.not_updated
                        .insert(active_id_str, backend_set_error_value(e));
                }
            }
        }
    }

    // Step B: onSuccessActivateScript
    if let Some(ref activate_ref) = activation.activate {
        // Resolve the target Id: "#creation_id" or bare Id.
        let target_id: Option<Id> = if let Some(creation_id) = activate_ref.strip_prefix('#') {
            // Look up in the created map; silently ignore if not found (spec §2.4).
            created_scripts
                .iter()
                .rev()
                .find(|(c, _)| c == creation_id)
                .map(|(_, id)| id.clone())
        } else {
            Id::new_validated(activate_ref.as_str()).ok()
        };
        if let Some(ref target_id) = target_id {
            activate_script(ctx, target_id, created_scripts, acc).await?;
        }
    }
    Ok(())
}

/// A one-key PatchObject `{"isActive": active}`.
fn is_active_patch(active: bool) -> PatchObject {
    let mut patch_map = serde_json::Map::new();
    patch_map.insert("isActive".to_owned(), Value::Bool(active));
    PatchObject::from_map(patch_map)
}

/// Set `entry`'s `isActive` to `active`, merging with an existing object.
fn merge_is_active(entry: &mut Value, active: bool) {
    if let Some(obj) = entry.as_object_mut() {
        obj.insert("isActive".to_owned(), json!(active));
    } else {
        *entry = json!({ "isActive": active });
    }
}

/// Step B of the activation: deactivate the script active now, if it is
/// not `target_id`, then activate `target_id`.
async fn activate_script<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    target_id: &Id,
    created_scripts: &[(String, Id)],
    acc: &mut SetAccumulators,
) -> Result<(), JmapError> {
    // Deactivate the current active script if it's different from target
    // and hasn't already been deactivated in Step A.
    let (all_scripts, _) = ctx
        .backend
        .get_objects::<SieveScript>(ctx.caller, ctx.account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // RFC 9661 §2.4: "if the id is either invalid or nonexistent,
    // it MUST be ignored, and the currently active SieveScript (if
    // any) will remain as such".
    if !all_scripts.iter().any(|s| &s.id == target_id) {
        return Ok(());
    }

    // Read after Step A, so a script it deactivated is not active
    // here; one the client otherwise updated in this call still is.
    if let Some(currently_active) = all_scripts.iter().find(|s| s.is_active)
        && &currently_active.id != target_id
    {
        let active_id = currently_active.id.clone();
        let active_id_str = active_id.as_ref().to_owned();
        // A script cannot be in both `updated` and `notUpdated`:
        // when the client's own update of it failed, leave it
        // active and activate nothing (Fix R2_3: deactivate failure
        // aborts activate to preserve the one-active invariant).
        if acc.not_updated.contains_key(&active_id_str) {
            return Ok(());
        }
        match ctx
            .backend
            .update_object::<SieveScript>(
                ctx.caller,
                ctx.account_id,
                &active_id,
                is_active_patch(false),
            )
            .await
        {
            Ok(_) => {
                // Merge with an entry from the client's own update.
                let entry = acc
                    .updated
                    .entry(active_id_str)
                    .or_insert_with(|| json!({}));
                merge_is_active(entry, false);
            }
            Err(e) => {
                acc.not_updated
                    .insert(active_id_str, backend_set_error_value(e));
                // Abort activation to preserve the one-active invariant.
                return Ok(());
            }
        }
    }

    mark_activated(ctx, target_id, created_scripts, acc).await;
    Ok(())
}

/// Activate script `target_id` and report it: in `created` when this
/// call created it, else in `updated`. A `notFound` is ignored.
async fn mark_activated<B: MailBackend + SieveBackend>(
    ctx: &SetCtx<'_, B>,
    target_id: &Id,
    created_scripts: &[(String, Id)],
    acc: &mut SetAccumulators,
) {
    let target_id_str = target_id.as_ref().to_owned();
    match ctx
        .backend
        .update_object::<SieveScript>(ctx.caller, ctx.account_id, target_id, is_active_patch(true))
        .await
    {
        Ok(_) => {
            // R5: If the target was just created in this same set call,
            // reflect isActive:true in the `created` map entry and do NOT
            // add it to `updated` (spec §2.4 example: activated-on-create
            // scripts appear in `created` with isActive:true, not `updated`).
            if let Some((creation_id, _)) =
                created_scripts.iter().rev().find(|(_, id)| id == target_id)
            {
                // Patch the created entry to show isActive:true.
                if let Some(entry) = acc.created.get_mut(creation_id) {
                    merge_is_active(entry, true);
                }
                // Do NOT insert into `updated` — the script is new to the client.
            } else {
                // Existing script: merge with or insert into updated.
                let entry = acc
                    .updated
                    .entry(target_id_str)
                    .or_insert_with(|| json!({}));
                merge_is_active(entry, true);
            }
        }
        // Fix R2_1: nonexistent bare id → spec §2.4 says silently ignore.
        Err(BackendSetError::SetError(ref se)) if se.error_type == SetErrorType::NotFound => {
            // Silently skip — no entry in not_updated.
        }
        Err(e) => {
            acc.not_updated
                .insert(target_id_str, backend_set_error_value(e));
        }
    }
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
    sieve_query_impl(backend, caller, args, None).await
}

/// The body of [`handle_sieve_query`]. With `destroyed`, the query runs over
/// those destroyed scripts (final versions with their destruction times,
/// draft-ietf-jmap-object-history §4) instead of the current ones, and
/// with no `sort` orders them most recently destroyed first.
pub(crate) async fn sieve_query_impl<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    destroyed: Option<Vec<(SieveScript, jmap_types::UTCDate)>>,
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
    let is_destroyed_query = destroyed.is_some();
    let (all_scripts, destroyed_at): (Vec<SieveScript>, Vec<jmap_types::UTCDate>) = match destroyed
    {
        Some(rows) => rows.into_iter().unzip(),
        None => {
            let (all, _not_found) = backend
                .get_objects::<SieveScript>(caller, &account_id, None, None)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;
            (all, Vec::new())
        }
    };
    let when = |s: &SieveScript| {
        all_scripts
            .iter()
            .position(|x| x.id == s.id)
            .and_then(|i| destroyed_at.get(i))
    };

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
    if sort.is_empty() && is_destroyed_query {
        // Object history §4: "If sort is null or empty, the results are
        // ordered by time of destruction, most recent first."
        sorted.sort_by(|a, b| match (when(a), when(b)) {
            (Some(x), Some(y)) => y.cmp_instant(x).unwrap_or(std::cmp::Ordering::Equal),
            _ => std::cmp::Ordering::Equal,
        });
    } else if sort.is_empty() {
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
    let mut query_state = backend
        .get_state::<SieveScript>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    if is_destroyed_query {
        // Its own kind of state (decision 25); there is no
        // SieveScript/queryChanges, and canCalculateChanges is false.
        query_state = jmap_types::State::from(format!("dq|{}", query_state.as_ref()));
    }

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
///
/// A `#creationId` `blobId` needs the request's creation ids, which this
/// function does not take: `register_sieve_handlers` resolves them (RFC
/// 8620 §5.3).
pub async fn handle_sieve_validate<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    sieve_validate(backend, caller, &BTreeMap::new(), args).await
}

/// [`handle_sieve_validate`], resolving a `#creationId` `blobId` against
/// `created_ids` ([`jmap_server::CallContext::created_ids`]; RFC 8620
/// §5.3). A reference that matches no earlier creation is not a valid Id,
/// as before: `invalidArguments`.
pub(crate) async fn sieve_validate<B: MailBackend + SieveBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    // Step 1: Extract accountId.
    let (account_id, args) = crate::helpers::extract_account_id(args)?;

    // Step 2: Extract blobId.
    let blob_id: Id = match args.get("blobId").and_then(|v| v.as_str()) {
        Some(s) => Id::new_validated(resolve_blob_ref(s, created_ids).into_owned())
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
