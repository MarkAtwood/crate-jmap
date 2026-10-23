//! VTCRecording/* and VTCLivestream/* method handlers
//! (draft-atwood-jmap-vtc-00 §VTCRecording Methods, §VTCLivestream Methods).
//!
//! `/changes` is the standard RFC 8620 §5.2 method. `/get` is RFC 8620
//! §5.1; `VTCLivestream/get` blanks `streamKey` and `streamUri` for callers
//! who are not moderators on the livestream's call (§Livestream Key
//! Exposure). `/set` enforces "Only participants with `role: "moderator"`
//! on the associated VTCCall MAY create, update, or destroy", the
//! capability, E2EE and `"active"` gates on create, and each type's state
//! machine. `VTCRecording/set` delivers the `VTCRecordingStateEvent`
//! consent signal through [`VtcBackend::deliver_recording_state_event`].
//! `/query` and `/queryChanges` supply the "Default sort: `startedAt`
//! descending" before the standard RFC 8620 §5.5 and §5.6 handling.
//!
//! # E2EE
//!
//! ponytail: these handlers have no key-holding recorder or livestream
//! bridge (§Key-Holding Media Participants), so every create on a call
//! with `e2eeEnabled: true` is `forbidden`; the draft only says the server
//! "MAY accept" such a create. A deployment with a key-holding bridge
//! registers its own `VTCRecording/set` / `VTCLivestream/set` handler.

use std::collections::HashMap;

use jmap_types::{GetObject, Id, JmapError, PatchObject, SetObject};
use jmap_vtc_types::VtcMediaComparator;
use jmap_vtc_types::{
    CallState, LivestreamState, RecordingState, VtcCall, VtcLivestream, VtcRecording,
    VtcRecordingStateEvent,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::helpers::{
    backend_set_error_value, default_sort, enforce_max_objects_in_set, extract_account_id,
    fetch_one, finalize_set_response, forbidden, invalid, not_found, reject_unsupported_sort,
    set_error_value, state_refused, SetAccumulators,
};
use crate::{BackendSetError, StatefulObject, VtcBackend};
use jmap_server::{
    now_utc_string, reject_server_set, server_fail_from_backend, server_fail_value_from_backend,
    HandlerOutput,
};

/// The stream configuration §Updating Stream Configuration lets an update
/// patch "only while the livestream is in state `"stopped"` or
/// `"failed"`".
const STREAM_CONFIG: &[&str] = &["streamUri", "streamKey", "platform"];

/// The two data types this module serves, for the shared `/set` driver.
#[derive(Clone, Copy)]
enum Kind {
    Recording,
    Livestream,
}

/// Handle `VTCRecording/get` (draft-atwood-jmap-vtc-00 §VTCRecording/get):
/// standard RFC 8620 §5.1 `/get`.
pub async fn handle_vtc_recording_get<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_get::<VtcRecording, B>(backend, caller, args).await
}

/// Handle `VTCRecording/changes` (draft-atwood-jmap-vtc-00
/// §VTCRecording/changes): standard RFC 8620 §5.2 `/changes`.
pub async fn handle_vtc_recording_changes<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<VtcRecording, B>(backend, caller, args).await
}

/// Handle `VTCRecording/set` (draft-atwood-jmap-vtc-00 §VTCRecording/set).
///
/// Create: §Starting a Recording, then [`VtcBackend::create_object`] and a
/// `VTCRecordingStateEvent` with `state: "recording"`. Update: §Pausing,
/// Resuming, and Stopping a Recording, then a `VTCRecordingStateEvent`
/// with the new state. Destroy: §Destroying a Recording. Every operation
/// requires [`VtcBackend::is_moderator`] on the recording's call.
pub async fn handle_vtc_recording_set<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_set::<VtcRecording, B>(backend, caller, args, Kind::Recording).await
}

/// Handle `VTCRecording/query` (draft-atwood-jmap-vtc-00
/// §VTCRecording/query).
///
/// Supplies the "Default sort: `startedAt` descending" and refuses any
/// other sort property with `unsupportedSort`. The backend evaluates the
/// filter; [`crate::query::recording_matches`] and
/// [`crate::query::compare_recordings`] implement it.
pub async fn handle_vtc_recording_query<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_query::<VtcRecording, B>(backend, caller, prepare_query(args)?).await
}

/// Handle `VTCRecording/queryChanges` (draft-atwood-jmap-vtc-00
/// §VTCRecording/queryChanges). RFC 8620 §5.6: the `filter` and `sort` are
/// those of the original query, so the `/query` rules apply here too.
pub async fn handle_vtc_recording_query_changes<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_query_changes::<VtcRecording, B>(backend, caller, prepare_query(args)?)
        .await
}

/// Handle `VTCLivestream/get` (draft-atwood-jmap-vtc-00
/// §VTCLivestream/get).
///
/// Standard RFC 8620 §5.1 `/get`, except that for each livestream whose
/// call the caller is not a moderator on, `streamKey` is `""` ("the server
/// SHOULD return an empty string in `VTCLivestream/get` responses for
/// participants whose role is not `"moderator"`", §VTCLivestream) and so is
/// `streamUri` ("the server SHOULD return `streamUri` as an empty string for
/// non-moderator participants", §Livestream Key Exposure). A livestream
/// whose call no longer exists is redacted.
pub async fn handle_vtc_livestream_get<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
) -> Result<HandlerOutput, JmapError> {
    // The redaction needs each item's `callId`; fetch it when the caller's
    // `properties` leaves it out, and drop it again afterwards.
    let mut strip_call_id = false;
    if let Some(Value::Array(props)) = args.get_mut("properties") {
        if !props.iter().any(|p| p == "callId") {
            props.push(json!("callId"));
            strip_call_id = true;
        }
    }

    let mut out = jmap_server::handle_get::<VtcLivestream, B>(backend, caller, args).await?;
    let account_id = out
        .primary
        .get("accountId")
        .and_then(Value::as_str)
        .map(Id::from)
        .ok_or_else(|| JmapError::server_fail("VTCLivestream/get response has no accountId"))?;

    let mut moderator_on: HashMap<String, bool> = HashMap::new();
    if let Some(Value::Array(list)) = out.primary.get_mut("list") {
        for item in list.iter_mut().filter_map(Value::as_object_mut) {
            let call_id = item
                .get("callId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let moderator = match moderator_on.get(&call_id) {
                Some(m) => *m,
                None => {
                    // Id::from: callId from this handler's own /get response.
                    let m =
                        is_moderator_on(backend, caller, &account_id, &Id::from(call_id.as_str()))
                            .await
                            .map_err(|_| JmapError::server_fail("moderator lookup failed"))?;
                    moderator_on.insert(call_id, m);
                    m
                }
            };
            if !moderator {
                for key in ["streamKey", "streamUri"] {
                    if let Some(v) = item.get_mut(key) {
                        *v = json!("");
                    }
                }
            }
            if strip_call_id {
                item.remove("callId");
            }
        }
    }
    Ok(out)
}

/// Handle `VTCLivestream/changes` (draft-atwood-jmap-vtc-00
/// §VTCLivestream/changes): standard RFC 8620 §5.2 `/changes`.
pub async fn handle_vtc_livestream_changes<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<VtcLivestream, B>(backend, caller, args).await
}

/// Handle `VTCLivestream/set` (draft-atwood-jmap-vtc-00
/// §VTCLivestream/set).
///
/// Create: §Starting a Livestream, then [`VtcBackend::create_object`].
/// Update: §Stopping a Livestream and §Updating Stream Configuration.
/// Destroy: §Destroying a Livestream. Every operation requires
/// [`VtcBackend::is_moderator`] on the livestream's call.
pub async fn handle_vtc_livestream_set<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_set::<VtcLivestream, B>(backend, caller, args, Kind::Livestream).await
}

/// Handle `VTCLivestream/query` (draft-atwood-jmap-vtc-00
/// §VTCLivestream/query).
///
/// Supplies the "Default sort: `startedAt` descending" and refuses any
/// other sort property with `unsupportedSort`. The backend evaluates the
/// filter; [`crate::query::livestream_matches`] and
/// [`crate::query::compare_livestreams`] implement it.
pub async fn handle_vtc_livestream_query<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_query::<VtcLivestream, B>(backend, caller, prepare_query(args)?).await
}

/// Handle `VTCLivestream/queryChanges` (draft-atwood-jmap-vtc-00
/// §VTCLivestream/queryChanges). RFC 8620 §5.6: the `/query` rules apply.
pub async fn handle_vtc_livestream_query_changes<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_query_changes::<VtcLivestream, B>(backend, caller, prepare_query(args)?)
        .await
}

fn prepare_query(mut args: Value) -> Result<Value, JmapError> {
    if let Value::Object(map) = &mut args {
        default_sort(
            map,
            json!([{ "property": "startedAt", "isAscending": false }]),
        );
    }
    reject_unsupported_sort::<VtcMediaComparator>(args.get("sort"))?;
    Ok(args)
}

/// The `/set` driver shared by both types: RFC 8620 §5.3 envelope, then
/// create, update and destroy in that order.
async fn handle_set<O, B>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    kind: Kind,
) -> Result<HandlerOutput, JmapError>
where
    O: SetObject + GetObject + MediaObject + Serialize + Send + Sync,
    B: VtcBackend,
{
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
        .get_state::<O>(caller, &account_id)
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
            let result = match kind {
                Kind::Recording => {
                    create_recording(backend, caller, &account_id, &create_id, obj).await
                }
                Kind::Livestream => {
                    create_livestream(backend, caller, &account_id, &create_id, obj).await
                }
            };
            match result {
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
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    acc.not_updated.insert(id_str, set_error_value(&err));
                    continue;
                }
            };
            let result = match kind {
                Kind::Recording => update_recording(backend, caller, &account_id, &id, patch).await,
                Kind::Livestream => {
                    update_livestream(backend, caller, &account_id, &id, patch).await
                }
            };
            match result {
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
            let id = match jmap_server::parse_wire_id(id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    acc.not_destroyed
                        .insert(id_str.to_owned(), set_error_value(&err));
                    continue;
                }
            };
            match destroy_media::<O, B>(backend, caller, &account_id, &id).await {
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

    finalize_set_response::<B, O>(backend, caller, &account_id, old_state, mutated, acc).await
}

/// The `callId` both media types carry, and the object itself for its
/// type's stop-before-destroy rule.
trait MediaObject {
    fn call_id(&self) -> &Id;
    fn as_recording(&self) -> Option<&VtcRecording>;
    fn as_livestream(&self) -> Option<&VtcLivestream>;
}

impl MediaObject for VtcRecording {
    fn call_id(&self) -> &Id {
        &self.call_id
    }
    fn as_recording(&self) -> Option<&VtcRecording> {
        Some(self)
    }
    fn as_livestream(&self) -> Option<&VtcLivestream> {
        None
    }
}

impl MediaObject for VtcLivestream {
    fn call_id(&self) -> &Id {
        &self.call_id
    }
    fn as_recording(&self) -> Option<&VtcRecording> {
        None
    }
    fn as_livestream(&self) -> Option<&VtcLivestream> {
        Some(self)
    }
}

/// `true` if the caller is a moderator on call `call_id`; `false` when the
/// call does not exist, since then nobody holds a role on it.
async fn is_moderator_on<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call_id: &Id,
) -> Result<bool, Value> {
    match fetch_one::<VtcCall, _>(backend, caller, account_id, call_id).await? {
        Some(call) => backend
            .is_moderator(caller, account_id, &call)
            .await
            .map_err(|e| server_fail_value_from_backend(&e)),
        None => Ok(false),
    }
}

/// Fetch object `id` and require the caller to be a moderator on its call:
/// "Non-moderators MUST receive `forbidden`."
async fn fetch_moderated<O, B>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<O, Value>
where
    O: GetObject + MediaObject + Send + Sync,
    B: VtcBackend,
{
    let obj = fetch_one::<O, _>(backend, caller, account_id, id)
        .await?
        .ok_or_else(not_found)?;
    if !is_moderator_on(backend, caller, account_id, obj.call_id()).await? {
        return Err(forbidden("the caller is not a moderator on this call"));
    }
    Ok(obj)
}

/// The checks §Starting a Recording and §Starting a Livestream share, in
/// one order: server-set properties, the capability flag, the call, the
/// moderator rule, E2EE and the `"active"` state. Returns the call.
///
/// Both sections say "The server sets `id`, `state` ...", so `state`, which
/// an update may change, is refused on create beside `O::SERVER_SET`.
async fn check_create<O: SetObject, B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    m: &Map<String, Value>,
    supported: impl Fn(&jmap_vtc_types::VtcCapability) -> bool,
) -> Result<VtcCall, Value> {
    if let Some(e) = reject_server_set::<O>(m) {
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
    let type_name = O::TYPE_NAME;

    let cap = backend
        .vtc_capability(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    if !supported(&cap) {
        return Err(forbidden(&format!(
            "{type_name} is not supported for this account"
        )));
    }

    let call_id = m
        .get("callId")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&["callId"], "callId is required"))?;
    let call = fetch_one::<VtcCall, _>(
        backend,
        caller,
        account_id,
        &Id::new_validated(call_id).map_err(|e| invalid(&["callId"], &e.to_string()))?,
    )
    .await?
    .ok_or_else(|| invalid(&["callId"], "callId does not name a call"))?;

    if !backend
        .is_moderator(caller, account_id, &call)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        return Err(forbidden("the caller is not a moderator on this call"));
    }
    if call.e2ee_enabled {
        return Err(forbidden(
            "the call is end-to-end encrypted and this server has no key-holding participant",
        ));
    }
    if call.state != CallState::Active {
        return Err(invalid(&["callId"], "the call is not in state \"active\""));
    }
    Ok(call)
}

/// Fill `m` with `defaults`, decode it as `O` and hand it to
/// [`VtcBackend::create_object`].
async fn store<O, B>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    create_id: &str,
    mut m: Map<String, Value>,
    defaults: Vec<(&str, Value)>,
) -> Result<(Id, O), Value>
where
    O: SetObject + DeserializeOwned + Send + Sync,
    B: VtcBackend,
{
    for (key, value) in defaults {
        m.insert(key.to_owned(), value);
    }
    // `id` is required by the types, so `defaults` sets it to "", which is
    // not an Id (RFC 8620 §1.2: "at least 1 and a maximum of 255 octets").
    // This parse validates the create; the builder parses again with the
    // minted id, which differs only in that string.
    serde_json::from_value::<O>(Value::Object(m.clone()))
        .map_err(|e| invalid(&[], &format!("invalid {}: {e}", O::TYPE_NAME)))?;
    let build = move |id: &Id| -> O {
        m.insert("id".to_owned(), json!(id));
        serde_json::from_value(Value::Object(m)).expect("validated above with the same fields")
    };
    backend
        .create_object(caller, account_id, create_id, build)
        .await
        .map_err(backend_set_error_value)
}

fn to_value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("derive(Serialize) on plain data is infallible")
}

/// §Starting a Recording. The `Ok` value is the `created` entry.
async fn create_recording<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    create_id: &str,
    obj: Value,
) -> Result<Value, Value> {
    let Value::Object(m) = obj else {
        return Err(invalid(&[], "a VTCRecording create must be a JSON object"));
    };
    let call =
        check_create::<VtcRecording, B>(backend, caller, account_id, &m, |c| c.supports_recording)
            .await?;
    let initiator = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;

    // "The server sets `id`, `state` (to `"recording"`), `startedAt`, and
    // `initiatedBy`."
    let (id, recording) = store::<VtcRecording, B>(
        backend,
        caller,
        account_id,
        create_id,
        m,
        vec![
            ("id", json!("")),
            ("state", json!(RecordingState::Recording)),
            ("startedAt", json!(now_utc_string())),
            ("stoppedAt", Value::Null),
            ("initiatedBy", json!(initiator)),
            ("blobId", Value::Null),
            ("size", Value::Null),
            ("duration", Value::Null),
            ("mediaType", Value::Null),
        ],
    )
    .await?;

    // vtc L1143 "On success, the server SHOULD deliver a
    // `VTCRecordingStateEvent` ...
    // with `state: "recording"` to all participants in the call."
    deliver(
        backend,
        caller,
        account_id,
        call.id,
        id,
        RecordingState::Recording,
        initiator,
    )
    .await;
    Ok(to_value(&recording))
}

/// Stop every recording of `call_id` in state `"recording"` or
/// `"paused"`, the jmap-vtc-recording-guide §Auto-stop on call end
/// cascade: "When a VTCCall transitions to `"ended"`, the server MUST stop
/// all active recordings for that call. Any VTCRecording in `"recording"`
/// or `"paused"` state is transitioned to `"stopped"`, with `stoppedAt` set
/// and `VTCRecordingStateEvent` delivered."
///
/// Runs after the call has ended, so no recording can start behind it:
/// [`VtcBackend::create_object`] refuses a recording on a call that is not
/// `"active"`. Each event names the caller as `initiatedBy`.
///
/// The recordings are read with [`VtcBackend::get_objects_unscoped`], so
/// a breakout room the caller cannot access still has its recordings
/// stopped.
///
/// ponytail: loads every VTCRecording of the account to find the call's,
/// so each call end is O(recordings). Upgrade path: an unscoped read
/// filtered by `callId`; the caller-scoped `query_objects` would skip the
/// recordings of a room the caller cannot access.
///
/// # Errors
///
/// Returns the first backend error; recordings stopped before it stay
/// stopped and their events are delivered.
// Same error type as `VtcBackend::update_object`, which it propagates.
pub(crate) async fn stop_recordings<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call_id: &Id,
) -> Result<(), BackendSetError<B::Error>> {
    let (recordings, _) = backend
        .get_objects_unscoped::<VtcRecording>(caller, account_id, None)
        .await
        .map_err(BackendSetError::Other)?;
    let live: Vec<VtcRecording> = recordings
        .into_iter()
        .filter(|r| {
            r.call_id == *call_id
                && matches!(r.state, RecordingState::Recording | RecordingState::Paused)
        })
        .collect();
    if live.is_empty() {
        return Ok(());
    }
    let initiator = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(BackendSetError::Other)?;
    for recording in &live {
        stop_recording(backend, caller, account_id, recording, initiator.clone()).await?;
    }
    Ok(())
}

/// Transition `recording` to `"stopped"`, set `stoppedAt` and deliver the
/// `VTCRecordingStateEvent` naming `initiated_by`. A recording that is no
/// longer `"recording"` or `"paused"`, stopped concurrently, is left as it
/// is and no event is delivered.
///
/// # Errors
///
/// Returns the [`VtcBackend::update_object_if`] error; no event is
/// delivered.
// Same error type as `VtcBackend::update_object`, which it propagates.
async fn stop_recording<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    recording: &VtcRecording,
    initiated_by: String,
) -> Result<(), BackendSetError<B::Error>> {
    let mut patch = PatchObject::new();
    let map = patch.as_map_mut();
    map.insert("state".to_owned(), json!(RecordingState::Stopped));
    map.insert("stoppedAt".to_owned(), json!(now_utc_string()));
    let from = [RecordingState::Recording, RecordingState::Paused];
    if backend
        .update_object_if::<VtcRecording>(caller, account_id, &recording.id, &from, patch)
        .await?
        .is_err()
    {
        return Ok(());
    }
    deliver(
        backend,
        caller,
        account_id,
        recording.call_id.clone(),
        recording.id.clone(),
        RecordingState::Stopped,
        initiated_by,
    )
    .await;
    Ok(())
}

/// Stop every livestream of `call_id` in state `"starting"` or `"live"`,
/// §Ending a Call: "When a call ends, the server stops ... every
/// VTCLivestream in state `"starting"` or `"live"`, as if a moderator had
/// set each to `"stopped"`." As for a moderator's stop, the server sets
/// `stoppedAt`; no event is delivered.
///
/// ponytail: loads every VTCLivestream of the account, so each call end is
/// O(livestreams); the upgrade path is [`stop_recordings`]'s.
///
/// # Errors
///
/// Returns the first backend error; livestreams stopped before it stay
/// stopped.
// Same error type as `VtcBackend::update_object`, which it propagates.
pub(crate) async fn stop_livestreams<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call_id: &Id,
) -> Result<(), BackendSetError<B::Error>> {
    let (livestreams, _) = backend
        .get_objects_unscoped::<VtcLivestream>(caller, account_id, None)
        .await
        .map_err(BackendSetError::Other)?;
    for livestream in livestreams.into_iter().filter(|l| {
        l.call_id == *call_id
            && matches!(l.state, LivestreamState::Starting | LivestreamState::Live)
    }) {
        stop_livestream(backend, caller, account_id, &livestream.id).await?;
    }
    Ok(())
}

/// Transition livestream `id` to `"stopped"` and set `stoppedAt`, as a
/// moderator's stop does. A livestream no longer `"starting"` or
/// `"live"` is left as it is.
///
/// # Errors
///
/// Returns the [`VtcBackend::update_object_if`] error.
// Same error type as `VtcBackend::update_object`, which it propagates.
async fn stop_livestream<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<(), BackendSetError<B::Error>> {
    let mut patch = PatchObject::new();
    let map = patch.as_map_mut();
    map.insert("state".to_owned(), json!(LivestreamState::Stopped));
    map.insert("stoppedAt".to_owned(), json!(now_utc_string()));
    let from = [LivestreamState::Starting, LivestreamState::Live];
    // A stream that already stopped or failed is left as it is.
    let _ = backend
        .update_object_if::<VtcLivestream>(caller, account_id, id, &from, patch)
        .await?;
    Ok(())
}

/// Build and deliver one `VTCRecordingStateEvent` for a recording. Runs
/// after the change has committed, so it cannot fail the operation.
async fn deliver<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call_id: Id,
    recording_id: Id,
    state: RecordingState,
    initiated_by: String,
) {
    // `VtcRecordingStateEvent` is `#[non_exhaustive]`; every field below is
    // one of its typed fields, so the decode cannot fail.
    let event: VtcRecordingStateEvent = serde_json::from_value(json!({
        "accountId": account_id,
        "callId": call_id,
        "recordingId": recording_id,
        "state": state,
        "initiatedBy": initiated_by,
    }))
    .expect("every VTCRecordingStateEvent field is supplied with its type");
    backend
        .deliver_recording_state_event(caller, account_id, event)
        .await;
}

/// §Starting a Livestream. The `Ok` value is the `created` entry.
async fn create_livestream<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    create_id: &str,
    obj: Value,
) -> Result<Value, Value> {
    let Value::Object(m) = obj else {
        return Err(invalid(&[], "a VTCLivestream create must be a JSON object"));
    };
    check_create::<VtcLivestream, B>(backend, caller, account_id, &m, |c| c.supports_livestream)
        .await?;

    // "`streamUri` (String, required)", "`streamKey` (String, required)";
    // "Optional: `platform` (String)".
    let mut bad: Vec<&str> = ["streamUri", "streamKey"]
        .into_iter()
        .filter(|k| !m.get(*k).is_some_and(Value::is_string))
        .collect();
    if m.get("platform")
        .is_some_and(|p| !p.is_string() && !p.is_null())
    {
        bad.push("platform");
    }
    if !bad.is_empty() {
        return Err(invalid(
            &bad,
            "streamUri and streamKey must be strings; platform a string or null",
        ));
    }
    let platform = m.get("platform").cloned().unwrap_or(Value::Null);

    // "The server sets `id` and `state` (to `"starting"`)"; `startedAt` is
    // "`null` before the stream is live".
    let (_, livestream) = store::<VtcLivestream, B>(
        backend,
        caller,
        account_id,
        create_id,
        m,
        vec![
            ("id", json!("")),
            ("state", json!(LivestreamState::Starting)),
            ("platform", platform),
            ("startedAt", Value::Null),
            ("stoppedAt", Value::Null),
        ],
    )
    .await?;
    Ok(to_value(&livestream))
}

/// Decode one `update` patch and collect its distinct top-level keys.
fn parse_patch(patch: Value) -> Result<(PatchObject, Vec<String>), Value> {
    let patch: PatchObject = serde_json::from_value(patch).map_err(|e| {
        crate::helpers::set_error_value(
            &jmap_server::SetError::new(jmap_server::SetErrorType::InvalidPatch)
                .with_description(e.to_string()),
        )
    })?;
    let mut keys: Vec<String> = patch
        .as_map()
        .keys()
        .map(|k| k.split('/').next().unwrap_or(k).to_owned())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    Ok((patch, keys))
}

/// Requested `state` of a patch, decoded as `S`.
fn requested_state<S: DeserializeOwned>(patch: &PatchObject) -> Option<Result<S, ()>> {
    patch
        .as_map()
        .get("state")
        .map(|v| S::deserialize(v).map_err(|_| ()))
}

/// Apply `patch` (plus `echo`, the server-set changes) and return the
/// `updated` echo. With `from`, the patch applies only while the stored
/// `state` is one of `from` ([`VtcBackend::update_object_if`]); otherwise
/// the `invalidProperties` SetError naming `state` is returned.
async fn apply<O, B>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    from: Option<&[O::State]>,
    mut patch: PatchObject,
    echo: Map<String, Value>,
) -> Result<Value, Value>
where
    O: StatefulObject<Patch = PatchObject> + Serialize + Send + Sync,
    B: VtcBackend,
{
    patch
        .as_map_mut()
        .extend(echo.iter().map(|(k, v)| (k.clone(), v.clone())));
    let applied = match from {
        Some(from) => backend
            .update_object_if::<O>(caller, account_id, id, from, patch)
            .await
            .map_err(backend_set_error_value)?
            .map_err(|_| set_error_value(&state_refused()))?,
        None => backend
            .update_object::<O>(caller, account_id, id, patch)
            .await
            .map_err(backend_set_error_value)?,
    };
    match applied {
        Some(obj) if echo.is_empty() => Ok(to_value(&obj)),
        _ if echo.is_empty() => Ok(Value::Null),
        _ => Ok(Value::Object(echo)),
    }
}

/// §Pausing, Resuming, and Stopping a Recording. `Ok` is the `updated`
/// echo.
async fn update_recording<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    patch: Value,
) -> Result<Value, Value> {
    let (patch, keys) = parse_patch(patch)?;
    let recording: VtcRecording = fetch_moderated(backend, caller, account_id, id).await?;

    // "`update` supports patching `state`".
    let fixed: Vec<&str> = keys
        .iter()
        .map(String::as_str)
        .filter(|k| *k != "state")
        .collect();
    if !fixed.is_empty() {
        return Err(invalid(&fixed, "only state can be updated"));
    }
    let Some(requested) = requested_state::<RecordingState>(&patch) else {
        return apply::<VtcRecording, B>(backend, caller, account_id, id, None, patch, Map::new())
            .await;
    };

    // "The server MUST return `invalidProperties` naming `state` for any
    // state transition not listed above". `from` is the states the
    // transition to `to` is listed from; the backend checks it atomically
    // with the write, so a concurrent change cannot slip an unlisted
    // transition such as `"stopped"` -> `"paused"` through.
    use RecordingState::{Paused, Recording, Stopped};
    let (to, from): (_, &[RecordingState]) = match requested {
        Ok(Paused) => (Paused, &[Recording]),
        Ok(Recording) => (Recording, &[Paused]),
        Ok(Stopped) => (Stopped, &[Recording, Paused]),
        _ => {
            return Err(invalid(
                &["state"],
                "this recording state transition is not permitted",
            ))
        }
    };
    let mut echo = Map::new();
    if to == Stopped {
        // "The server sets `stoppedAt`".
        echo.insert("stoppedAt".to_owned(), json!(now_utc_string()));
    }
    let updated =
        apply::<VtcRecording, B>(backend, caller, account_id, id, Some(from), patch, echo).await?;

    let initiator = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    deliver(
        backend,
        caller,
        account_id,
        recording.call_id,
        recording.id,
        to,
        initiator,
    )
    .await;
    Ok(updated)
}

/// §Stopping a Livestream and §Updating Stream Configuration. `Ok` is the
/// `updated` echo.
async fn update_livestream<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    patch: Value,
) -> Result<Value, Value> {
    let (patch, keys) = parse_patch(patch)?;
    let livestream: VtcLivestream = fetch_moderated(backend, caller, account_id, id).await?;

    let fixed: Vec<&str> = keys
        .iter()
        .map(String::as_str)
        .filter(|k| *k != "state" && !STREAM_CONFIG.contains(k))
        .collect();
    if !fixed.is_empty() {
        return Err(invalid(
            &fixed,
            "only state, streamUri, streamKey and platform can be updated",
        ));
    }

    let streaming = matches!(
        livestream.state,
        LivestreamState::Starting | LivestreamState::Live
    );
    // "The server MUST return `invalidProperties` naming each patched
    // configuration property for configuration updates while the stream is
    // in state `"starting"` or `"live"`."
    let config: Vec<&str> = keys
        .iter()
        .map(String::as_str)
        .filter(|k| STREAM_CONFIG.contains(k))
        .collect();
    if streaming && !config.is_empty() {
        return Err(invalid(
            &config,
            "stream configuration can change only while stopped or failed",
        ));
    }

    let mut echo = Map::new();
    let mut from = None;
    if let Some(requested) = requested_state::<LivestreamState>(&patch) {
        // "`update` with `state: "stopped"` ends the stream"; "The server
        // MUST return `invalidProperties` naming `state` if the livestream
        // is not in state `"starting"` or `"live"`." The other transitions
        // are server-initiated. The backend checks the state atomically
        // with the write.
        if !(streaming && requested == Ok(LivestreamState::Stopped)) {
            return Err(invalid(
                &["state"],
                "a livestream can only be stopped, and only while starting or live",
            ));
        }
        // "The server sets `stoppedAt` to the current time."
        echo.insert("stoppedAt".to_owned(), json!(now_utc_string()));
        from = Some([LivestreamState::Starting, LivestreamState::Live].as_slice());
    }
    // A configuration-only patch needs no compare-and-set: a client can
    // only stop a stream, so a stream read as `"stopped"` or `"failed"`
    // cannot become `"starting"` or `"live"` again before the write.
    apply::<VtcLivestream, B>(backend, caller, account_id, id, from, patch, echo).await
}

/// §Destroying a Recording and §Destroying a Livestream: "The server MUST
/// return `forbidden` when the caller is not a moderator". §Destroying a
/// Recording: "If the recording is in state `"recording"` or `"paused"`,
/// the server first stops it and delivers the `VTCRecordingStateEvent`
/// with `state: "stopped"`." §Destroying a Livestream: "If the livestream
/// is in state `"starting"` or `"live"`, the server first stops it, so that
/// no orphaned stream keeps running after the metadata is removed."
async fn destroy_media<O, B>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<(), Value>
where
    O: SetObject + GetObject + MediaObject + Send + Sync,
    B: VtcBackend,
{
    let obj: O = fetch_moderated(backend, caller, account_id, id).await?;
    if let Some(recording) = obj
        .as_recording()
        .filter(|r| matches!(r.state, RecordingState::Recording | RecordingState::Paused))
    {
        let initiator = backend
            .caller_user_id(caller, account_id)
            .await
            .map_err(|e| server_fail_value_from_backend(&e))?;
        stop_recording(backend, caller, account_id, recording, initiator)
            .await
            .map_err(backend_set_error_value)?;
    }
    if obj
        .as_livestream()
        .is_some_and(|l| matches!(l.state, LivestreamState::Starting | LivestreamState::Live))
    {
        stop_livestream(backend, caller, account_id, id)
            .await
            .map_err(backend_set_error_value)?;
    }
    backend
        .destroy_object::<O>(caller, account_id, id)
        .await
        .map_err(backend_set_error_value)
}
