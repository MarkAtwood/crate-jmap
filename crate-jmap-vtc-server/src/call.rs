//! VTCCall/* method handlers (draft-atwood-jmap-vtc-00 §VTCCall Methods).
//!
//! `/get` and `/changes` are the standard RFC 8620 §5.1 and §5.2 methods.
//! `/set` validates call creation, the state machine (§State Machine), ending
//! a call and the call-level moderator actions before handing the mutation
//! to the backend. `/query` and `/queryChanges` supply the draft's default
//! sort and the `chatId` / `spaceId` capability rule before the standard
//! RFC 8620 §5.5 and §5.6 handling.
//!
//! Each handler returns `Err(JmapError)` for method-level failures; per-
//! record failures inside `/set` land in `notCreated` / `notUpdated` /
//! `notDestroyed`. The extra-invocation list is always empty.

use jmap_types::{Id, JmapError, PatchObject, UTCDate};
use jmap_vtc_types::query::{VtcCallComparator, VtcCallFilter};
use jmap_vtc_types::{CallEndReason, CallState, CallType, MediaKind, VtcCall};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::backend::{BackendSetError, SetError, SetErrorType, VtcBackend};
use crate::helpers::{
    backend_set_error_value, default_sort, enforce_max_objects_in_set, extract_account_id,
    fetch_one, finalize_set_response, forbidden, invalid, reject_unsupported_sort, set_error_value,
    utc_key, SetAccumulators, PLACEHOLDER_ID,
};
use crate::media::stop_recordings;
use jmap_server::{
    now_utc_string, server_fail_from_backend, server_fail_value_from_backend, HandlerOutput,
};

/// Capability URI of draft-atwood-jmap-chat-00, which gates the Chat binding
/// properties.
const JMAP_CHAT_URI: &str = "urn:ietf:params:jmap:chat";

/// Properties a `VTCCall/set` create may not carry: "The server sets `id`,
/// `initiatorId`, `createdAt`, `state`, and `joinUri`", plus the other
/// properties §VTCCall marks server-set.
const SERVER_SET_ON_CREATE: &[&str] = &[
    "id",
    "accountId",
    "state",
    "createdAt",
    "endedAt",
    "endReason",
    "initiatorId",
    "joinUri",
    "activeParticipantCount",
    "breakoutRoomIds",
];

/// Properties a `VTCCall/set` update may not patch: those §VTCCall marks
/// immutable or server-set, except `state`, whose transitions "clients
/// request ... via `VTCCall/set`", and the server-generated `joinUri`.
const NOT_PATCHABLE: &[&str] = &[
    "id",
    "accountId",
    "callType",
    "createdAt",
    "endedAt",
    "endReason",
    "initiatorId",
    "joinUri",
    "activeParticipantCount",
    "parentCallId",
    "breakoutRoomIds",
];

/// Properties only a moderator may patch: ending the call (§Ending a Call),
/// "Toggle lobby" (§Moderator Actions on VTCCall), `policy` ("Mutable by
/// moderators only") and `e2eeEnabled` ("the server MUST return
/// `forbidden` if a non-moderator attempts to update `e2eeEnabled`").
const MODERATOR_ONLY: &[&str] = &["state", "lobbyEnabled", "policy", "e2eeEnabled"];

/// Handle `VTCCall/get` (draft-atwood-jmap-vtc-00 §VTCCall/get): standard
/// RFC 8620 §5.1 `/get`.
pub async fn handle_vtc_call_get<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_get::<VtcCall, B>(backend, caller, args).await
}

/// Handle `VTCCall/changes` (draft-atwood-jmap-vtc-00 §VTCCall/changes):
/// standard RFC 8620 §5.2 `/changes`.
pub async fn handle_vtc_call_changes<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_changes::<VtcCall, B>(backend, caller, args).await
}

/// Handle `VTCCall/query` (draft-atwood-jmap-vtc-00 §VTCCall/query).
///
/// Supplies the "Default sort: `createdAt` descending", refuses a sort
/// property other than `createdAt` / `endedAt` with `unsupportedSort`, and
/// refuses a `chatId` or `spaceId` filter with `unsupportedFilter` unless
/// the account advertises `urn:ietf:params:jmap:chat`. The backend
/// evaluates the filter; [`crate::query::call_matches`] and
/// [`crate::query::compare_calls`] implement it.
pub async fn handle_vtc_call_query<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args = prepare_query(backend, caller, args).await?;
    jmap_server::handlers::handle_query::<VtcCall, B>(backend, caller, args).await
}

/// Handle `VTCCall/queryChanges` (draft-atwood-jmap-vtc-00
/// §VTCCall/queryChanges). RFC 8620 §5.6: the `filter` and `sort` are
/// those of the original query, so the `/query` rules apply here too.
pub async fn handle_vtc_call_query_changes<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args = prepare_query(backend, caller, args).await?;
    jmap_server::handlers::handle_query_changes::<VtcCall, B>(backend, caller, args).await
}

async fn prepare_query<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
) -> Result<Value, JmapError> {
    if let Value::Object(map) = &mut args {
        default_sort(
            map,
            json!([{ "property": "createdAt", "isAscending": false }]),
        );
    }
    reject_unsupported_sort::<VtcCallComparator>(args.get("sort"))?;
    if let Some(filter) = args.get("filter").filter(|f| !f.is_null()) {
        let filter =
            VtcCallFilter::deserialize(filter).map_err(|_| JmapError::unsupported_filter())?;
        let account_id = args
            .get("accountId")
            .and_then(Value::as_str)
            .map(Id::from)
            .ok_or_else(|| JmapError::invalid_arguments("accountId is required"))?;
        if uses_chat_binding(&filter) {
            // RFC 8620 §3.6.2: an unknown account is `accountNotFound`,
            // whatever the filter.
            if !backend
                .account_exists(caller, &account_id)
                .await
                .map_err(|e| server_fail_from_backend(&e))?
            {
                return Err(JmapError::account_not_found());
            }
            if !backend.has_capability(caller, &account_id, JMAP_CHAT_URI) {
                return Err(JmapError::unsupported_filter());
            }
        }
    }
    Ok(args)
}

/// `true` if any condition in `filter` sets `chatId` or `spaceId`.
fn uses_chat_binding(filter: &VtcCallFilter) -> bool {
    match filter {
        VtcCallFilter::Condition(c) => c.chat_id.is_some() || c.space_id.is_some(),
        VtcCallFilter::Operator(op) => op.conditions.iter().any(uses_chat_binding),
        _ => false,
    }
}

/// Handle `VTCCall/set` (draft-atwood-jmap-vtc-00 §VTCCall/set).
///
/// Create: the `callType`-specific rules of §Creating a Ring Call, §Creating
/// a Room Call and §Creating a Scheduled Call, then
/// [`VtcBackend::create_call`]. Update: ending a call and the call-level
/// moderator actions, validated against §State Machine; ending a call
/// runs the breakout-room and recording cascade described at [`end_call`].
/// Destroy is held
/// to the same permission as ending the call, then forwarded to the
/// backend.
pub async fn handle_vtc_call_set<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: an unknown accountId is `accountNotFound`.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<VtcCall>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(Value::as_str) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut acc = SetAccumulators::default();
    let mut mutated = false;

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj) in create_map {
            match create_call(backend, caller, &account_id, obj).await {
                Ok(call) => {
                    mutated = true;
                    acc.created.insert(
                        create_id,
                        serde_json::to_value(&call)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Err(e) => {
                    acc.not_created.insert(create_id, e);
                }
            }
        }
    }

    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch) in update_map {
            let id = Id::from(id_str.as_str());
            match update_call(backend, caller, &account_id, &id, patch).await {
                Ok(echo) => {
                    mutated = true;
                    acc.updated.insert(id_str, echo);
                }
                Err(e) => {
                    acc.not_updated.insert(id_str, e);
                }
            }
        }
    }

    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of `destroy` is an Id.
        let ids: Vec<&str> = destroy_arr
            .iter()
            .map(|v| v.as_str().ok_or(v))
            .collect::<Result<_, _>>()
            .map_err(|bad| {
                JmapError::invalid_arguments(format!(
                    "destroy: every element must be a string Id; got {bad}"
                ))
            })?;
        for id_str in ids {
            match destroy_call(backend, caller, &account_id, &Id::from(id_str)).await {
                Ok(()) => {
                    mutated = true;
                    acc.destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    acc.not_destroyed.insert(id_str.to_owned(), e);
                }
            }
        }
    }

    finalize_set_response::<B, VtcCall>(backend, caller, &account_id, old_state, mutated, acc).await
}

/// Validate one `create` entry, fill the server-set properties and hand it
/// to [`VtcBackend::create_call`]. The `Err` value is the SetError JSON.
async fn create_call<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    obj: Value,
) -> Result<VtcCall, Value> {
    let Value::Object(mut m) = obj else {
        return Err(invalid(&[], "a VTCCall create must be a JSON object"));
    };

    let server_set: Vec<&str> = SERVER_SET_ON_CREATE
        .iter()
        .copied()
        .filter(|k| m.contains_key(*k))
        .collect();
    if !server_set.is_empty() {
        return Err(invalid(
            &server_set,
            "these properties are set by the server",
        ));
    }

    let call_type = match m.get("callType").map(CallType::deserialize) {
        Some(Ok(t)) if !matches!(t, CallType::Other(_)) => t,
        _ => {
            return Err(invalid(
                &["callType"],
                "callType must be \"ring\", \"room\" or \"scheduled\"",
            ))
        }
    };

    let cap = backend
        .vtc_capability(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    if !cap.may_create_call {
        return Err(forbidden("mayCreateCall is false for this account"));
    }
    let (supported, initial_state) = match call_type {
        CallType::Ring => (cap.supports_ring_calls, CallState::Creating),
        CallType::Room => (cap.supports_room_calls, CallState::Active),
        _ => (cap.supports_scheduled_calls, CallState::Pending),
    };
    if !supported {
        return Err(forbidden("this call type is not supported"));
    }

    let media_types: Vec<MediaKind> = m
        .get("mediaTypes")
        .and_then(|v| Vec::<MediaKind>::deserialize(v).ok())
        .ok_or_else(|| invalid(&["mediaTypes"], "mediaTypes is required"))?;
    if media_types
        .iter()
        .any(|k| !cap.supported_media_types.contains(k))
    {
        return Err(invalid(
            &["mediaTypes"],
            "mediaTypes contains values not in supportedMediaTypes",
        ));
    }

    let targets = m.remove("targetParticipantIds");
    let ring_targets: Vec<String> = if call_type == CallType::Ring {
        targets
            .and_then(|v| Vec::<String>::deserialize(v).ok())
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                invalid(
                    &["targetParticipantIds"],
                    "targetParticipantIds must be a non-empty array of userIds",
                )
            })?
    } else {
        Vec::new()
    };

    let now = now_utc_string();
    if call_type == CallType::Scheduled {
        let start = m
            .get("scheduledStartAt")
            .and_then(Value::as_str)
            .and_then(|s| UTCDate::new_validated(s).ok());
        if !start.is_some_and(|s| utc_key(&s) > utc_key(&now)) {
            return Err(invalid(
                &["scheduledStartAt"],
                "scheduledStartAt must be a UTCDate in the future",
            ));
        }
    }

    if m.get("chatId").is_some_and(|v| !v.is_null())
        && !backend.has_capability(caller, account_id, JMAP_CHAT_URI)
    {
        return Err(invalid(
            &["chatId"],
            "chatId requires the urn:ietf:params:jmap:chat capability",
        ));
    }

    if let Some(parent_id) = m.get("parentCallId").and_then(Value::as_str) {
        let parent = fetch_one::<VtcCall, _>(backend, caller, account_id, &Id::from(parent_id))
            .await?
            .ok_or_else(|| invalid(&["parentCallId"], "parentCallId does not name a call"))?;
        let moderator = backend
            .is_moderator(caller, account_id, &parent)
            .await
            .map_err(|e| server_fail_value_from_backend(&e))?;
        if !moderator {
            return Err(forbidden(
                "only a moderator of the parent call may create a breakout room",
            ));
        }
    }

    let initiator = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;

    for (key, value) in [
        ("id", json!(PLACEHOLDER_ID)),
        ("accountId", json!(account_id)),
        ("state", json!(initial_state)),
        ("createdAt", json!(now)),
        ("endedAt", Value::Null),
        ("endReason", Value::Null),
        ("initiatorId", json!(initiator)),
        ("joinUri", json!("")),
        ("activeParticipantCount", json!(0)),
    ] {
        m.insert(key.to_owned(), value);
    }

    let call: VtcCall = serde_json::from_value(Value::Object(m))
        .map_err(|e| invalid(&[], &format!("invalid VTCCall: {e}")))?;

    backend
        .create_call(caller, account_id, call, &ring_targets)
        .await
        .map_err(backend_set_error_value)
}

/// `true` if the caller may end `call`. §Ending a Call: "A moderator (or
/// the initiator of a ring call)".
async fn may_end_call<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
) -> Result<bool, Value> {
    if backend
        .is_moderator(caller, account_id, call)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        return Ok(true);
    }
    Ok(call.call_type == CallType::Ring
        && backend
            .caller_user_id(caller, account_id)
            .await
            .map_err(|e| server_fail_value_from_backend(&e))?
            == call.initiator_id)
}

/// Check one `destroy` entry and hand it to the backend.
///
/// Destroying a call removes it as surely as ending it, so it takes the
/// same permission: §Moderator Privilege Escalation requires `forbidden` for "any
/// `VTCCall/set` operation restricted to moderators (ending a call, ...
/// creating or closing breakout rooms, ...)".
async fn destroy_call<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<(), Value> {
    let call = fetch_one::<VtcCall, _>(backend, caller, account_id, id)
        .await?
        .ok_or_else(|| set_error_value(&SetError::new(SetErrorType::NotFound)))?;
    if !may_end_call(backend, caller, account_id, &call).await? {
        return Err(forbidden("the caller is not a moderator on this call"));
    }
    backend
        .destroy_object::<VtcCall>(caller, account_id, id)
        .await
        .map_err(backend_set_error_value)
}

/// Validate one `update` entry and hand it to the backend. `Ok` is the
/// `updated` echo: the properties the server changed beyond the patch.
async fn update_call<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    patch: Value,
) -> Result<Value, Value> {
    let mut patch: PatchObject = serde_json::from_value(patch).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;

    let mut keys: Vec<&str> = patch
        .as_map()
        .keys()
        .map(|k| k.split('/').next().unwrap_or(k))
        .collect();
    keys.sort_unstable();
    keys.dedup();

    let fixed: Vec<&str> = keys
        .iter()
        .copied()
        .filter(|k| NOT_PATCHABLE.contains(k))
        .collect();
    if !fixed.is_empty() {
        return Err(invalid(&fixed, "these properties cannot be updated"));
    }

    let call = fetch_one::<VtcCall, _>(backend, caller, account_id, id)
        .await?
        .ok_or_else(|| set_error_value(&SetError::new(SetErrorType::NotFound)))?;

    if keys.iter().any(|k| MODERATOR_ONLY.contains(k)) {
        let allowed = if keys == ["state"] {
            may_end_call(backend, caller, account_id, &call).await?
        } else {
            backend
                .is_moderator(caller, account_id, &call)
                .await
                .map_err(|e| server_fail_value_from_backend(&e))?
        };
        if !allowed {
            return Err(forbidden("the caller is not a moderator on this call"));
        }
    }

    let mut echo = Value::Null;
    if let Some(requested) = patch.as_map().get("state") {
        if call.state == CallState::Ended {
            return Err(invalid(
                &["state"],
                "the call is already in state \"ended\"",
            ));
        }
        let end_reason = match (
            CallState::deserialize(requested),
            end_reason_for(&call.state),
        ) {
            (Ok(CallState::Ended), Some(reason)) => reason,
            _ => {
                return Err(invalid(
                    &["state"],
                    "this state transition is not permitted by the state machine",
                ))
            }
        };
        // §Ending a Call: "The server sets `endedAt` ... The server sets
        // `endReason` based on context".
        let ended_at = json!(now_utc_string());
        let end_reason = json!(end_reason);
        let map = patch.as_map_mut();
        map.insert("endedAt".to_owned(), ended_at.clone());
        map.insert("endReason".to_owned(), end_reason.clone());
        echo = json!({ "endedAt": ended_at, "endReason": end_reason });
    }

    let result = if echo.is_null() {
        backend
            .update_object::<VtcCall>(caller, account_id, id, patch)
            .await
    } else {
        end_cascade(backend, caller, account_id, &call, patch).await
    };
    match result {
        Ok(Some(obj)) if echo.is_null() => {
            Ok(serde_json::to_value(&obj).expect("derive(Serialize) on plain data is infallible"))
        }
        Ok(_) => Ok(echo),
        Err(e) => Err(backend_set_error_value(e)),
    }
}

/// The `endReason` for ending a call in `state` (§Ending a Call):
/// "`"completed"`: The call was in state `"active"` when ended" and
/// "`"cancelled"`" for a ringing call, or a pending one ("The initiator
/// withdrew a ringing or pending call", §VTCCall `endReason`). `None` when
/// the state machine has no transition from `state` to `"ended"` that a
/// caller can request.
fn end_reason_for(state: &CallState) -> Option<CallEndReason> {
    match state {
        CallState::Active => Some(CallEndReason::Completed),
        CallState::Ringing | CallState::Pending => Some(CallEndReason::Cancelled),
        _ => None,
    }
}

/// The patch that ends a call: `state` `"ended"` with the `endedAt` and
/// `endReason` §Ending a Call says the server sets.
fn ended_patch(end_reason: CallEndReason) -> PatchObject {
    let mut patch = PatchObject::new();
    let map = patch.as_map_mut();
    map.insert("state".to_owned(), json!(CallState::Ended));
    map.insert("endedAt".to_owned(), json!(now_utc_string()));
    map.insert("endReason".to_owned(), json!(end_reason));
    patch
}

/// End `id` on the server's own initiative: the "deployment-defined
/// inactivity timeout" and "All participants leave" transitions of §State
/// Machine, which no `VTCCall/set` request makes.
///
/// The kit runs no timer. A deployment calls this when its own timeout
/// fires or the last participant leaves, with the `end_reason` the draft
/// assigns (§VTCCall `endReason`, for example
/// [`CallEndReason::Timeout`] or [`CallEndReason::Completed`]). The call
/// ends with the same cascade as a moderator's `VTCCall/set`:
///
/// - every breakout room under it that has not ended is ended first
///   ("When a parent call ends, all child breakout-room VTCCalls MUST also
///   be ended (transitioning them to `"ended"` and setting `endedAt`)",
///   jmap-vtc-implementer-guide §Breakout room cleanup), with the
///   `endReason` §Ending a Call gives for its own state, and leaves the
///   parent's `breakoutRoomIds`;
/// - each ended call's recordings in state `"recording"` or `"paused"` are
///   stopped, with `stoppedAt` set and a `VTCRecordingStateEvent`
///   delivered (jmap-vtc-recording-guide §Auto-stop on call end).
///
/// `caller` is the server's own context; the moderator checks of
/// `VTCCall/set` do not apply, and it is the `initiatedBy` of each
/// `VTCRecordingStateEvent`.
///
/// # Errors
///
/// `notFound` for an unknown `id`, and `invalidProperties` naming `state`
/// when the call has already ended. Otherwise the first backend error;
/// calls ended and recordings stopped before it stay so.
// Same error type as `VtcBackend::update_object`, which it propagates.
#[allow(clippy::result_large_err)]
pub async fn end_call<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    end_reason: CallEndReason,
) -> Result<(), BackendSetError<B::Error>> {
    let (found, _) = backend
        .get_objects::<VtcCall>(caller, account_id, Some(std::slice::from_ref(id)), None)
        .await
        .map_err(BackendSetError::Other)?;
    let call = found
        .into_iter()
        .next()
        .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?;
    if call.state == CallState::Ended {
        return Err(BackendSetError::SetError(
            SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["state"])
                .with_description("the call is already in state \"ended\""),
        ));
    }
    end_cascade(backend, caller, account_id, &call, ended_patch(end_reason))
        .await
        .map(|_| ())
}

/// Apply `patch`, which ends `call`, after ending every breakout room
/// beneath it, and stop each ended call's active recordings. Returns
/// [`VtcBackend::update_object`]'s result for `call`.
///
/// jmap-vtc-implementer-guide §Breakout room cleanup: "Implement
/// parent-call end as a cascade that ends all child calls first." The
/// rooms are collected breadth-first through `breakoutRoomIds` and ended
/// deepest first, so every call ends after the rooms under it.
// Same error type as `VtcBackend::update_object`, which it propagates.
#[allow(clippy::result_large_err)]
async fn end_cascade<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
    patch: PatchObject,
) -> Result<Option<VtcCall>, BackendSetError<B::Error>> {
    let mut rooms: Vec<VtcCall> = Vec::new();
    let mut pending = call.breakout_room_ids.clone();
    while !pending.is_empty() {
        let (found, _) = backend
            .get_objects::<VtcCall>(caller, account_id, Some(&pending), None)
            .await
            .map_err(BackendSetError::Other)?;
        pending = Vec::new();
        for room in found {
            // The id checks stop a corrupt store whose rooms form a cycle.
            if room.state == CallState::Ended
                || room.id == call.id
                || rooms.iter().any(|r| r.id == room.id)
            {
                continue;
            }
            pending.extend(room.breakout_room_ids.iter().cloned());
            rooms.push(room);
        }
    }

    for room in rooms.iter().rev() {
        // Only a `"creating"` room, which nobody answered, or an
        // unrecognized state lacks a §Ending a Call reason.
        let reason = end_reason_for(&room.state).unwrap_or(CallEndReason::Cancelled);
        backend
            .update_object::<VtcCall>(caller, account_id, &room.id, ended_patch(reason))
            .await?;
        stop_recordings(backend, caller, account_id, &room.id).await?;
    }
    let result = backend
        .update_object::<VtcCall>(caller, account_id, &call.id, patch)
        .await?;
    stop_recordings(backend, caller, account_id, &call.id).await?;
    Ok(result)
}
