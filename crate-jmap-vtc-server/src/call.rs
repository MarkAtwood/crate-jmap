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
use jmap_vtc_types::{
    CallEndReason, CallResponse, CallState, CallType, MediaKind, RingEndReason, VtcCall,
    VtcCallEndEvent, VtcEvent, VtcParticipant,
};
use jmap_vtc_types::{VtcCallComparator, VtcCallFilter};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::helpers::{
    backend_set_error_value, default_sort, enforce_max_objects_in_set, extract_account_id,
    fetch_one, finalize_set_response, forbidden, invalid, reject_unsupported_sort, set_error_value,
    state_refused, utc_key, SetAccumulators,
};
use crate::media::{stop_livestreams, stop_recordings};
use crate::{BackendSetError, SetError, SetErrorType, VtcBackend};
use jmap_server::{
    now_utc_string, reject_read_only_patch, reject_server_set, server_fail_from_backend,
    server_fail_value_from_backend, HandlerOutput,
};

/// Capability URI of draft-atwood-jmap-chat-00, which gates the Chat binding
/// properties.
pub(crate) const JMAP_CHAT_URI: &str = "urn:ietf:params:jmap:chat";

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
    jmap_server::handle_get::<VtcCall, B>(backend, caller, args).await
}

/// Handle `VTCCall/changes` (draft-atwood-jmap-vtc-00 §VTCCall/changes):
/// standard RFC 8620 §5.2 `/changes`.
pub async fn handle_vtc_call_changes<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<VtcCall, B>(backend, caller, args).await
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
    jmap_server::handle_query::<VtcCall, B>(backend, caller, args).await
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
    jmap_server::handle_query_changes::<VtcCall, B>(backend, caller, args).await
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

    // "The server sets `id`, `initiatorId`, `createdAt`, `state`, and
    // `joinUri`", with the other properties §VTCCall marks server-set.
    // `state` is not in `VtcCall::SERVER_SET`: its transitions "clients
    // request ... via `VTCCall/set`".
    if let Some(e) = reject_server_set::<VtcCall>(&m) {
        return Err(set_error_value(
            &e.with_description("these properties are set by the server"),
        ));
    }
    if m.contains_key("state") {
        return Err(invalid(
            &["state"],
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
        ("id", json!("")),
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

    // `id` is required by the type, so the parse uses "", which is not an
    // Id (RFC 8620 §1.2: "at least 1 and a maximum of 255 octets"); the
    // builder overwrites it with the id the backend mints.
    let mut call: VtcCall = serde_json::from_value(Value::Object(m))
        .map_err(|e| invalid(&[], &format!("invalid VTCCall: {e}")))?;

    backend
        .create_call(
            caller,
            account_id,
            move |id: &Id| {
                call.id = id.clone();
                call
            },
            &ring_targets,
        )
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
    let patch: PatchObject = serde_json::from_value(patch).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;

    let mut keys: Vec<&str> = patch
        .as_map()
        .keys()
        .map(|k| k.split('/').next().unwrap_or(k))
        .collect();
    keys.sort_unstable();
    keys.dedup();

    // §VTCCall: the immutable and server-set properties, `joinUri`
    // included; `state` stays patchable.
    if let Some(e) = reject_read_only_patch::<VtcCall>(patch.as_map(), None) {
        return Err(set_error_value(
            &e.with_description("these properties cannot be updated"),
        ));
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

    // §VTCCall `e2eeEnabled`: "The server SHOULD reject an update from
    // `true` to `false` with `invalidProperties` naming `e2eeEnabled`, so
    // that participants never have to detect a downgrade." A `null` value
    // resets the property to its default (RFC 8620 §5.3), `false`, so any
    // value but `true` is a downgrade.
    if call.e2ee_enabled
        && patch
            .as_map()
            .get("e2eeEnabled")
            .is_some_and(|v| *v != Value::Bool(true))
    {
        return Err(invalid(
            &["e2eeEnabled"],
            "e2eeEnabled cannot change from true to false",
        ));
    }

    let Some(requested) = patch.as_map().get("state") else {
        return match backend
            .update_object::<VtcCall>(caller, account_id, id, patch)
            .await
        {
            Ok(Some(obj)) => {
                Ok(serde_json::to_value(&obj)
                    .expect("derive(Serialize) on plain data is infallible"))
            }
            Ok(None) => Ok(Value::Null),
            Err(e) => Err(backend_set_error_value(e)),
        };
    };
    let wants_end = matches!(CallState::deserialize(requested), Ok(CallState::Ended));
    let mut call = call;
    // A call whose state changed since the read (a ring answered, a
    // scheduled call joined) ends with the reason for its new state, as
    // `end_cascade` does for rooms: "`"active"` → `"ended"`: ... a
    // moderator ends the call" (§State Machine). The loop ends because
    // §State Machine has no cycle back from a later state.
    loop {
        if call.state == CallState::Ended {
            return Err(invalid(
                &["state"],
                "the call is already in state \"ended\"",
            ));
        }
        let end_reason = match end_reason_for(&call.state) {
            Some(reason) if wants_end => reason,
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
        let mut attempt = patch.clone();
        let map = attempt.as_map_mut();
        map.insert("endedAt".to_owned(), ended_at.clone());
        map.insert("endReason".to_owned(), end_reason.clone());
        let from = std::slice::from_ref(&call.state);
        match end_cascade(backend, caller, account_id, &call, from, attempt).await {
            Ok(_) => {
                if call.state == CallState::Ringing {
                    // §VTCCallEndEvent: "When a ringing call is answered,
                    // declined, cancelled, or times out, the server
                    // delivers a `VTCCallEndEvent` to all devices that
                    // received the ring."
                    deliver_call_end(backend, caller, account_id, id, RingEndReason::Cancelled)
                        .await;
                }
                return Ok(json!({ "endedAt": ended_at, "endReason": end_reason }));
            }
            Err(BackendSetError::SetError(e)) if is_state_refusal(&e) => {
                let now = fetch_one::<VtcCall, _>(backend, caller, account_id, id)
                    .await?
                    .ok_or_else(|| set_error_value(&SetError::new(SetErrorType::NotFound)))?;
                if now.state == call.state {
                    return Err(set_error_value(&e));
                }
                call = now;
            }
            Err(e) => return Err(backend_set_error_value(e)),
        }
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

/// Move call `id` from one of `from` to `state`, a transition a
/// participant's join or answer causes (§State Machine). A call no longer
/// in one of `from`, for example one a moderator ended since the caller
/// read it, is left as it is: "The server MUST NOT transition a call out
/// of `"ended"`". The `Err` value is the SetError JSON.
pub(crate) async fn set_call_state<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    from: &[CallState],
    state: CallState,
) -> Result<(), Value> {
    let mut patch = PatchObject::new();
    patch.as_map_mut().insert("state".to_owned(), json!(state));
    backend
        .update_object_if::<VtcCall>(caller, account_id, id, from, patch)
        .await
        .map(|_| ())
        .map_err(backend_set_error_value)
}

/// Whether `e` is the `invalidProperties` naming `state` that
/// [`end_call`] returns for a call not in one of its `from` states.
pub(crate) fn is_state_refusal(e: &SetError) -> bool {
    e.error_type == SetErrorType::InvalidProperties
        && e.properties.as_deref() == Some(&["state".to_owned()][..])
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

/// End `id` on the server's own initiative, a transition no `VTCCall/set`
/// request makes, such as a room call's "deployment-defined inactivity
/// timeout" (§State Machine).
///
/// The kit runs no timer. A deployment calls this when its own timeout
/// fires, with the `end_reason` the draft assigns (§VTCCall `endReason`,
/// for example [`CallEndReason::Timeout`]: "A deployment-defined
/// inactivity timeout fired on an active call with no participants").
/// The last participant leaving needs no call here: `VTCParticipant/set`
/// already ends a ring or scheduled call "with `endReason: "completed"`,
/// or `"cancelled"` if the call was still `"ringing"`", and "A room call
/// stays `"active"` when it empties" (§Leaving a Call). The call ends with
/// the same cascade as a moderator's `VTCCall/set`:
///
/// - every breakout room under it that has not ended is ended first
///   ("When a parent call ends, all child breakout-room VTCCalls MUST also
///   be ended (transitioning them to `"ended"` and setting `endedAt`)",
///   jmap-vtc-implementer-guide §Breakout room cleanup), with the
///   `endReason` §Ending a Call gives for its own state, and leaves the
///   parent's `breakoutRoomIds`;
/// - each ended call's recordings in state `"recording"` or `"paused"` are
///   stopped, with `stoppedAt` set and a `VTCRecordingStateEvent`
///   delivered (jmap-vtc-recording-guide §Auto-stop on call end);
/// - a `"ringing"` call that ends `"cancelled"` delivers a
///   `VTCCallEndEvent` with `endReason` `"cancelled"` through
///   [`VtcBackend::deliver_event`]: "When a ringing call is answered,
///   declined, cancelled, or times out, the server delivers a
///   `VTCCallEndEvent` to all devices that received the ring"
///   (§VTCCallEndEvent). `VTCCall/set` does the same when the initiator
///   ends a ringing call.
///
/// `from` lists the states `end_reason` applies to, for example
/// `[CallState::Active]` for `"timeout"`. The call ends only while its
/// stored state is one of them, checked by
/// [`VtcBackend::update_object_if`] atomically with the write, so a
/// reason chosen from an earlier read never lands on a call that has
/// moved on: a ring call answered since then is not ended `"missed"`.
///
/// `caller` is the server's own context; the moderator checks of
/// `VTCCall/set` do not apply, and it is the `initiatedBy` of each
/// `VTCRecordingStateEvent`.
///
/// # Errors
///
/// `notFound` for an unknown `id`, and `invalidProperties` naming `state`
/// when the call has already ended or is not in one of `from` when the
/// end reaches the store. Otherwise the first backend error; calls ended
/// and recordings stopped before it stay so.
// Same error type as `VtcBackend::update_object`, which it propagates.
#[allow(clippy::result_large_err)]
pub async fn end_call<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    from: &[CallState],
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
    if !from.contains(&call.state) {
        return Err(BackendSetError::SetError(state_refused()));
    }
    let cancelled = call.state == CallState::Ringing && end_reason == CallEndReason::Cancelled;
    let patch = ended_patch(end_reason);
    end_cascade(backend, caller, account_id, &call, from, patch).await?;
    if cancelled {
        deliver_call_end(backend, caller, account_id, id, RingEndReason::Cancelled).await;
    }
    Ok(())
}

/// The ring timeout period draft-atwood-jmap-vtc-00 §Ring Timeout
/// recommends: "a deployment-defined period (RECOMMENDED: 30 seconds)".
/// The default of [`VtcBackend::ring_timeout`].
pub const RING_TIMEOUT: Duration = Duration::from_secs(30);

/// Enforce the ring timeout of ring call `id` at the instant `now`
/// (draft-atwood-jmap-vtc-00 §Ring Timeout).
///
/// The kit runs no timer. A deployment calls this once per ring call when
/// its own timer fires, [`VtcBackend::ring_timeout`] after the call's
/// `createdAt`. `now` is the deployment's clock reading. Before
/// `createdAt` plus that period, or when
/// the call is not a ring call in state `"ringing"` or `"active"`, nothing
/// happens. Otherwise:
///
/// - a `"ringing"` call ends through [`end_call`] with `endReason`
///   `"missed"`, from `"ringing"` only; a call answered since the read is
///   read again and handled as `"active"`. Then a `VTCCallEndEvent` with
///   `endReason` `"timeout"` is delivered: "If no target participant answers within a
///   deployment-defined period ..., the server transitions the call to
///   `"ended"` with `endReason: "missed"` and dispatches `VTCCallEndEvent`
///   with `endReason: "timeout"` to all ringing devices";
/// - an `"active"` call continues, and the same event is delivered when a
///   target's `callResponse` is still `"pending"`: "If the call is already
///   `"active"` when the period expires, the call continues; the server
///   dispatches `VTCCallEndEvent` with `endReason: "timeout"` to the
///   devices of targets whose `callResponse` is still `"pending"`".
///
/// The event goes through [`VtcBackend::deliver_event`]. `caller` is the
/// server's own context, as for [`end_call`]. Returns `Ok(true)` when the
/// call ended or the event was delivered.
///
/// # Errors
///
/// `notFound` for an unknown `id`, and `invalidProperties` naming
/// `createdAt` when the stored `createdAt` is not an RFC 8620 `UTCDate` at
/// or after 1970. Otherwise the first backend error.
// Same error type as `VtcBackend::update_object`, which it propagates.
#[allow(clippy::result_large_err)]
pub async fn expire_ring<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    now: SystemTime,
) -> Result<bool, BackendSetError<B::Error>> {
    // A `"ringing"` call that left that state before the end reached the
    // store is read again. The loop ends because §State Machine has no
    // transition back to `"ringing"`.
    let call = loop {
        let (found, _) = backend
            .get_objects::<VtcCall>(caller, account_id, Some(std::slice::from_ref(id)), None)
            .await
            .map_err(BackendSetError::Other)?;
        let call = found
            .into_iter()
            .next()
            .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?;
        if call.call_type != CallType::Ring
            || !matches!(call.state, CallState::Ringing | CallState::Active)
        {
            return Ok(false);
        }
        let deadline = ring_deadline(&call.created_at, backend.ring_timeout())
            .map_err(BackendSetError::SetError)?;
        if now < deadline {
            return Ok(false);
        }
        if call.state != CallState::Ringing {
            break call;
        }
        let from = [CallState::Ringing];
        match end_call(
            backend,
            caller,
            account_id,
            id,
            &from,
            CallEndReason::Missed,
        )
        .await
        {
            Err(BackendSetError::SetError(e)) if is_state_refusal(&e) => continue,
            ended => {
                ended?;
                break call;
            }
        }
    };
    if call.state == CallState::Active {
        // ponytail: loads every VTCParticipant of the account, once per
        // ring. Upgrade path: a `callId` filtered `query_objects`.
        let (all, _) = backend
            .get_objects::<VtcParticipant>(caller, account_id, None, None)
            .await
            .map_err(BackendSetError::Other)?;
        let pending = all
            .iter()
            .any(|p| p.call_id == *id && p.call_response == Some(CallResponse::Pending));
        if !pending {
            return Ok(false);
        }
    }
    deliver_call_end(backend, caller, account_id, id, RingEndReason::Timeout).await;
    Ok(true)
}

/// Deliver a `VTCCallEndEvent` for call `call_id` with `end_reason`
/// through [`VtcBackend::deliver_event`], which routes it to the devices
/// that received the ring (§VTCCallEndEvent).
pub(crate) async fn deliver_call_end<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call_id: &Id,
    end_reason: RingEndReason,
) {
    let event: VtcCallEndEvent = serde_json::from_value(json!({
        "accountId": account_id,
        "callId": call_id,
        "endReason": end_reason,
    }))
    .expect("every field is supplied with its type");
    backend
        .deliver_event(caller, account_id, VtcEvent::CallEnd(event))
        .await;
}

/// The instant a ring call created at `created_at` times out: `created_at`
/// plus `timeout` (draft-atwood-jmap-vtc-00 §Ring Timeout).
///
/// # Errors
///
/// `invalidProperties` naming `createdAt` when `created_at` is not an
/// RFC 8620 `UTCDate` at or after 1970.
// The `SetError` both callers return as is.
#[allow(clippy::result_large_err)]
pub(crate) fn ring_deadline(
    created_at: &UTCDate,
    timeout: Duration,
) -> Result<SystemTime, SetError> {
    // Whole seconds suffice for a period of seconds; `utc_key` drops the
    // fractional part `to_epoch_seconds` does not accept.
    UTCDate::from(format!("{}Z", utc_key(created_at)))
        .to_epoch_seconds()
        .ok()
        .and_then(|secs| u64::try_from(secs).ok())
        .and_then(|secs| UNIX_EPOCH.checked_add(Duration::from_secs(secs)))
        .and_then(|created| created.checked_add(timeout))
        .ok_or_else(|| {
            SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["createdAt"])
                .with_description("the call's createdAt is not a UTCDate")
        })
}

/// Apply `patch`, which ends `call`, after ending every breakout room
/// beneath it, and stop each ended call's active recordings and
/// livestreams (§Ending a Call). Returns
/// [`VtcBackend::update_object_if`]'s result for `call`, and the
/// `invalidProperties` SetError naming `state` when the stored `call` is
/// not in one of `from`, the states `patch`'s `endReason` was chosen for.
///
/// jmap-vtc-implementer-guide §Breakout room cleanup: "Implement
/// parent-call end as a cascade that ends all child calls first." The
/// rooms are collected breadth-first through `breakoutRoomIds` and ended
/// deepest first, so every call ends after the rooms under it. They are
/// read with [`VtcBackend::get_objects_unscoped`], so a room `caller`
/// cannot access still ends.
// Same error type as `VtcBackend::update_object`, which it propagates.
#[allow(clippy::result_large_err)]
async fn end_cascade<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
    from: &[CallState],
    patch: PatchObject,
) -> Result<Option<VtcCall>, BackendSetError<B::Error>> {
    let mut rooms: Vec<VtcCall> = Vec::new();
    let mut pending = call.breakout_room_ids.clone();
    while !pending.is_empty() {
        let (found, _) = backend
            .get_objects_unscoped::<VtcCall>(caller, account_id, Some(&pending))
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
        let mut state = room.state.clone();
        // A room whose state changed since the read ends with the reason
        // for its new state, or is skipped once it has ended. The loop ends
        // because §State Machine has no cycle back from a later state.
        loop {
            // Only a `"creating"` room, which nobody answered, or an
            // unrecognized state lacks a §Ending a Call reason.
            let reason = end_reason_for(&state).unwrap_or(CallEndReason::Cancelled);
            let patch = ended_patch(reason);
            match backend
                .update_object_if::<VtcCall>(caller, account_id, &room.id, &[state], patch)
                .await?
            {
                Ok(_) => break,
                Err(CallState::Ended) => break,
                Err(now) => state = now,
            }
        }
        stop_recordings(backend, caller, account_id, &room.id).await?;
        stop_livestreams(backend, caller, account_id, &room.id).await?;
    }
    // `patch` carries the `endReason` the caller chose for the states in
    // `from`, so it applies only from those.
    let result = backend
        .update_object_if::<VtcCall>(caller, account_id, &call.id, from, patch)
        .await?
        .map_err(|_| BackendSetError::SetError(state_refused()))?;
    stop_recordings(backend, caller, account_id, &call.id).await?;
    stop_livestreams(backend, caller, account_id, &call.id).await?;
    Ok(result)
}
