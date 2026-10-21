//! Call-end cascades: breakout rooms end with their parent
//! (jmap-vtc-implementer-guide.md L902-906) and active recordings stop when
//! their call ends (jmap-vtc-recording-guide.md L322-328), for a moderator's
//! `VTCCall/set` and for a server-initiated end. Line numbers cite
//! jmap-chat-spec 3227c0f; bare `L` numbers are draft-atwood-jmap-vtc-00.md.

#[macro_use]
mod common;

use common::*;
use jmap_types::Id;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    end_call, handle_vtc_call_changes, handle_vtc_call_get, handle_vtc_livestream_get,
    handle_vtc_recording_get, handle_vtc_recording_set, BackendSetError, SetErrorType,
};
use jmap_vtc_types::{CallEndReason, VtcEvent};
use serde_json::{json, Value};

/// The server's own caller context for a server-initiated end.
const SERVER: &str = "server:timer";

fn room() -> Value {
    json!({ "callType": "room", "mediaTypes": ["audio"] })
}

async fn room_under(backend: &MemoryBackend, parent: Option<&str>) -> String {
    let mut call = room();
    if let Some(parent) = parent {
        call["parentCallId"] = json!(parent);
    }
    create(backend, BOB, call).await["id"]
        .as_str()
        .expect("id")
        .to_owned()
}

async fn call_state(backend: &MemoryBackend) -> u64 {
    let resp = call!(
        handle_vtc_call_changes,
        backend,
        BOB,
        json!({ "sinceState": "0" }),
    );
    resp["newState"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .expect("MemoryBackend states count commits")
}

/// The ids `VTCCall/changes` reports as updated since `since`.
async fn updated_since(backend: &MemoryBackend, since: u64) -> Vec<String> {
    let resp = call!(
        handle_vtc_call_changes,
        backend,
        BOB,
        json!({ "sinceState": since.to_string() }),
    );
    let mut ids: Vec<String> = resp["updated"]
        .as_array()
        .expect("updated")
        .iter()
        .map(|v| v.as_str().expect("id").to_owned())
        .collect();
    ids.sort();
    ids
}

/// Start a recording on `call_id` as [`BOB`] (L1058-1060) and return its id.
async fn start_recording(backend: &MemoryBackend, call_id: &str) -> String {
    let resp = call!(
        handle_vtc_recording_set,
        backend,
        BOB,
        json!({ "create": { "r0": { "callId": call_id } } }),
    );
    resp["created"]["r0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("start recording: {resp}"))
        .to_owned()
}

async fn recording(backend: &MemoryBackend, id: &str) -> Value {
    call!(
        handle_vtc_recording_get,
        backend,
        BOB,
        json!({ "ids": [id] }),
    )["list"][0]
        .clone()
}

/// The `VTCRecordingStateEvent`s delivered after the first `skip`.
fn events_after(backend: &MemoryBackend, skip: usize) -> Vec<Value> {
    backend
        .recording_events_for_test()
        .into_iter()
        .skip(skip)
        .map(|e| serde_json::to_value(VtcEvent::RecordingState(e)).expect("event"))
        .collect()
}

fn assert_ended(call: &Value, reason: &str) {
    assert_eq!(call["state"], "ended", "{call}");
    assert!(call["endedAt"].is_string(), "endedAt must be set: {call}");
    assert_eq!(call["endReason"], reason, "{call}");
}

/// L1534-1537 `state` "The new recording state" and `initiatedBy` "The
/// userId of the participant who initiated the state change";
/// draft-atwood-jmap-vtc-wss-00 L296-303: `accountId` is "the id of the
/// call's hosting account".
fn assert_stopped_event(event: &Value, call_id: &str, recording_id: &str, by: &str) {
    assert_eq!(
        *event,
        json!({
            "@type": "VTCRecordingStateEvent",
            "accountId": ACCOUNT,
            "callId": call_id,
            "recordingId": recording_id,
            "state": "stopped",
            "initiatedBy": by
        })
    );
}

#[tokio::test]
async fn moderator_end_ends_every_breakout_room_before_the_parent() {
    let backend = backend();
    let parent = room_under(&backend, None).await;
    let first = room_under(&backend, Some(&parent)).await;
    let second = room_under(&backend, Some(&parent)).await;
    let nested = room_under(&backend, Some(&first)).await;
    let before = call_state(&backend).await;

    // L697-703: the example update.
    update(&backend, BOB, &parent, json!({ "state": "ended" }))
        .await
        .expect("end parent");

    // Guide L902-904 "When a parent call ends, all child breakout-room
    // VTCCalls MUST also be ended (transitioning them to `"ended"` and
    // setting `endedAt`)." L691 "`"completed"`: The call was in state
    // `"active"` when ended."
    for id in [&parent, &first, &second, &nested] {
        assert_ended(&get(&backend, id).await, "completed");
    }

    // Guide L906 "Implement parent-call end as a cascade that ends all
    // child calls first." The three rooms take the first three commits:
    // after them only the parent changes, and the nested room is ended
    // before the room it sits in.
    assert_eq!(
        updated_since(&backend, before + 3).await,
        vec![parent.clone()]
    );
    assert!(!updated_since(&backend, before + 1).await.contains(&nested));

    // L715-716 "Transition a child VTCCall to `"ended"`. The server removes
    // its id from the parent's `breakoutRoomIds`."
    assert_eq!(get(&backend, &parent).await["breakoutRoomIds"], json!([]));
    assert_eq!(get(&backend, &first).await["breakoutRoomIds"], json!([]));
}

#[tokio::test]
async fn moderator_end_stops_recording_and_paused_recordings() {
    let backend = backend_with(media_capability());
    let call = room_under(&backend, None).await;
    let other = room_under(&backend, None).await;
    let recording_id = start_recording(&backend, &call).await;
    let paused_id = start_recording(&backend, &call).await;
    let elsewhere_id = start_recording(&backend, &other).await;
    // L1082 "`"recording"` → `"paused"`: Pause recording."
    let resp = call!(
        handle_vtc_recording_set,
        &backend,
        BOB,
        json!({ "update": { &paused_id: { "state": "paused" } } }),
    );
    assert!(resp["notUpdated"].is_null(), "pause: {resp}");
    let seen = backend.recording_events_for_test().len();

    update(&backend, BOB, &call, json!({ "state": "ended" }))
        .await
        .expect("end");

    // Recording guide L324-326 "Any VTCRecording in `"recording"` or
    // `"paused"` state is transitioned to `"stopped"`, with `stoppedAt` set
    // and `VTCRecordingStateEvent` delivered."
    for id in [&recording_id, &paused_id] {
        let r = recording(&backend, id).await;
        assert_eq!(r["state"], "stopped", "{r}");
        assert!(r["stoppedAt"].is_string(), "stoppedAt must be set: {r}");
    }
    let mut events = events_after(&backend, seen);
    events.sort_by_key(|e| e["recordingId"].as_str().map(str::to_owned));
    let mut ids = [recording_id.as_str(), paused_id.as_str()];
    ids.sort_unstable();
    assert_eq!(events.len(), 2, "{events:?}");
    for (event, id) in events.iter().zip(ids) {
        assert_stopped_event(event, &call, id, BOB);
    }

    // L322 "all active recordings for that call": another call's
    // recording keeps running.
    assert_eq!(
        recording(&backend, &elsewhere_id).await["state"],
        "recording"
    );
}

#[tokio::test]
async fn server_end_cascades_through_rooms_and_recordings() {
    let backend = backend_with(media_capability());
    let parent = room_under(&backend, None).await;
    let child = room_under(&backend, Some(&parent)).await;
    let parent_rec = start_recording(&backend, &parent).await;
    let child_rec = start_recording(&backend, &child).await;
    let seen = backend.recording_events_for_test().len();

    // L444 "`"active"` → `"ended"`: ... a deployment-defined inactivity
    // timeout fires." L367 "`"timeout"`: A deployment-defined inactivity
    // timeout fired on an active call with no participants."
    end_call(
        &backend,
        &SERVER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(parent.as_str()),
        CallEndReason::Timeout,
    )
    .await
    .expect("auto-end");

    assert_ended(&get(&backend, &parent).await, "timeout");
    // Guide L902-904; the room was active, so L691 "completed".
    assert_ended(&get(&backend, &child).await, "completed");
    for id in [&parent_rec, &child_rec] {
        assert_eq!(recording(&backend, id).await["state"], "stopped");
    }

    // Recording guide L324-326; the room ends before its parent (guide
    // L906), so its recording stops first.
    let events = events_after(&backend, seen);
    assert_eq!(events.len(), 2, "{events:?}");
    assert_stopped_event(&events[0], &child, &child_rec, SERVER);
    assert_stopped_event(&events[1], &parent, &parent_rec, SERVER);
}

#[tokio::test]
async fn server_end_refuses_an_ended_or_unknown_call() {
    let backend = backend();
    let id = room_under(&backend, None).await;
    update(&backend, BOB, &id, json!({ "state": "ended" }))
        .await
        .expect("end");

    // L705-706 "The server MUST return `invalidProperties` naming `state`
    // if the call is already in state `"ended"`."
    let caller = SERVER.to_owned();
    let account = Id::from(ACCOUNT);
    match end_call(
        &backend,
        &caller,
        &account,
        &Id::from(id.as_str()),
        CallEndReason::Timeout,
    )
    .await
    {
        Err(BackendSetError::SetError(e)) => {
            assert_eq!(e.error_type, SetErrorType::InvalidProperties);
            assert_eq!(e.properties, Some(vec!["state".to_owned()]));
        }
        other => panic!("expected invalidProperties, got {other:?}"),
    }
    match end_call(
        &backend,
        &caller,
        &account,
        &Id::from("nope"),
        CallEndReason::Timeout,
    )
    .await
    {
        Err(BackendSetError::SetError(e)) => assert_eq!(e.error_type, SetErrorType::NotFound),
        other => panic!("expected notFound, got {other:?}"),
    }
}

/// Seed a `VTCLivestream` of `call_id` in `state`.
fn seed_livestream(backend: &MemoryBackend, id: &str, call_id: &str, state: &str) {
    let mut livestream = stored_livestream(id, state, Value::Null);
    livestream["callId"] = json!(call_id);
    seed_as(backend, "VTCLivestream", livestream);
}

async fn livestream(backend: &MemoryBackend, id: &str) -> Value {
    call!(
        handle_vtc_livestream_get,
        backend,
        BOB,
        json!({ "ids": [id] }),
    )["list"][0]
        .clone()
}

#[tokio::test]
async fn call_end_stops_starting_and_live_livestreams() {
    let backend = backend_with(media_capability());
    let parent = room_under(&backend, None).await;
    let child = room_under(&backend, Some(&parent)).await;
    let other = room_under(&backend, None).await;
    seed_livestream(&backend, "LS", &parent, "starting");
    seed_livestream(&backend, "LL", &child, "live");
    seed_livestream(&backend, "LF", &parent, "failed");
    seed_livestream(&backend, "LO", &other, "live");

    update(&backend, BOB, &parent, json!({ "state": "ended" }))
        .await
        .expect("end");

    // Spec 6bcd9d8 draft-atwood-jmap-vtc-00 L747 "When a call ends, the
    // server stops ... every VTCLivestream in state `"starting"` or
    // `"live"`, as if a moderator had set each to `"stopped"`." The child
    // room ends with its parent, so its livestream stops too. A
    // moderator's stop, L1254: "The server sets `stoppedAt` to the current
    // time."
    for id in ["LS", "LL"] {
        let l = livestream(&backend, id).await;
        assert_eq!(l["state"], "stopped", "{l}");
        assert!(l["stoppedAt"].is_string(), "stoppedAt must be set: {l}");
    }
    // Only `"starting"` or `"live"` livestreams of the ended calls stop.
    let failed = livestream(&backend, "LF").await;
    assert_eq!(failed["state"], "failed", "{failed}");
    assert_eq!(failed["stoppedAt"], Value::Null, "{failed}");
    assert_eq!(livestream(&backend, "LO").await["state"], "live");
}

/// The stored `VTCCall`, `VTCRecording` or `VTCLivestream` `id` as
/// [`ALICE`] sees it; [`BOB`] is denied the room under test.
async fn as_alice(kind: &str, backend: &MemoryBackend, id: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT, "ids": [id] });
    let resp = match kind {
        "call" => call!(handle_vtc_call_get, backend, ALICE, args),
        "recording" => call!(handle_vtc_recording_get, backend, ALICE, args),
        _ => call!(handle_vtc_livestream_get, backend, ALICE, args),
    };
    resp["list"][0].clone()
}

#[tokio::test]
async fn end_cascades_through_a_room_the_moderator_cannot_access() {
    let backend = backend_with(media_capability());
    let parent = room_under(&backend, None).await;
    let hidden = room_under(&backend, Some(&parent)).await;
    let nested = room_under(&backend, Some(&hidden)).await;
    let hidden_rec = start_recording(&backend, &hidden).await;
    seed_livestream(&backend, "LH", &hidden, "live");
    // Spec 33702a5 draft-atwood-jmap-vtc-00 L1671 "Non-participants
    // receive `notFound` for calls they cannot access."
    backend.deny_call_access(BOB, &hidden);
    assert!(get(&backend, &hidden).await.is_null(), "BOB is denied");

    update(&backend, BOB, &parent, json!({ "state": "ended" }))
        .await
        .expect("end parent");

    // Spec 33702a5 jmap-vtc-implementer-guide L911-913 "when a parent call
    // ends, its child breakout-room VTCCalls MUST also be ended". The
    // rooms were active: draft L744 "`"completed"`: The call was in state
    // `"active"` when ended."
    for id in [&parent, &hidden, &nested] {
        assert_ended(&as_alice("call", &backend, id).await, "completed");
    }
    // Draft L771 "The server removes its id from the parent's
    // `breakoutRoomIds`".
    assert_eq!(
        as_alice("call", &backend, &parent).await["breakoutRoomIds"],
        json!([])
    );
    // Draft L747 "When a call ends, the server stops every VTCRecording
    // for the call in state `"recording"` or `"paused"` and every
    // VTCLivestream in state `"starting"` or `"live"`".
    let r = as_alice("recording", &backend, &hidden_rec).await;
    assert_eq!(r["state"], "stopped", "{r}");
    let l = as_alice("livestream", &backend, "LH").await;
    assert_eq!(l["state"], "stopped", "{l}");
}
