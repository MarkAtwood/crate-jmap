//! RFC 8621 §8 VacationResponse/get and VacationResponse/set handlers.
//!
//! [`VacationResponse`] is a **singleton**: there is exactly one per account and
//! its `id` is always the string `"singleton"`.  Create and destroy are
//! forbidden; only update of `"singleton"` is permitted.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/set` → §5.3), with the type-specific arguments
//! defined by RFC 8621 §8. The returned `Value` is the corresponding
//! method-response object per the same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures (RFC 8620
//! §3.6.2 and §5):
//!
//! - every handler: `accountNotFound`, `invalidArguments`, `serverFail`;
//! - `/get`, `/set`: `requestTooLarge` past `maxObjectsInGet` /
//!   `maxObjectsInSet`;
//! - `/set`: `stateMismatch`.
//!
//! Per-target failures inside `/set` (singleton-create rejection,
//! singleton-destroy rejection, non-singleton id rejection) surface in the
//! `notCreated` / `notUpdated` / `notDestroyed` maps within
//! `Ok((Value, ...))`, not as `Err`.

use jmap_mail_types::VacationResponse;
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{Value, json};

use crate::MailBackend;
use crate::helpers::{
    SetAccumulators, enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response,
};
use jmap_server::{BackendSetError, SetError, SetErrorType};
use jmap_server::{HandlerOutput, server_fail_from_backend, strip_read_only_patch};

const SINGLETON_ID: &str = "singleton";

// ---------------------------------------------------------------------------
// VacationResponse/get
// ---------------------------------------------------------------------------

/// Handle a `VacationResponse/get` request (RFC 8621 §8.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Accepts `ids = null` or `ids = ["singleton"]` — both return the singleton
/// (if it exists). `ids = []` returns an empty list immediately.  Any id
/// other than `"singleton"` is placed in `notFound`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id, or
///   `ids` is not a string array;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `requestTooLarge` when `ids` holds more than `maxObjectsInGet` ids;
/// - `serverFail` when the backend fails.
pub async fn handle_vacation_get<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    enforce_max_objects_in_get(&args, backend.max_objects_in_get(caller, &account_id))?;

    let requested_ids: Option<Vec<String>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be a string array"))?,
        ),
    };

    let state = backend
        .get_state::<VacationResponse>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // ids=[] — return empty immediately.
    if let Some(ref ids) = requested_ids
        && ids.is_empty()
    {
        return Ok(HandlerOutput::new(json!({
            "accountId": account_id.as_ref(),
            "state": state.as_ref(),
            "list": [],
            "notFound": [],
        })));
    }

    // Any requested id that is not "singleton" is notFound.
    let not_found: Vec<Value> = requested_ids
        .iter()
        .flatten()
        .filter(|id| id.as_str() != SINGLETON_ID)
        .map(|id| Value::String(id.clone()))
        .collect();

    // Fetch the singleton from the backend.
    // Id::from: SINGLETON_ID is a constant RFC 8620 §1.2 Id, not wire input.
    let singleton_id = Id::from(SINGLETON_ID);
    let (list, _) = backend
        .get_objects::<VacationResponse>(caller, &account_id, Some(&[singleton_id]), None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // RFC 8621 §8: "There MUST only be exactly one VacationResponse object
    // in an account". Until the first update stores it, the singleton is
    // the default: disabled, every other property null.
    let list = if list.is_empty() {
        vec![VacationResponse::new(Id::from(SINGLETON_ID), false)]
    } else {
        list
    };
    let list_json: Vec<Value> = list
        .iter()
        .map(jmap_server::serialize_value)
        .collect::<Result<_, _>>()?;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": Value::Array(not_found),
    })))
}

// ---------------------------------------------------------------------------
// VacationResponse/set
// ---------------------------------------------------------------------------

/// Handle a `VacationResponse/set` request (RFC 8621 §8.2).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Rules enforced here (not in the backend):
/// - `create` is always rejected with `SetErrorType::Singleton`.
/// - `destroy` is always rejected with `SetErrorType::Singleton`.
/// - `update "singleton"` is the only permitted mutation.  If no
///   [`VacationResponse`] exists yet the handler creates it (upsert semantics).
/// - Any update id other than `"singleton"` is rejected with `NotFound`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id, or
///   when `create`, `update` or `destroy` is malformed;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `requestTooLarge` when `create`, `update` and `destroy` together hold
///   more than `maxObjectsInSet` objects;
/// - `stateMismatch` when `ifInState` is not the current state;
/// - `serverFail` when the backend fails.
///
/// A create, update or destroy that fails on its own is a [`SetError`] in
/// the response, not an `Err`.
pub async fn handle_vacation_set<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.2). Reject
    // unbounded /set batches before touching the storage layer.
    // VacationResponse is a singleton, so the cap is moot in practice
    // (typical /set is 1 create + 1 update); applied for canonical-
    // template uniformity across the 6 mail-server /set handlers.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    // ifInState check.
    let old_state = backend
        .get_state::<VacationResponse>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && old_state.as_ref() != if_in_state
    {
        return Err(JmapError::state_mismatch());
    }

    let ctx = SetCtx {
        backend,
        caller,
        account_id: &account_id,
    };
    let mut acc = SetAccumulators::default();
    set_create(&args, &mut acc);
    let mutated = set_update(&ctx, &mut args, &mut acc).await?;
    set_destroy(&args, &mut acc)?;

    // VacationResponse is a singleton: created/destroyed are always empty by
    // construction (see the rejections in `set_create` and `set_destroy`),
    // so the helper's empty-map → Value::Null conversion emits `null` for
    // both.
    finalize_set_response::<B, VacationResponse>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        acc,
    )
    .await
}

/// What every `VacationResponse/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: MailBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
}

/// The `create` part of `VacationResponse/set`: every create is
/// rejected with `singleton`.
fn set_create(args: &serde_json::Map<String, Value>, acc: &mut SetAccumulators) {
    if let Some(create) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, _) in create {
            let err = SetError::new(SetErrorType::Singleton)
                .with_description("VacationResponse is a singleton; use update to modify");
            acc.not_created
                .insert(create_id.clone(), serde_json::Value::from(&err));
        }
    }
}

/// The `update` part of `VacationResponse/set`: only `"singleton"` is a
/// valid id. Returns whether the backend was written.
async fn set_update<B: MailBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Object(update)) = args.remove("update") {
        for (id, patch_val) in update {
            mutated |= update_one(ctx, id, patch_val, acc).await?;
        }
    }
    Ok(mutated)
}

/// One `update` entry of `VacationResponse/set`. Returns whether the
/// backend was written.
async fn update_one<B: MailBackend>(
    ctx: &SetCtx<'_, B>,
    id: String,
    patch_val: Value,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    if id != SINGLETON_ID {
        let err = SetError::new(SetErrorType::NotFound);
        acc.not_updated.insert(id, serde_json::Value::from(&err));
        return Ok(false);
    }

    // Convert wire-format Value into a typed PatchObject. RFC 8620
    // §5.3 mandates a PatchObject is a JSON Object; non-object
    // values produce an `invalidPatch` SetError.
    let mut patch = match serde_json::from_value::<PatchObject>(patch_val) {
        Ok(p) => p,
        Err(e) => {
            acc.not_updated.insert(
                id,
                json!({ "type": "invalidPatch", "description": e.to_string() }),
            );
            return Ok(false);
        }
    };

    // The stored `id` is always "singleton" (RFC 8621 §8), so a
    // whole-object patch may repeat it (RFC 8620 §5.3); it is
    // dropped, and nothing left is a no-op success.
    let stored = serde_json::Map::from_iter([("id".to_owned(), json!(SINGLETON_ID))]);
    if let Err(err) = strip_read_only_patch::<VacationResponse>(patch.as_map_mut(), Some(&stored)) {
        acc.not_updated.insert(id, serde_json::Value::from(&*err));
        return Ok(false);
    }
    if patch.is_empty() {
        acc.updated.insert(id, Value::Null);
        return Ok(false);
    }

    let singleton_id = Id::from(SINGLETON_ID);
    match ctx
        .backend
        .update_object::<VacationResponse>(ctx.caller, ctx.account_id, &singleton_id, patch.clone())
        .await
    {
        Ok(Some(obj)) => {
            acc.updated.insert(id, jmap_server::serialize_value(&obj)?);
            Ok(true)
        }
        Ok(None) => {
            acc.updated.insert(id, Value::Null);
            Ok(true)
        }
        Err(BackendSetError::SetError(ref set_err))
            if set_err.error_type == SetErrorType::NotFound =>
        {
            upsert_singleton(ctx, id, patch, acc).await
        }
        Err(e) => {
            let e = e.into_set_error_or_method_error()?;
            acc.not_updated.insert(id, serde_json::Value::from(&e));
            Ok(false)
        }
    }
}

/// The singleton does not exist yet: store the default, then apply
/// `patch` to it (upsert). Returns whether the backend was written.
///
/// Concurrency note: two concurrent requests can both reach this function
/// and attempt to create the singleton. Backends that receive concurrent
/// requests MUST make `create_object` idempotent for the singleton key (e.g.
/// via a unique constraint or a compare-and-swap) to avoid duplicate
/// creation. The handler layer cannot add locking here because it holds no
/// shared state.
async fn upsert_singleton<B: MailBackend>(
    ctx: &SetCtx<'_, B>,
    id: String,
    patch: PatchObject,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    // The patch is checked against the default singleton first, so an
    // invalid patch stores nothing.
    if let Err(err) = patched_default(&patch) {
        acc.not_updated.insert(id, err);
        return Ok(false);
    }
    // RFC 8621 §8.1: the singleton always exists; until now it was the
    // default /get reports. Store that default, then update it, so object
    // history keeps it as the version this update replaced. If the update
    // then fails, the stored default is what /get showed before, so
    // nothing is rolled back (a destroy would break the "exactly one"
    // rule).
    if let Err(e) = ctx
        .backend
        .create_object::<VacationResponse, _>(
            ctx.caller,
            ctx.account_id,
            SINGLETON_ID,
            |id: &Id| VacationResponse::new(id.clone(), false),
        )
        .await
    {
        let e = e.into_set_error_or_method_error()?;
        acc.not_updated.insert(id, serde_json::Value::from(&e));
        return Ok(false);
    }
    let singleton_id = Id::from(SINGLETON_ID);
    match ctx
        .backend
        .update_object::<VacationResponse>(ctx.caller, ctx.account_id, &singleton_id, patch)
        .await
    {
        Ok(Some(obj)) => {
            acc.updated.insert(id, jmap_server::serialize_value(&obj)?);
        }
        Ok(None) => {
            acc.updated.insert(id, Value::Null);
        }
        Err(be) => {
            let e = be.into_set_error_or_method_error()?;
            acc.not_updated.insert(id, serde_json::Value::from(&e));
        }
    }
    Ok(true)
}

/// The `destroy` part of `VacationResponse/set`: every destroy is
/// rejected with `singleton`.
///
/// # Errors
///
/// `invalidArguments` when an element of `destroy` is not a string.
fn set_destroy(
    args: &serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<(), JmapError> {
    if let Some(destroy) = args.get("destroy").and_then(|v| v.as_array()) {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        let id_strs: Vec<&str> = destroy
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

        for id in id_strs {
            let err = SetError::new(SetErrorType::Singleton)
                .with_description("VacationResponse is a singleton; cannot destroy");
            acc.not_destroyed
                .insert(id.to_owned(), serde_json::Value::from(&err));
        }
    }
    Ok(())
}

/// The default [`VacationResponse`] (RFC 8621 §8: disabled, every other
/// property null) with `patch` applied, or the [`SetError`] for the update
/// (checked before the default is stored):
/// `invalidPatch` for a bad path, `invalidProperties` for a value of the
/// wrong type.
fn patched_default(patch: &PatchObject) -> Result<VacationResponse, Value> {
    let default = VacationResponse::new(Id::from(SINGLETON_ID), false);
    let Ok(Value::Object(mut map)) = serde_json::to_value(&default) else {
        return Err(json!({ "type": "serverFail" }));
    };
    if let Err(e) = jmap_server::apply_patch_object(&mut map, patch.as_map()) {
        return Err(json!({ "type": "invalidPatch", "description": e.to_string() }));
    }
    serde_json::from_value(Value::Object(map)).map_err(|_| {
        let properties: Vec<&str> = patch
            .as_map()
            .keys()
            .map(|k| k.split('/').next().unwrap_or(k))
            .collect();
        json!({ "type": "invalidProperties", "properties": properties })
    })
}
