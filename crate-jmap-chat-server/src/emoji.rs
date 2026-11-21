//! `CustomEmoji`/* method handlers (draft-atwood-jmap-chat-00 §`CustomEmoji`).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-atwood-jmap-chat-00 §`CustomEmoji`. The
//! returned `Value` is the corresponding method-response object per the
//! same section refs.
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
//! - `/set`: `stateMismatch`;
//! - `/changes`, `/queryChanges`: `cannotCalculateChanges`;
//! - `/queryChanges`: `tooManyChanges`;
//! - `/query`, `/queryChanges`: `unsupportedFilter`;
//! - `/query`: `unsupportedSort`, `anchorNotFound`.
//!
//! Per-target failures inside `/set` (including the backend-canonical
//! `may_set_custom_emoji` rich-SetError deny path) surface in the `notCreated`
//! / `notUpdated` / `notDestroyed` maps within `Ok((Value, ...))`, not as
//! `Err`.

use std::collections::BTreeMap;

use jmap_chat_types::CustomEmoji;
use jmap_chat_types::{CustomEmojiComparator, CustomEmojiFilter};
use jmap_types::{Id, JmapError, PatchObject, State, UTCDate};
use serde_json::{Value, json};

use crate::helpers::check_read_only_patch;
use crate::helpers::resolved_create;
use crate::helpers::{PropType, mistyped_create_props};
use crate::helpers::{
    SetAccumulators, default_sort, enforce_max_objects_in_get, enforce_max_objects_in_set,
    extract_account_id, finalize_set_response, not_found_json, now_utc_string, optional_arg,
    reject_unsupported_sort, serialize_value,
};
use crate::{ChatBackend, ChatLimits, EmojiSetOp};
use jmap_server::{
    HandlerOutput, QueryParams, backend_set_error_value, bool_arg, filter_arg, is_client_updatable,
    query_anchor_args, reject_server_set, server_fail_from_backend, server_fail_value_from_backend,
};

// ---------------------------------------------------------------------------
// CustomEmoji/get
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/get` method call (draft-atwood-jmap-chat-00 §`CustomEmoji`).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id, or
///   `ids` is not an Id array;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `requestTooLarge` when `ids` holds more than `maxObjectsInGet` ids;
/// - `serverFail` when the backend fails.
pub async fn handle_emoji_get<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-dtpe).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    enforce_max_objects_in_get(&args, backend.max_objects_in_get(caller, &account_id))?;

    let ids: Option<Vec<Id>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<CustomEmoji>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<CustomEmoji>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(serialize_value)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    })))
}

// ---------------------------------------------------------------------------
// CustomEmoji/changes
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/changes` method call (draft-atwood-jmap-chat-00 §`CustomEmoji`).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id,
///   `sinceState` is missing, or `maxChanges` is not a positive integer;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `cannotCalculateChanges` when the backend cannot compute the changes
///   since `sinceState`;
/// - `serverFail` when the backend fails.
pub async fn handle_emoji_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-dtpe).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let since_state: State = match args.get("sinceState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceState is required")),
    };

    let max_changes = jmap_server::parse_max_changes(args.get("maxChanges"))?;

    let result = backend
        .get_changes::<CustomEmoji>(
            caller,
            &account_id,
            jmap_server::ChangesParams::new(&since_state).max_changes(max_changes),
        )
        .await
        .map_err(jmap_server::changes_error_from_backend)?;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "updatedProperties": Value::Null,
        "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    })))
}

// ---------------------------------------------------------------------------
// CustomEmoji/query
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/query` method call (draft-atwood-jmap-chat-00 §`CustomEmoji`).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Unknown filter clauses and sort properties are refused with
/// `unsupportedFilter` / `unsupportedSort` (RFC 8620 §5.5).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id, or
///   when `filter`, `sort`, `position`, `anchor`, `anchorOffset` or `limit`
///   is malformed;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `unsupportedFilter` when the filter does not decode as a `CustomEmoji` filter;
/// - `unsupportedSort` when `sort` names a property this server does not
///   define;
/// - `anchorNotFound` when `anchor` is not in the results;
/// - `serverFail` when the backend fails, or any other RFC 8620 §5.5 error
///   the backend returns.
pub async fn handle_emoji_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-dtpe).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let calculate_total: bool = bool_arg(&args, "calculateTotal", false);

    let limit: Option<u64> = match args.remove("limit") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {}",
                    jmap_server::json_type_name(&v)
                )));
            }
        },
    };

    let position: i64 = match args.remove("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "position: expected an integer, got {}",
                jmap_server::json_type_name(&v)
            ))
        })?,
    };

    let (anchor, anchor_offset) = query_anchor_args(&args)?;

    // §CustomEmoji/query: "Default sort: `name` ascending."
    default_sort(&mut args, json!([{ "property": "name" }]));
    reject_unsupported_sort::<CustomEmojiComparator>(args.get("sort"))?;
    let filter: Option<CustomEmojiFilter> = filter_arg(&mut args)?;

    let sort: Option<Vec<CustomEmojiComparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;

    let result = backend
        .query_objects::<CustomEmoji>(
            caller,
            &account_id,
            QueryParams::new()
                .filter(filter.as_ref())
                .sort(sort.as_deref())
                .limit(limit)
                .position(position)
                .anchor(anchor.as_ref())
                .anchor_offset(anchor_offset),
        )
        .await
        .map_err(JmapError::from)?;

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": result.query_state.as_ref(),
        "canCalculateChanges": result.can_calculate_changes,
        "position": result.position,
        "ids": result.ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total && let Some(t) = result.total {
        resp["total"] = json!(t);
    }

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// CustomEmoji/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/queryChanges` method call (draft-atwood-jmap-chat-00 §`CustomEmoji`).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id,
///   `sinceQueryState` is missing, or `filter`, `sort`, `maxChanges` or
///   `upToId` is malformed;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `unsupportedFilter` when the filter does not decode as a `CustomEmoji` filter;
/// - `unsupportedSort` when `sort` names a property this server does not
///   define;
/// - `cannotCalculateChanges` when the backend cannot compute the changes
///   since `sinceQueryState`;
/// - `serverFail` when the backend fails, or any other RFC 8620 §5.6 error
///   the backend returns.
pub async fn handle_emoji_query_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
) -> Result<HandlerOutput, JmapError> {
    // RFC 8620 §5.6: the `filter` and `sort` are those of the original
    // query, so the §CustomEmoji/query default sort applies here too.
    if let Value::Object(map) = &mut args {
        default_sort(map, json!([{ "property": "name" }]));
    }
    reject_unsupported_sort::<CustomEmojiComparator>(args.get("sort"))?;
    jmap_server::handle_query_changes::<CustomEmoji, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// CustomEmoji/set
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/set` method call (draft-atwood-jmap-chat-00 §`CustomEmoji`).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Validation enforced here (not in the backend):
/// - `name` is required on create and must match `/^[a-z0-9_-]+$/`.
/// - `blobId` is required on create.
/// - `id`, `createdBy`, `createdAt`, `spaceId` are server-set or immutable and
///   rejected in updates unless identical to the stored value (RFC 8620 §5.3); an empty patch is a
///   no-op success.
/// - The [`ChatLimits`] emoji caps, as a pre-check after the
///   authorization gate (jmap-chat-implementer-guide §2.2: "The server
///   MUST reject `CustomEmoji/set` with `overQuota` (per RFC 8620 §5.3)
///   when the count or size limit is exceeded"). A create that would put
///   more than `max_custom_emoji_per_space` emoji in its Space, or a
///   create or `blobId` update whose blob
///   ([`ChatResolvers::attachment_blob`](crate::ChatResolvers::attachment_blob)) is larger than
///   `max_custom_emoji_bytes`, is `overQuota`. The count pre-check
///   discounts emoji this call destroys, so it never refuses a create
///   that an `atomic` call's final state allows; the backend enforces
///   both caps canonically.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// A `spaceId` that is a `#creationId` reference names a record created earlier in the request and resolves against
/// `created_ids` (pass [`jmap_server::CallContext::created_ids`]; RFC 8620
/// §5.3); an unknown reference is `invalidProperties` naming the property.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id, or
///   an element of `destroy` is not a string;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `requestTooLarge` when `create`, `update` and `destroy` together hold
///   more than `maxObjectsInSet` objects;
/// - `stateMismatch` when `ifInState` is not the current state;
/// - `serverFail` when the backend fails.
///
/// A create, update or destroy that fails on its own is a
/// [`SetError`](jmap_server::SetError) in the response, not an `Err`.
pub async fn handle_emoji_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    mut args: Value,
) -> Result<HandlerOutput, JmapError> {
    // A `#creationId` blob reference names a blob created earlier in the
    // request (RFC 8620 §5.3; draft-ietf-jmap-blobext-03 §8); an unknown one
    // is left for the Id checks below.
    jmap_server::resolve_blob_refs(&mut args, &["blobId"], created_ids);
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-fo38).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<CustomEmoji>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    let limits = backend.limits(caller, &account_id);
    let ctx = SetCtx {
        backend,
        caller,
        account_id: &account_id,
        created_ids,
        limits: &limits,
    };
    let mut acc = SetAccumulators::default();
    let mut mutated = set_create(&ctx, &mut args, &mut acc).await?;
    // RFC 8620 §5.3: update keys and destroy ids may name this call's creates.
    jmap_server::resolve_set_id_refs(&mut args, created_ids, &acc.created);
    mutated |= set_update(&ctx, &mut args, &mut acc).await?;
    mutated |= set_destroy(&ctx, &mut args, &mut acc).await?;

    finalize_set_response::<B, CustomEmoji>(backend, caller, &account_id, old_state, mutated, acc)
        .await
}

/// What every `CustomEmoji/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: ChatBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
    /// The request-wide `#creationId` map (RFC 8620 §5.3).
    created_ids: &'a BTreeMap<Id, Id>,
    /// The account's emoji caps.
    limits: &'a ChatLimits,
}

/// The `create` part of `CustomEmoji/set`. Returns whether the backend was
/// written.
async fn set_create<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        let mut space_counts =
            emoji_space_counts(ctx.backend, ctx.caller, ctx.account_id, ctx.limits, args)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;
        for (create_id, obj_val) in create_map {
            mutated |= create_one(ctx, create_id, obj_val, acc, &mut space_counts).await?;
        }
    }
    Ok(mutated)
}

/// One `create` entry of `CustomEmoji/set`. `space_counts` is the running
/// per-Space emoji count for the `max_custom_emoji_per_space` pre-check.
/// Returns whether the backend was written.
async fn create_one<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    create_id: &str,
    obj_val: &Value,
    acc: &mut SetAccumulators,
    space_counts: &mut BTreeMap<Id, u64>,
) -> Result<bool, JmapError> {
    let resolved = match resolved_create(obj_val, &["spaceId"], ctx.created_ids) {
        Ok(v) => v,
        Err(err) => {
            acc.not_created
                .insert(create_id.to_owned(), serde_json::Value::from(&*err));
            return Ok(false);
        }
    };
    let (name, blob_id, space_id) = match create_fields(&resolved) {
        Ok(fields) => fields,
        Err(err) => {
            acc.not_created.insert(create_id.to_owned(), err);
            return Ok(false);
        }
    };

    let now_str = now_utc_string();
    let now: UTCDate = UTCDate::from(now_str.as_ref());

    // Authorization gate (draft-atwood-jmap-chat-00 commit
    // `9344aec`). Runs AFTER wire-format validation so
    // malformed creates don't consume an authorization
    // decision, and BEFORE `create_object` so an unauthorized
    // emoji never touches storage. The target scope is
    // whatever spaceId the create payload supplies (None for
    // server-global).
    if let Some(err) = emoji_denied(ctx, space_id.as_ref(), EmojiSetOp::Create).await {
        acc.not_created.insert(create_id.to_owned(), err);
        return Ok(false);
    }

    if let (Some(space), Some(max)) = (&space_id, ctx.limits.max_custom_emoji_per_space)
        && space_counts.get(space).copied().unwrap_or(0) >= u64::from(max)
    {
        acc.not_created
            .insert(create_id.to_owned(), over_quota(TOO_MANY_EMOJI));
        return Ok(false);
    }
    if let Some(err) = emoji_blob_too_large(
        ctx.backend,
        ctx.caller,
        ctx.account_id,
        ctx.limits,
        &blob_id,
    )
    .await
    {
        acc.not_created.insert(create_id.to_owned(), err);
        return Ok(false);
    }

    // createdBy is the owner, referenced by the accountId
    // (draft-atwood-jmap-chat-00 §Terminology).
    let emoji = |id: &Id| {
        let mut emoji = CustomEmoji::new(id.clone(), name, blob_id, ctx.account_id.clone(), now);
        emoji.space_id = space_id;
        emoji
    };

    match ctx
        .backend
        .create_object::<CustomEmoji, _>(ctx.caller, ctx.account_id, create_id, emoji)
        .await
    {
        Ok((_server_id, created_obj)) => {
            if let Some(space) = &created_obj.space_id {
                *space_counts.entry(space.clone()).or_default() += 1;
            }
            acc.created.insert(
                create_id.to_owned(),
                jmap_server::serialize_value(&created_obj)?,
            );
            Ok(true)
        }
        Err(e) => {
            acc.not_created
                .insert(create_id.to_owned(), backend_set_error_value(e));
            Ok(false)
        }
    }
}

/// Check the server-set and client-supplied properties of one
/// `CustomEmoji/set` create object (with `#creationId` references
/// resolved) and return its `name`, `blobId` and `spaceId`, or the
/// `SetError` value for `notCreated`.
fn create_fields(obj_val: &Value) -> Result<(String, Id, Option<Id>), Value> {
    if let Some(err) = obj_val
        .as_object()
        .and_then(reject_server_set::<CustomEmoji>)
    {
        return Err(serde_json::Value::from(&err));
    }
    // Optional properties: a wrongly-typed value is invalidProperties
    // (RFC 8620 §5.3), not a silent absence.
    if let Some(err) = mistyped_create_props(obj_val, &[("spaceId", PropType::Str)]) {
        return Err(err);
    }
    // Validate name: required, non-empty, matches /^[a-z0-9_-]+$/
    let bad_name = || json!({ "type": "invalidProperties", "properties": ["name"] });
    let name = match obj_val.get("name").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_owned(),
        _ => return Err(bad_name()),
    };
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(bad_name());
    }
    if name.len() > 64 {
        return Err(bad_name());
    }

    // blobId is required
    let blob_id = match obj_val
        .get("blobId")
        .and_then(|v| v.as_str())
        .and_then(|s| Id::new_validated(s).ok())
    {
        Some(id) => id,
        _ => return Err(json!({ "type": "invalidProperties", "properties": ["blobId"] })),
    };

    // spaceId is optional
    let space_id: Option<Id> = match obj_val.get("spaceId").and_then(|v| v.as_str()) {
        Some(s) => Some(
            jmap_server::parse_wire_id(s, "spaceId")
                .map_err(|err| serde_json::Value::from(&*err))?,
        ),
        None => None,
    };
    Ok((name, blob_id, space_id))
}

/// The `SetError` value when [`SpacePolicy::may_set_custom_emoji`](crate::SpacePolicy::may_set_custom_emoji)
/// denies `op` on an emoji in `space_id` (`None` for server-global), or
/// `None` to go on.
async fn emoji_denied<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    space_id: Option<&Id>,
    op: EmojiSetOp,
) -> Option<Value> {
    match ctx
        .backend
        .may_set_custom_emoji(ctx.caller, ctx.account_id, space_id, op)
        .await
    {
        Ok(Ok(())) => None,
        Ok(Err(set_err)) => Some(serde_json::Value::from(&set_err)),
        Err(e) => Some(server_fail_value_from_backend(&e)),
    }
}

/// [`emoji_denied`] for the Space of the stored emoji `id`. An emoji that
/// does not resolve is not gated, so the backend surfaces `notFound`; a
/// pre-fetch storage error is `serverFail`.
async fn stored_emoji_denied<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    id: &Id,
    op: EmojiSetOp,
) -> Option<Value> {
    match ctx
        .backend
        .get_objects::<CustomEmoji>(
            ctx.caller,
            ctx.account_id,
            Some(std::slice::from_ref(id)),
            None,
        )
        .await
    {
        Ok((found, _not_found)) => match found.first() {
            Some(emoji) => emoji_denied(ctx, emoji.space_id.as_ref(), op).await,
            None => None,
        },
        Err(e) => Some(server_fail_value_from_backend(&e)),
    }
}

/// The `update` part of `CustomEmoji/set`. Returns whether the backend was
/// written.
async fn set_update<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            mutated |= update_one(ctx, id_str, patch_val, acc).await?;
        }
    }
    Ok(mutated)
}

/// One `update` entry of `CustomEmoji/set`. Returns whether the backend was
/// written.
async fn update_one<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    id_str: String,
    mut patch_val: Value,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let id = match jmap_server::parse_wire_id(&id_str, "id") {
        Ok(id) => id,
        Err(err) => {
            acc.not_updated
                .insert(id_str, serde_json::Value::from(&*err));
            return Ok(false);
        }
    };

    // Authorization gate (draft-atwood-jmap-chat-00 commit
    // `9344aec`). Pre-fetch the existing emoji to learn its
    // `spaceId`. If the pre-fetch reports the id as not
    // found, skip the gate entirely — `update_object` will
    // surface `notFound` and we don't want to consume an
    // authorization decision for a non-existent target. A
    // pre-fetch storage error is surfaced as `serverFail`.
    // The gate runs before the read-only and no-op checks, so a
    // caller who may not update the emoji gets `forbidden` even
    // for `{}`.
    if let Some(err) = stored_emoji_denied(ctx, &id, EmojiSetOp::Update).await {
        acc.not_updated.insert(id_str, err);
        return Ok(false);
    }

    // §CustomEmoji: `id`, `createdBy` and `createdAt` are
    // server-set; `spaceId` is set only on create.
    match check_read_only_patch::<_, CustomEmoji>(
        ctx.backend,
        ctx.caller,
        ctx.account_id,
        &id,
        &mut patch_val,
    )
    .await
    {
        Ok(false) => {}
        Ok(true) => {
            acc.updated.insert(id_str, Value::Null);
            return Ok(false);
        }
        Err(err) => {
            acc.not_updated.insert(id_str, err);
            return Ok(false);
        }
    }

    let clean_patch = match clean_patch(patch_val) {
        Ok(patch) => patch,
        Err(err) => {
            acc.not_updated.insert(id_str, err);
            return Ok(false);
        }
    };

    if let Some(err) = update_blob_error(ctx, &clean_patch).await {
        acc.not_updated.insert(id_str, err);
        return Ok(false);
    }

    match ctx
        .backend
        .update_object::<CustomEmoji>(
            ctx.caller,
            ctx.account_id,
            &id,
            PatchObject::from_map(clean_patch),
        )
        .await
    {
        Ok(Some(obj)) => {
            acc.updated
                .insert(id_str, jmap_server::serialize_value(&obj)?);
            Ok(true)
        }
        Ok(None) => {
            acc.updated.insert(id_str, Value::Null);
            Ok(true)
        }
        Err(e) => {
            acc.not_updated.insert(id_str, backend_set_error_value(e));
            Ok(false)
        }
    }
}

/// The [`emoji_blob_too_large`] `SetError` value for the `blobId` that the
/// cleaned update patch `clean_patch` sets, or `None`. A malformed `blobId`
/// is left to the backend's patch validation.
async fn update_blob_error<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    clean_patch: &serde_json::Map<String, Value>,
) -> Option<Value> {
    let blob_id = clean_patch
        .get("blobId")
        .and_then(Value::as_str)
        .and_then(|s| Id::new_validated(s).ok())?;
    emoji_blob_too_large(
        ctx.backend,
        ctx.caller,
        ctx.account_id,
        ctx.limits,
        &blob_id,
    )
    .await
}

/// Project one `CustomEmoji/set` update patch onto the client-updatable
/// properties, or return the `invalidPatch` `SetError` value.
///
/// §CustomEmoji/set: "`update` supports: `name`, `blobId`", the
/// client-settable properties that are not immutable.
fn clean_patch(patch_val: Value) -> Result<serde_json::Map<String, Value>, Value> {
    let Value::Object(mut patch_map) = patch_val else {
        return Err(
            json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
        );
    };
    patch_map.retain(|field, _| is_client_updatable::<CustomEmoji>(field));
    if patch_map.is_empty() {
        return Err(
            json!({ "type": "invalidPatch", "description": "no updatable fields in patch" }),
        );
    }
    Ok(patch_map)
}

/// The `destroy` part of `CustomEmoji/set`. Returns whether the backend
/// was written.
async fn set_destroy<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(destroy_arr) = args.get("destroy").and_then(|v| v.as_array()) {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        let id_strs: Vec<&str> = destroy_arr
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

            // Authorization gate (draft-atwood-jmap-chat-00 commit
            // `9344aec`). Pre-fetch the existing emoji to learn its
            // `spaceId`. If pre-fetch reports the id as not found,
            // skip the gate so `destroy_object` can surface
            // `notFound` naturally. A pre-fetch storage error is
            // surfaced as `serverFail`.
            if let Some(err) = stored_emoji_denied(ctx, &id, EmojiSetOp::Destroy).await {
                acc.not_destroyed.insert(id_str.to_owned(), err);
                continue;
            }

            match ctx
                .backend
                .destroy_object::<CustomEmoji>(ctx.caller, ctx.account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    acc.destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    acc.not_destroyed
                        .insert(id_str.to_owned(), backend_set_error_value(e));
                }
            }
        }
    }

    Ok(mutated)
}

/// `overQuota` description for [`ChatLimits::max_custom_emoji_per_space`].
/// Static, without the count or the cap, as for the Space caps
/// (bd:JMAP-x2gd.107). Byte-identical to the memory backend's.
pub(crate) const TOO_MANY_EMOJI: &str = "too many custom emoji in this Space";

/// `overQuota` description for [`ChatLimits::max_custom_emoji_bytes`].
/// Byte-identical to the memory backend's.
pub(crate) const EMOJI_TOO_LARGE: &str = "custom emoji image too large";

/// An `overQuota` `SetError` (RFC 8620 §5.3) with `description`.
fn over_quota(description: &str) -> Value {
    json!({ "type": "overQuota", "description": description })
}

/// The stored [`CustomEmoji`] count per Space, less the emoji this call's
/// `destroy` names, for the [`ChatLimits::max_custom_emoji_per_space`]
/// pre-check. Empty when that cap is `None`.
///
/// Creates run before destroys, so outside `atomic` the backend still
/// refuses a create that only a later destroy would make room for; the
/// discount keeps the pre-check from refusing it under `atomic`, where
/// the backend judges the final state.
async fn emoji_space_counts<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    limits: &ChatLimits,
    args: &serde_json::Map<String, Value>,
) -> Result<BTreeMap<Id, u64>, B::Error> {
    let mut counts = BTreeMap::new();
    if limits.max_custom_emoji_per_space.is_none() {
        return Ok(counts);
    }
    let (stored, _) = backend
        .get_objects::<CustomEmoji>(caller, account_id, None, None)
        .await?;
    let destroyed: Vec<&str> = args
        .get("destroy")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for emoji in stored {
        if let Some(space) = emoji.space_id
            && !destroyed.contains(&emoji.id.as_ref())
        {
            *counts.entry(space).or_default() += 1;
        }
    }
    Ok(counts)
}

/// The `SetError` for an emoji image `blob_id` larger than
/// [`ChatLimits::max_custom_emoji_bytes`] (`overQuota`), or one the
/// backend rejects (`invalidProperties` naming `blobId`). `None` when
/// there is no cap, the blob fits, or the backend does not report it.
async fn emoji_blob_too_large<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    limits: &ChatLimits,
    blob_id: &Id,
) -> Option<Value> {
    let max = limits.max_custom_emoji_bytes?;
    match backend.attachment_blob(caller, account_id, blob_id).await {
        Ok(Ok(Some(blob))) if blob.size > max => Some(over_quota(EMOJI_TOO_LARGE)),
        Ok(Ok(_)) => None,
        Ok(Err(_)) => Some(json!({ "type": "invalidProperties", "properties": ["blobId"] })),
        Err(e) => Some(server_fail_value_from_backend(&e)),
    }
}
