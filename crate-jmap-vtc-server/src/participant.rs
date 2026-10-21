//! VTCParticipant/* method handlers (draft-atwood-jmap-vtc-00
//! §VTCParticipant Methods).
//!
//! `/get` and `/changes` are the standard RFC 8620 §5.1 and §5.2 methods.
//! `/set` create joins a room or scheduled call (§Joining a Room or
//! Scheduled Call) or, with `gatewayData`, dials out through a gateway
//! (§Gateway Dial-Out). `/set` update answers or declines a ring, updates
//! media state under the call policy, leaves, reconnects, carries the
//! participant-level moderator actions and relays `gatewayData/pendingSignal`
//! (§VTCGatewaySignal). `/query` and `/queryChanges` supply the draft's
//! default sort and require a `callId` or `userId` scope.
//!
//! The call transitions a participant causes (§State Machine) run after the
//! participant change has committed: answering a ringing call or joining a
//! pending one activates it, the last decline or departure ends it, and the
//! last moderator's departure promotes an heir.
//!
//! An answer or a decline also delivers a `VTCCallEndEvent`, with
//! `endReason` `"answered_elsewhere"` or `"declined"`, through
//! [`VtcBackend::deliver_event`], for the backend to route to that user's
//! other ringing devices (§Multi-Device Forking, §VTCCallEndEvent).
//!
//! Not here: the per-call access and E2EE visibility rules, which the
//! backend's reads apply ([`VtcBackend`] §Per-call access).

use jmap_types::{Id, JmapError, PatchObject};
use jmap_vtc_types::query::{Filter, Operator, VtcParticipantComparator, VtcParticipantFilter};
use jmap_vtc_types::{
    CallEndReason, CallResponse, CallRole, CallState, CallType, JoinMethod, LobbyState,
    RingEndReason, VtcCall, VtcEvent, VtcGatewaySignal, VtcMediaState, VtcParticipant,
    VtcUnmuteRequestEvent,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::time::SystemTime;

use crate::backend::{BackendSetError, SetError, SetErrorType, VtcBackend};
use crate::call::{deliver_call_end, end_call, ring_deadline, set_call_state};
use crate::helpers::{
    backend_set_error_value, default_sort, enforce_max_objects_in_set, extract_account_id,
    fetch_one, finalize_set_response, forbidden, invalid, not_found, reject_unsupported_sort,
    set_error_value, utc_key, SetAccumulators,
};
use jmap_server::{
    apply_patch_object, now_utc_string, server_fail_from_backend, server_fail_value_from_backend,
    HandlerOutput,
};

/// Properties a create may not carry: those §VTCParticipant marks
/// server-set, and those "The server sets" at join or dial-out.
const SERVER_SET_ON_CREATE: &[&str] = &[
    "id",
    "userId",
    "role",
    "joinedAt",
    "leftAt",
    "callResponse",
    "lobbyState",
    "speakerTimeMs",
    "kickedBy",
    "e2eeFingerprint",
    "unmuteRequested",
    "breakoutCallId",
];

/// Properties no update may patch: those §VTCParticipant marks immutable or
/// server-set, and `gatewayData`, which the server "stores and relays ...
/// without interpretation" from the gateway. A client reaches
/// `gatewayData` only through [`PENDING_SIGNAL`].
const NOT_PATCHABLE: &[&str] = &[
    "id",
    "callId",
    "userId",
    "joinedAt",
    "speakerTimeMs",
    "e2eeFingerprint",
    "gatewayData",
];

/// The patch path of an outbound gateway signal (§VTCGatewaySignal).
const PENDING_SIGNAL: &str = "gatewayData/pendingSignal";

/// Handle `VTCParticipant/get` (draft-atwood-jmap-vtc-00
/// §VTCParticipant/get): standard RFC 8620 §5.1 `/get`.
pub async fn handle_vtc_participant_get<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_get::<VtcParticipant, B>(backend, caller, args).await
}

/// Handle `VTCParticipant/changes` (draft-atwood-jmap-vtc-00
/// §VTCParticipant/changes): standard RFC 8620 §5.2 `/changes`.
pub async fn handle_vtc_participant_changes<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_changes::<VtcParticipant, B>(backend, caller, args).await
}

/// Handle `VTCParticipant/query` (draft-atwood-jmap-vtc-00
/// §VTCParticipant/query).
///
/// Supplies the "Default sort: `joinedAt` ascending", refuses a sort
/// property other than `joinedAt` / `displayName` with `unsupportedSort`,
/// and refuses a query scoped by neither `callId` nor `userId` with
/// `unsupportedFilter` ("Servers SHOULD require this property on every
/// query unless combined with `userId`"). The backend evaluates the
/// filter; [`crate::query::participant_matches`] and
/// [`crate::query::compare_participants`] implement it.
pub async fn handle_vtc_participant_query<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args = prepare_query(backend, caller, args).await?;
    jmap_server::handlers::handle_query::<VtcParticipant, B>(backend, caller, args).await
}

/// Handle `VTCParticipant/queryChanges` (draft-atwood-jmap-vtc-00
/// §VTCParticipant/queryChanges). RFC 8620 §5.6: the `filter` and `sort`
/// are those of the original query, so the `/query` rules apply here too.
pub async fn handle_vtc_participant_query_changes<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args = prepare_query(backend, caller, args).await?;
    jmap_server::handlers::handle_query_changes::<VtcParticipant, B>(backend, caller, args).await
}

async fn prepare_query<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
) -> Result<Value, JmapError> {
    if let Value::Object(map) = &mut args {
        default_sort(
            map,
            json!([{ "property": "joinedAt", "isAscending": true }]),
        );
    }
    reject_unsupported_sort::<VtcParticipantComparator>(args.get("sort"))?;
    let scoped = args
        .get("filter")
        .filter(|f| !f.is_null())
        .map(|f| VtcParticipantFilter::deserialize(f).map_err(|_| JmapError::unsupported_filter()))
        .transpose()?
        .is_some_and(|f| is_scoped(&f));
    if !scoped {
        // RFC 8620 §3.6.2: an unknown account is `accountNotFound`,
        // whatever the filter.
        let account_id = args
            .get("accountId")
            .and_then(Value::as_str)
            .map(Id::from)
            .ok_or_else(|| JmapError::invalid_arguments("accountId is required"))?;
        if !backend
            .account_exists(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        {
            return Err(JmapError::account_not_found());
        }
        return Err(JmapError::unsupported_filter());
    }
    Ok(args)
}

/// `true` if every participant `filter` can match is limited to one
/// `callId` or one `userId`.
fn is_scoped(filter: &VtcParticipantFilter) -> bool {
    match filter {
        Filter::Condition(c) => c.call_id.is_some() || c.user_id.is_some(),
        Filter::Operator(op) => match op.operator {
            Operator::And => op.conditions.iter().any(is_scoped),
            Operator::Or => !op.conditions.is_empty() && op.conditions.iter().all(is_scoped),
            _ => false,
        },
        _ => false,
    }
}

/// Handle `VTCParticipant/set` (draft-atwood-jmap-vtc-00
/// §VTCParticipant/set).
///
/// Create: §Joining a Room or Scheduled Call, or §Gateway Dial-Out when the
/// create carries `gatewayData`, then [`VtcBackend::create_object`].
/// Update: §Answering a Ring Call, §Declining a Ring Call, §Updating Media
/// State, §Leaving a Call, §Reconnecting, §Participant-Level Moderator
/// Actions and the `gatewayData/pendingSignal` relay of §VTCGatewaySignal.
/// Destroy is `forbidden`: the draft defines no destroy, and a record is
/// the user's "single continuous participant identity" in the call, kept
/// after they leave.
pub async fn handle_vtc_participant_set<B: VtcBackend>(
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
        .get_state::<VtcParticipant>(caller, &account_id)
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
            match create_participant(backend, caller, &account_id, &create_id, obj).await {
                Ok(created) => {
                    mutated = true;
                    acc.created.insert(create_id, created);
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
            match update_participant(backend, caller, &account_id, &id, patch).await {
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
            let id = Id::from(id_str);
            let err = match fetch_one::<VtcParticipant, _>(backend, caller, &account_id, &id).await
            {
                Ok(Some(_)) => forbidden("VTCParticipant records are not destroyed; set leftAt"),
                Ok(None) => not_found(),
                Err(e) => e,
            };
            acc.not_destroyed.insert(id_str.to_owned(), err);
        }
    }

    finalize_set_response::<B, VtcParticipant>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        acc,
    )
    .await
}

fn to_value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("derive(Serialize) on plain data is infallible")
}

/// Decode `v` as one of the `#[non_exhaustive]` VTC types whose every
/// typed field the caller supplies.
fn build<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).expect("every field is supplied with its type")
}

/// Validate one `create` entry, fill the server-set properties and hand it
/// to [`VtcBackend::create_object`]. `Ok` is the `created` entry: "The
/// server responds with the complete VTCParticipant record".
async fn create_participant<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    create_id: &str,
    obj: Value,
) -> Result<Value, Value> {
    let Value::Object(mut m) = obj else {
        return Err(invalid(
            &[],
            "a VTCParticipant create must be a JSON object",
        ));
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

    // "`callId` (String, required)", "`joinMethod` (String, required)".
    let call_id = m
        .get("callId")
        .and_then(Value::as_str)
        .map(Id::from)
        .ok_or_else(|| invalid(&["callId"], "callId is required"))?;
    let join_method = m
        .get("joinMethod")
        .and_then(|v| JoinMethod::deserialize(v).ok())
        .ok_or_else(|| invalid(&["joinMethod"], "joinMethod is required"))?;

    // "The server SHOULD return `notFound` if the `callId` does not exist".
    let call = fetch_one::<VtcCall, _>(backend, caller, account_id, &call_id)
        .await?
        .ok_or_else(not_found)?;
    if call.state == CallState::Ended {
        return Err(invalid(&["callId"], "the call is in state \"ended\""));
    }
    let moderator = backend
        .is_moderator(caller, account_id, &call)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;

    let dial_out = m.get("gatewayData").is_some_and(|v| !v.is_null());
    let fields = if dial_out {
        dial_out_fields(
            backend,
            caller,
            account_id,
            &call,
            &m,
            &join_method,
            moderator,
        )
        .await?
    } else {
        join_fields(backend, caller, account_id, &call, &mut m, moderator).await?
    };
    for (key, value) in fields {
        m.insert(key.to_owned(), value);
    }
    // `id` is required by the type, so the parse uses "", which is not an
    // Id (RFC 8620 §1.2: "at least 1 and a maximum of 255 octets"); the
    // builder overwrites it with the id the backend mints.
    let mut participant: VtcParticipant = serde_json::from_value(Value::Object(m))
        .map_err(|e| invalid(&[], &format!("invalid VTCParticipant: {e}")))?;

    let (_, stored) = backend
        .create_object(caller, account_id, create_id, move |id: &Id| {
            participant.id = id.clone();
            participant
        })
        .await
        .map_err(backend_set_error_value)?;

    // "When a participant joins a `"pending"` scheduled call, the server
    // transitions the call to `"active"`." The join is already committed, so
    // as in `run_effects` the transition is best-effort and its failure is
    // not the create's SetError (RFC 8620 §5.3: `newState` must reflect the
    // committed join).
    // ponytail: the failure is dropped, as the crate has no logging.
    // Upgrade path: a `VtcBackend` hook that reports it.
    // The backend applies it only while the call is still `"pending"`, so a
    // moderator's end since `call` was read stands (§State Machine: "The
    // server MUST NOT transition a call out of `"ended"`").
    if !dial_out && call.state == CallState::Pending {
        let from = [CallState::Pending];
        let _ = set_call_state(
            backend,
            caller,
            account_id,
            &call.id,
            &from,
            CallState::Active,
        )
        .await;
    }
    Ok(to_value(&stored))
}

/// The call states in which a participant may join `call`: §Joining
/// ("For room calls ... `"active"`"; "For scheduled calls ... `"pending"` or
/// `"active"`"), and for a ring call those in which a target may answer.
fn joinable(call: &VtcCall) -> bool {
    match call.call_type {
        CallType::Room => call.state == CallState::Active,
        CallType::Scheduled => matches!(call.state, CallState::Pending | CallState::Active),
        CallType::Ring => matches!(call.state, CallState::Ringing | CallState::Active),
        _ => false,
    }
}

/// The server-set properties of a join (§Joining a Room or Scheduled
/// Call): "The server sets `id`, `userId`, `displayName`, `role`,
/// `joinedAt`, `mediaState` (defaults), and `speakerTimeMs` (to `0`)."
async fn join_fields<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
    m: &mut Map<String, Value>,
    moderator: bool,
) -> Result<Vec<(&'static str, Value)>, Value> {
    if m.contains_key("displayName") {
        return Err(invalid(
            &["displayName"],
            "the server sets displayName at join time",
        ));
    }
    if !joinable(call) {
        return Err(invalid(
            &["callId"],
            "the call cannot be joined in its current state",
        ));
    }
    if call.state == CallState::Pending {
        check_concurrent_calls(backend, caller, account_id).await?;
    }
    let media_state = join_media_state(call, m.remove("mediaState"), moderator)?;
    let me = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;

    // "When `lobbyEnabled` is `true`, the server sets the new participant's
    // `lobbyState` to `"waiting"` and leaves `joinedAt` `null`"; `lobbyState`
    // is "`null` when ... the participant bypassed the lobby (moderators)".
    let lobby = call.lobby_enabled && !moderator;
    let role = if moderator {
        CallRole::Moderator
    } else {
        CallRole::Participant
    };
    Ok(vec![
        ("id", json!("")),
        ("userId", json!(me)),
        ("displayName", json!(me)),
        ("role", json!(role)),
        (
            "joinedAt",
            if lobby {
                Value::Null
            } else {
                json!(now_utc_string())
            },
        ),
        ("leftAt", Value::Null),
        ("callResponse", Value::Null),
        ("gatewayData", Value::Null),
        (
            "lobbyState",
            if lobby {
                json!(LobbyState::Waiting)
            } else {
                Value::Null
            },
        ),
        ("mediaState", to_value(&media_state)),
        ("speakerTimeMs", json!(0)),
        ("kickedBy", Value::Null),
        ("e2eeFingerprint", Value::Null),
        ("unmuteRequested", json!(false)),
        ("breakoutCallId", Value::Null),
    ])
}

/// The joining participant's `mediaState`.
///
/// Without a requested value the server's defaults are those of the
/// §Joining example response (`audio` and `video` on, `screen` and
/// `raisedHand` off), less any field the policy keeps a non-moderator from
/// turning on. A requested value that turns such a field on is `forbidden`
/// (§VTCCallPolicy). Either way, "When `policy.muteOnEntry` is `true`, the
/// server sets `mediaState.audio` to `false` ... regardless of the
/// client's requested value. Similarly for `policy.videoOffOnEntry`".
fn join_media_state(
    call: &VtcCall,
    requested: Option<Value>,
    moderator: bool,
) -> Result<VtcMediaState, Value> {
    let policy = &call.policy;
    let explicit = requested.is_some();
    let mut state: VtcMediaState = match requested {
        Some(v) => VtcMediaState::deserialize(&v)
            .map_err(|_| invalid(&["mediaState"], "mediaState must be a VTCMediaState"))?,
        None => build(json!({
            "audio": true, "video": true, "screen": false, "raisedHand": false
        })),
    };
    if !moderator {
        let blocked_audio = state.audio && !policy.participants_can_unmute;
        let blocked_video = state.video && !policy.participants_can_start_video;
        let blocked_screen = state.screen && !policy.participants_can_share_screen;
        if explicit && (blocked_audio || blocked_video || blocked_screen) {
            return Err(forbidden("the call policy keeps this media off"));
        }
        state.audio &= !blocked_audio;
        state.video &= !blocked_video;
        state.screen &= !blocked_screen;
    }
    state.audio &= !policy.mute_on_entry;
    state.video &= !policy.video_off_on_entry;
    Ok(state)
}

/// "If the account already has `maxConcurrentCalls` active calls, the
/// server MUST return `overQuota`", applied to a join that would make one
/// more call active. The count is the account's, so it comes from
/// [`VtcBackend::active_call_count`], not the caller-scoped `get_objects`.
async fn check_concurrent_calls<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
) -> Result<(), Value> {
    let cap = backend
        .vtc_capability(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    let Some(max) = cap.max_concurrent_calls else {
        return Ok(());
    };
    let active = backend
        .active_call_count(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    if active >= max {
        return Err(set_error_value(
            &SetError::new(SetErrorType::OverQuota)
                .with_description("the account has reached maxConcurrentCalls"),
        ));
    }
    Ok(())
}

/// The checks and server-set properties of §Gateway Dial-Out. "The server
/// creates a VTCParticipant with `joinedAt: null` and initiates the
/// outbound call via the gateway"; the backend places the call.
async fn dial_out_fields<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
    m: &Map<String, Value>,
    join_method: &JoinMethod,
    moderator: bool,
) -> Result<Vec<(&'static str, Value)>, Value> {
    if !moderator {
        return Err(forbidden("only a moderator may dial out"));
    }
    if m.contains_key("mediaState") {
        return Err(invalid(
            &["mediaState"],
            "the server sets a dial-out participant's mediaState",
        ));
    }
    let cap = backend
        .vtc_capability(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    let method = to_value(join_method);
    let dialable = cap
        .gateways
        .iter()
        .flatten()
        .any(|g| g.supports_dial_out && to_value(&g.protocol) == method);
    if !dialable {
        return Err(invalid(
            &["joinMethod"],
            "joinMethod matches no gateway protocol whose supportsDialOut is true",
        ));
    }
    if !m.get("gatewayData").is_some_and(Value::is_object) {
        return Err(invalid(&["gatewayData"], "gatewayData must be an object"));
    }
    let display_name = match m.get("displayName") {
        None => method,
        Some(Value::String(s)) => json!(s),
        Some(_) => return Err(invalid(&["displayName"], "displayName must be a string")),
    };
    if call.e2ee_enabled {
        return Err(forbidden(
            "the call is end-to-end encrypted and this server has no key-holding gateway",
        ));
    }
    Ok(vec![
        ("id", json!("")),
        ("userId", Value::Null),
        ("displayName", display_name),
        ("role", json!(CallRole::Participant)),
        ("joinedAt", Value::Null),
        ("leftAt", Value::Null),
        ("callResponse", Value::Null),
        ("lobbyState", Value::Null),
        (
            "mediaState",
            json!({ "audio": false, "video": false, "screen": false, "raisedHand": false }),
        ),
        ("speakerTimeMs", json!(0)),
        ("kickedBy", Value::Null),
        ("e2eeFingerprint", Value::Null),
        ("unmuteRequested", json!(false)),
        ("breakoutCallId", Value::Null),
    ])
}

/// What an update does to the call and to whom it delivers, once the
/// participant change has committed.
#[derive(Default)]
struct Effects {
    /// Move the call to `"active"`.
    activate: bool,
    /// The participant answered a ring.
    answered: bool,
    /// The participant declined a ring.
    declined: bool,
    /// The participant left or was kicked.
    left: bool,
    /// Deliver a `VTCUnmuteRequestEvent`.
    unmute_request: bool,
    /// Deliver this outbound `VTCGatewaySignal`.
    signal: Option<VtcGatewaySignal>,
}

/// Validate one `update` entry, add the properties the server sets, hand
/// it to the backend and run its [`Effects`]. `Ok` is the `updated` echo:
/// the properties the server changed beyond the patch.
async fn update_participant<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    patch: Value,
) -> Result<Value, Value> {
    let mut patch: PatchObject = serde_json::from_value(patch).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;
    let signal = patch.as_map_mut().remove(PENDING_SIGNAL);

    let mut keys: Vec<String> = patch
        .as_map()
        .keys()
        .map(|k| k.split('/').next().unwrap_or(k).to_owned())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let has = |k: &str| keys.iter().any(|key| key == k);
    let fixed: Vec<&str> = NOT_PATCHABLE.iter().copied().filter(|k| has(k)).collect();
    if !fixed.is_empty() {
        return Err(invalid(&fixed, "these properties cannot be updated"));
    }

    // "An unknown VTCParticipant id yields `notFound`".
    let old = fetch_one::<VtcParticipant, _>(backend, caller, account_id, id)
        .await?
        .ok_or_else(not_found)?;
    let call = fetch_one::<VtcCall, _>(backend, caller, account_id, &old.call_id)
        .await?
        .ok_or_else(not_found)?;
    let me = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    let own = old.user_id.as_deref() == Some(me.as_str());
    let moderator = backend
        .is_moderator(caller, account_id, &call)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    // §Participant-Level Moderator Actions: "Non-moderators SHOULD receive
    // `forbidden` for updates targeting other participants."
    if !own && !moderator {
        return Err(forbidden(
            "only a moderator may update another participant's record",
        ));
    }
    let moderator_only = |what: &str| {
        if moderator {
            Ok(())
        } else {
            Err(forbidden(&format!("only a moderator may {what}")))
        }
    };

    let new = preview(&old, &patch)?;
    // §Multi-Device Forking: a target "may still answer (and join) or
    // decline until the ring timeout expires".
    let ring_expired = || -> Result<bool, Value> {
        if call.call_type != CallType::Ring {
            return Ok(false);
        }
        let deadline = ring_deadline(&call.created_at, backend.ring_timeout())
            .map_err(|e| set_error_value(&e))?;
        Ok(SystemTime::now() >= deadline)
    };
    let now = json!(now_utc_string());
    let mut echo = Map::new();
    let mut effects = Effects::default();

    if has("joinMethod") {
        // §Answering a Ring Call.
        if !own {
            return Err(forbidden("only the target may answer"));
        }
        if !matches!(call.state, CallState::Ringing | CallState::Active)
            || old.call_response != Some(CallResponse::Pending)
        {
            return Err(invalid(
                &["joinMethod"],
                "the call is not ringing or active, or this ring was already answered",
            ));
        }
        if ring_expired()? {
            return Err(invalid(&["joinMethod"], "the ring timeout has expired"));
        }
        echo.insert("joinedAt".to_owned(), now.clone());
        echo.insert("callResponse".to_owned(), json!(CallResponse::Accepted));
        effects.activate = call.state == CallState::Ringing;
        effects.answered = true;
    }

    if has("callResponse") {
        // §Declining a Ring Call.
        if !own {
            return Err(forbidden("only the target may decline"));
        }
        if new.call_response != Some(CallResponse::Declined)
            || !matches!(call.state, CallState::Ringing | CallState::Active)
            || old.call_response != Some(CallResponse::Pending)
        {
            return Err(invalid(
                &["callResponse"],
                "only a pending ring of a ringing or active call can be declined",
            ));
        }
        if ring_expired()? {
            return Err(invalid(&["callResponse"], "the ring timeout has expired"));
        }
        echo.insert("leftAt".to_owned(), now.clone());
        effects.declined = true;
    }

    if has("leftAt") {
        if new.left_at.is_some() {
            // §Leaving a Call; on another's record, a kick.
            if old.left_at.is_some() {
                return Err(invalid(&["leftAt"], "the participant has already left"));
            }
            echo.insert("leftAt".to_owned(), now.clone());
            if !own {
                echo.insert("kickedBy".to_owned(), json!(me));
            }
            effects.left = true;
        } else {
            // §Reconnecting.
            if !own {
                return Err(forbidden("only the participant may rejoin"));
            }
            if old.left_at.is_none() {
                return Err(invalid(&["leftAt"], "the participant has not left"));
            }
            if old.kicked_by.is_some() {
                return Err(forbidden(
                    "a kicked participant cannot rejoin until a moderator clears kickedBy",
                ));
            }
            if !joinable(&call) {
                return Err(invalid(
                    &["callId"],
                    "the call cannot be joined in its current state",
                ));
            }
            if call.lobby_enabled && !moderator {
                echo.insert("joinedAt".to_owned(), Value::Null);
                echo.insert("lobbyState".to_owned(), json!(LobbyState::Waiting));
            } else {
                echo.insert("joinedAt".to_owned(), now.clone());
            }
            effects.activate = call.state == CallState::Pending;
        }
    }

    if has("kickedBy") {
        if new.kicked_by.is_some() {
            return Err(invalid(
                &["kickedBy"],
                "clients can set kickedBy only to null",
            ));
        }
        moderator_only("clear a kick")?;
    }

    if has("lobbyState") {
        // §Lobby Bypass: admission "SHOULD require a moderator-initiated
        // `VTCParticipant/set` update".
        moderator_only("admit or reject a lobby participant")?;
        let stamp = match (&old.lobby_state, &new.lobby_state) {
            (Some(LobbyState::Waiting), Some(LobbyState::Admitted)) => "joinedAt",
            (Some(LobbyState::Waiting), Some(LobbyState::Rejected)) => "leftAt",
            _ => {
                return Err(invalid(
                    &["lobbyState"],
                    "lobbyState may only move from \"waiting\" to \"admitted\" or \"rejected\"",
                ))
            }
        };
        echo.insert(stamp.to_owned(), now.clone());
    }

    if has("role") {
        moderator_only("grant or revoke the moderator role")?;
        if !matches!(new.role, CallRole::Moderator | CallRole::Participant) {
            return Err(invalid(
                &["role"],
                "role must be \"moderator\" or \"participant\"",
            ));
        }
        if own && old.role == CallRole::Moderator && new.role == CallRole::Participant {
            let others = call_participants(backend, caller, account_id, &call.id).await?;
            let heir = others
                .iter()
                .any(|p| p.id != old.id && p.role == CallRole::Moderator && p.left_at.is_none());
            if !heir {
                return Err(forbidden(
                    "the last moderator may not revoke their own moderator role",
                ));
            }
        }
    }

    if has("breakoutCallId") {
        moderator_only("assign a breakout room")?;
        if new
            .breakout_call_id
            .as_ref()
            .is_some_and(|b| !call.breakout_room_ids.contains(b))
        {
            return Err(invalid(
                &["breakoutCallId"],
                "breakoutCallId is not in the call's breakoutRoomIds",
            ));
        }
    }

    if has("unmuteRequested") && new.unmute_requested {
        moderator_only("ask a participant to unmute")?;
        effects.unmute_request = !old.unmute_requested;
    }

    if has("mediaState") && own {
        // §VTCCallPolicy limits what a non-moderator turns on.
        let (was, is) = (&old.media_state, &new.media_state);
        let policy = &call.policy;
        let audio_on = is.audio && !was.audio;
        if !moderator
            && ((audio_on && !policy.participants_can_unmute && !old.unmute_requested)
                || (is.video && !was.video && !policy.participants_can_start_video)
                || (is.screen && !was.screen && !policy.participants_can_share_screen))
        {
            return Err(forbidden("the call policy keeps this media off"));
        }
        // `unmuteRequested`: "Cleared to `false` when the participant
        // unmutes".
        if audio_on && old.unmute_requested && !has("unmuteRequested") {
            echo.insert("unmuteRequested".to_owned(), json!(false));
        }
    }

    if let Some(pending) = signal {
        moderator_only("send a gateway signal")?;
        effects.signal = Some(gateway_signal(account_id, &old, pending)?);
        // The server "removes the key": report the stored value.
        echo.insert("gatewayData".to_owned(), to_value(&old.gateway_data));
    }

    patch.as_map_mut().extend(
        echo.iter()
            .filter(|(k, _)| *k != "gatewayData")
            .map(|(k, v)| (k.clone(), v.clone())),
    );
    let mut echo = Value::Object(echo);
    if !patch.as_map().is_empty() {
        if let Some(obj) = backend
            .update_object::<VtcParticipant>(caller, account_id, id, patch)
            .await
            .map_err(backend_set_error_value)?
        {
            echo = to_value(&obj);
        }
    }

    run_effects(backend, caller, account_id, &old, &me, effects).await;
    Ok(match echo {
        Value::Object(m) if m.is_empty() => Value::Null,
        other => other,
    })
}

/// `old` with `patch` applied, as the update would store it.
fn preview(old: &VtcParticipant, patch: &PatchObject) -> Result<VtcParticipant, Value> {
    let mut base = match to_value(old) {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    apply_patch_object(&mut base, patch.as_map()).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;
    VtcParticipant::deserialize(&Value::Object(base))
        .map_err(|e| invalid(&[], &format!("invalid VTCParticipant: {e}")))
}

/// The outbound `VTCGatewaySignal` a moderator's `gatewayData/pendingSignal`
/// carries (§VTCGatewaySignal): its `signal` and `data`, for the gateway
/// participant `target` of a call hosted in `account_id`.
fn gateway_signal(
    account_id: &Id,
    target: &VtcParticipant,
    pending: Value,
) -> Result<VtcGatewaySignal, Value> {
    if target.join_method == JoinMethod::Webrtc {
        return Err(invalid(
            &["gatewayData"],
            "the participant did not join through a gateway",
        ));
    }
    let (Some(signal), Some(data)) = (
        pending.get("signal").filter(|s| s.is_string()),
        pending.get("data").filter(|d| d.is_object()),
    ) else {
        return Err(invalid(
            &["gatewayData"],
            "pendingSignal needs a string signal and an object data",
        ));
    };
    Ok(build(json!({
        "accountId": account_id,
        "callId": target.call_id,
        "participantId": target.id,
        "protocol": target.join_method,
        "signal": signal,
        "data": data,
        "direction": "outbound",
        "timestamp": now_utc_string(),
    })))
}

/// Every participant record of `call_id`.
///
/// ponytail: loads every VTCParticipant of the account, O(participants)
/// per leave or decline. Upgrade path: a `callId` filtered
/// [`jmap_server::JmapBackend::query_objects`].
async fn call_participants<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call_id: &Id,
) -> Result<Vec<VtcParticipant>, Value> {
    let (all, _) = backend
        .get_objects::<VtcParticipant>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    Ok(all.into_iter().filter(|p| p.call_id == *call_id).collect())
}

/// Apply the call transitions and deliveries of an update that has
/// committed. `old` is the participant before it.
///
/// Best-effort: the update is already stored, so a failed transition is
/// not the update's failure and is not reported as its SetError (RFC 8620
/// §5.3: `newState` must reflect the committed change). The call is
/// reloaded first, and a call that has ended since the request began (for
/// example a moderator's `VTCCall/set` `"ended"`) takes no transition: a
/// leave after the end only records `leftAt`.
async fn run_effects<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    old: &VtcParticipant,
    me: &str,
    effects: Effects,
) {
    let call = fetch_one::<VtcCall, _>(backend, caller, account_id, &old.call_id)
        .await
        .ok()
        .flatten()
        .filter(|c| c.state != CallState::Ended);
    if let Some(call) = &call {
        // ponytail: transition failures are dropped, as the crate has no
        // logging. Upgrade path: a `VtcBackend` hook that reports them.
        let _ = call_transitions(backend, caller, account_id, call, old, &effects).await;
    }
    if effects.answered {
        // §Answering a Ring Call: "In either case the server dispatches a
        // `VTCCallEndEvent` with `endReason: "answered_elsewhere"` to the
        // answering user's other ringing devices".
        let reason = RingEndReason::AnsweredElsewhere;
        deliver_call_end(backend, caller, account_id, &old.call_id, reason).await;
    }
    if effects.declined {
        // §VTCCallEndEvent: "When a ringing call is answered, declined,
        // cancelled, or times out, the server delivers a `VTCCallEndEvent`
        // to all devices that received the ring."
        let reason = RingEndReason::Declined;
        deliver_call_end(backend, caller, account_id, &old.call_id, reason).await;
    }
    if effects.unmute_request {
        let event: VtcUnmuteRequestEvent = build(json!({
            "accountId": account_id,
            "callId": old.call_id,
            "participantId": old.id,
            "requestedBy": me,
        }));
        backend
            .deliver_event(caller, account_id, VtcEvent::UnmuteRequest(event))
            .await;
    }
    if let Some(signal) = effects.signal {
        backend
            .deliver_event(caller, account_id, VtcEvent::GatewaySignal(signal))
            .await;
    }
}

/// The call transitions of [`run_effects`] on `call`, which has not
/// ended. A failed activation does not stop the decline and leave
/// transitions; the `Err` value is the SetError JSON of the decline or
/// leave failure, else of the activation failure.
async fn call_transitions<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
    old: &VtcParticipant,
    effects: &Effects,
) -> Result<(), Value> {
    // "if the call is `"ringing"`, the server transitions it to
    // `"active"`"; a joined `"pending"` call likewise. `effects.activate`
    // was decided from the call before the update, so the backend applies
    // it only from those states: a call already active or ended is left as
    // it is.
    let activated = if effects.activate {
        let from = [CallState::Ringing, CallState::Pending];
        set_call_state(
            backend,
            caller,
            account_id,
            &call.id,
            &from,
            CallState::Active,
        )
        .await
    } else {
        Ok(())
    };
    if effects.declined && call.state == CallState::Ringing {
        // "When all target participants have declined and the call is
        // still `"ringing"`, the server transitions the call to `"ended"`
        // with `endReason: "declined"`."
        let targets = call_participants(backend, caller, account_id, &call.id).await?;
        let mut targets = targets.iter().filter_map(|p| p.call_response.as_ref());
        if targets.all(|r| *r == CallResponse::Declined) {
            end(
                backend,
                caller,
                account_id,
                &call.id,
                CallEndReason::Declined,
            )
            .await?;
        }
    }
    if effects.left {
        after_leave(backend, caller, account_id, call, &old.role)
            .await
            .map_err(backend_set_error_value)?;
    }
    activated
}

/// Apply the call side effects of participants leaving `call`, after
/// their `leftAt` writes have committed (draft-atwood-jmap-vtc-00
/// §Leaving a Call). `VTCParticipant/set` runs this for a leave; call it
/// once per call after setting `leftAt` some other way, such as a Space
/// ban. `left_role` is `"moderator"` if any participant that left held
/// it.
///
/// - "When the last active participant leaves a ring or scheduled call,
///   the server transitions the VTCCall to `"ended"` with `endReason:
///   "completed"`, or `"cancelled"` if the call was still `"ringing"`. A
///   room call stays `"active"` when it empties". The end goes through
///   [`end_call`], which delivers the `"cancelled"` `VTCCallEndEvent`.
/// - Otherwise, when the last moderator leaves, "the server SHOULD grant
///   `"moderator"` to the remaining authenticated participant (non-null
///   `userId`) who joined earliest".
///
/// # Errors
///
/// The backend's error from reading the participants, from
/// [`end_call`], or from updating the heir's `role`.
// Same error type as `VtcBackend::update_object`, which it propagates.
#[allow(clippy::result_large_err)]
pub async fn after_leave<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
    left_role: &CallRole,
) -> Result<(), BackendSetError<B::Error>> {
    let (participants, _) = backend
        .get_objects::<VtcParticipant>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    let active: Vec<&VtcParticipant> = participants
        .iter()
        .filter(|p| p.call_id == call.id && p.joined_at.is_some() && p.left_at.is_none())
        .collect();
    if active.is_empty() {
        if matches!(call.call_type, CallType::Ring | CallType::Scheduled) {
            let reason = if call.state == CallState::Ringing {
                CallEndReason::Cancelled
            } else {
                CallEndReason::Completed
            };
            end_call(backend, caller, account_id, &call.id, reason).await?;
        }
        return Ok(());
    }
    if *left_role != CallRole::Moderator || active.iter().any(|p| p.role == CallRole::Moderator) {
        return Ok(());
    }
    let heir = active
        .iter()
        .filter(|p| p.user_id.is_some())
        .filter_map(|p| p.joined_at.as_ref().map(|t| (utc_key(t), &p.id)))
        .min();
    if let Some((_, heir)) = heir {
        let mut patch = PatchObject::new();
        patch
            .as_map_mut()
            .insert("role".to_owned(), json!(CallRole::Moderator));
        backend
            .update_object::<VtcParticipant>(caller, account_id, heir, patch)
            .await?;
    }
    Ok(())
}

/// [`end_call`] with its error as SetError JSON.
async fn end<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call_id: &Id,
    reason: CallEndReason,
) -> Result<(), Value> {
    end_call(backend, caller, account_id, call_id, reason)
        .await
        .map_err(backend_set_error_value)
}
