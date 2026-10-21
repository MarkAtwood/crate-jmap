//! `VTCRecording/*` (draft-atwood-jmap-vtc-00 §VTCRecording Methods,
//! L1035-1131) and the `VTCRecordingStateEvent` consent signal (L1517-1537).
//! Line numbers cite draft-atwood-jmap-vtc-00.md at jmap-chat-spec 3227c0f.

#[macro_use]
mod common;

use common::*;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    handle_vtc_recording_get, handle_vtc_recording_query, handle_vtc_recording_set,
};
use jmap_vtc_types::VtcEvent;
use serde_json::{json, Value};

async fn set(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(handle_vtc_recording_set, backend, caller, args)
}

/// The `notCreated` / `notUpdated` / `notDestroyed` entry, or `None` when
/// the operation succeeded.
async fn create(backend: &MemoryBackend, caller: &str, obj: Value) -> Result<Value, Value> {
    let resp = set(backend, caller, json!({ "create": { "r0": obj } })).await;
    match resp["notCreated"].get("r0") {
        Some(e) => Err(e.clone()),
        None => Ok(resp["created"]["r0"].clone()),
    }
}

async fn update(
    backend: &MemoryBackend,
    caller: &str,
    id: &str,
    patch: Value,
) -> Result<Value, Value> {
    let resp = set(backend, caller, json!({ "update": { id: patch } })).await;
    match resp["notUpdated"].get(id) {
        Some(e) => Err(e.clone()),
        None => Ok(resp["updated"][id].clone()),
    }
}

async fn stored(backend: &MemoryBackend, id: &str) -> Value {
    call!(
        handle_vtc_recording_get,
        backend,
        BOB,
        json!({ "ids": [id] }),
    )["list"][0]
        .clone()
}

fn events(backend: &MemoryBackend) -> Vec<Value> {
    backend
        .recording_events_for_test()
        .iter()
        .map(|e| serde_json::to_value(VtcEvent::RecordingState(e.clone())).expect("event"))
        .collect()
}

/// L1064-1072: the example create body, with this fixture's call id.
fn start() -> Value {
    json!({ "callId": "C1" })
}

fn assert_type(err: &Value, ty: &str) {
    assert_eq!(err["type"], ty, "{err}");
}

#[tokio::test]
async fn moderator_starts_a_recording_and_every_participant_is_told() {
    let backend = media_backend();
    let created = create(&backend, BOB, start()).await.expect("start");
    let id = created["id"].as_str().expect("id");

    // L1060 "The server sets `id`, `state` (to `"recording"`), `startedAt`,
    // and `initiatedBy`." L475 `stoppedAt` "`null` while recording is
    // active or paused."
    assert_eq!(created["state"], "recording");
    assert_eq!(created["initiatedBy"], BOB);
    assert!(created["startedAt"].is_string(), "{created}");
    assert_eq!(created["stoppedAt"], Value::Null);
    assert_eq!(stored(&backend, id).await["state"], "recording");

    // L1076 "On success, the server MUST deliver a `VTCRecordingStateEvent`
    // ... with `state: "recording"` to all participants in the call."
    // Shape: the L1851-1857 example event, plus the hosting account's
    // `accountId` (draft-atwood-jmap-vtc-wss-00 L296-303).
    assert_eq!(
        events(&backend),
        [json!({
            "@type": "VTCRecordingStateEvent",
            "accountId": ACCOUNT,
            "callId": "C1",
            "recordingId": id,
            "state": "recording",
            "initiatedBy": BOB
        })]
    );
}

#[tokio::test]
async fn non_moderator_is_forbidden_everything() {
    let backend = media_backend();
    seed_as(
        &backend,
        "VTCRecording",
        stored_recording("R1", "recording", "2026-06-05T14:40:00Z"),
    );
    // L1051 "Only participants with `role: "moderator"` on the associated
    // VTCCall MAY create, update, or destroy VTCRecording objects.
    // Non-moderators MUST receive `forbidden`." Alice is no moderator of C1.
    assert_type(
        &create(&backend, ALICE, start()).await.expect_err("create"),
        "forbidden",
    );
    assert_type(
        &update(&backend, ALICE, "R1", json!({ "state": "stopped" }))
            .await
            .expect_err("update"),
        "forbidden",
    );
    let resp = set(&backend, ALICE, json!({ "destroy": ["R1"] })).await;
    // L1102 "The server MUST return `forbidden` when the caller is not a
    // moderator on the associated call."
    assert_type(&resp["notDestroyed"]["R1"], "forbidden");
    assert_eq!(stored(&backend, "R1").await["state"], "recording");
    assert!(events(&backend).is_empty());
}

#[tokio::test]
async fn e2ee_call_is_forbidden() {
    let backend = backend_with(media_capability());
    let mut c = stored_call("C1", "room", "active", "2026-06-05T14:30:00Z");
    c["e2eeEnabled"] = json!(true);
    seed(&backend, c);
    // L1074 "The server MUST return `forbidden` when the call has
    // `e2eeEnabled: true` and the recording would require server-side media
    // access." L406 "When `true`, the server MUST NOT access plaintext media
    // itself." This server has no key-holding recorder (L1555-1562).
    assert_type(
        &create(&backend, BOB, start()).await.expect_err("e2ee"),
        "forbidden",
    );
    assert!(events(&backend).is_empty());
}

#[tokio::test]
async fn unsupported_recording_is_forbidden() {
    let backend = backend(); // supportsRecording: false
    seed(
        &backend,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    // L1074 "The server MUST return `forbidden` when the account-level
    // `supportsRecording` is `false`."
    assert_type(
        &create(&backend, BOB, start()).await.expect_err("off"),
        "forbidden",
    );
}

#[tokio::test]
async fn call_not_active_is_invalid_call_id() {
    let backend = backend_with(media_capability());
    seed(
        &backend,
        stored_call("C1", "scheduled", "pending", "2026-06-05T14:30:00Z"),
    );
    // L1074 "The server MUST return `invalidProperties` naming `callId` when
    // the call is not in state `"active"`."
    let err = create(&backend, BOB, start()).await.expect_err("pending");
    assert_type(&err, "invalidProperties");
    assert_eq!(err["properties"], json!(["callId"]));
}

#[tokio::test]
async fn server_set_properties_are_refused_on_create() {
    let backend = media_backend();
    // L1060 "The server sets `id`, `state` ..., `startedAt`, and
    // `initiatedBy`."
    let err = create(
        &backend,
        BOB,
        json!({ "callId": "C1", "initiatedBy": ALICE }),
    )
    .await
    .expect_err("server-set");
    assert_type(&err, "invalidProperties");
    assert_eq!(err["properties"], json!(["initiatedBy"]));
}

#[tokio::test]
async fn pause_resume_stop_each_signal_consent() {
    let backend = media_backend();
    seed_as(
        &backend,
        "VTCRecording",
        stored_recording("R1", "recording", "2026-06-05T14:40:00Z"),
    );
    // L1082 "`"recording"` → `"paused"` ... MUST deliver a
    // `VTCRecordingStateEvent` with `state: "paused"`"; L1083 "`"paused"` →
    // `"recording"` ... with `state: "recording"`"; L1084 "→ `"stopped"`:
    // ... The server sets `stoppedAt` and delivers a
    // `VTCRecordingStateEvent` with `state: "stopped"`."
    update(&backend, BOB, "R1", json!({ "state": "paused" }))
        .await
        .expect("pause");
    assert_eq!(stored(&backend, "R1").await["stoppedAt"], Value::Null);
    update(&backend, BOB, "R1", json!({ "state": "recording" }))
        .await
        .expect("resume");
    let echo = update(&backend, BOB, "R1", json!({ "state": "stopped" }))
        .await
        .expect("stop");
    let rec = stored(&backend, "R1").await;
    assert_eq!(rec["state"], "stopped");
    assert!(rec["stoppedAt"].is_string(), "{rec}");
    // RFC 8620 §5.3 `updated`: "any property that changed in a way not
    // explicitly requested by the PatchObject".
    assert_eq!(echo["stoppedAt"], rec["stoppedAt"]);

    let states: Vec<Value> = events(&backend)
        .iter()
        .map(|e| e["state"].clone())
        .collect();
    assert_eq!(
        states,
        [json!("paused"), json!("recording"), json!("stopped")]
    );
    for e in events(&backend) {
        assert_eq!(e["recordingId"], "R1");
        assert_eq!(e["callId"], "C1");
        assert_eq!(e["initiatedBy"], BOB);
    }
}

#[tokio::test]
async fn unlisted_transitions_are_invalid_state() {
    let backend = media_backend();
    for (id, state) in [("R1", "stopped"), ("R2", "available"), ("R3", "recording")] {
        seed_as(
            &backend,
            "VTCRecording",
            stored_recording(id, state, "2026-06-05T14:40:00Z"),
        );
    }
    // L1098 "The server MUST return `invalidProperties` naming `state` for
    // any state transition not listed above (e.g., `"stopped"` →
    // `"recording"`, `"available"` → anything)." L1098 also: the post-stop
    // transitions "are server-initiated and not client-controllable".
    for (id, to) in [("R1", "recording"), ("R2", "paused"), ("R3", "processing")] {
        let err = update(&backend, BOB, id, json!({ "state": to }))
            .await
            .expect_err("transition");
        assert_type(&err, "invalidProperties");
        assert_eq!(err["properties"], json!(["state"]), "{id}");
    }
    assert!(events(&backend).is_empty());
}

#[tokio::test]
async fn moderator_destroys_a_recording() {
    let backend = media_backend();
    seed_as(
        &backend,
        "VTCRecording",
        stored_recording("R1", "available", "2026-06-05T14:40:00Z"),
    );
    // L1102 "`destroy` removes the VTCRecording metadata."
    let resp = set(&backend, BOB, json!({ "destroy": ["R1"] })).await;
    assert_eq!(resp["destroyed"], json!(["R1"]), "{resp}");
    assert_eq!(stored(&backend, "R1").await, Value::Null);
}

#[tokio::test]
async fn destroying_a_live_recording_stops_it_first() {
    let backend = media_backend();
    for (id, state) in [("R1", "recording"), ("R2", "paused"), ("R3", "available")] {
        seed_as(
            &backend,
            "VTCRecording",
            stored_recording(id, state, "2026-06-05T14:40:00Z"),
        );
    }
    // jmap-chat-spec 6bcd9d8 L1169: "If the recording is in state
    // `"recording"` or `"paused"`, the server first stops it and delivers
    // the `VTCRecordingStateEvent` with `state: "stopped"`."
    let resp = set(&backend, BOB, json!({ "destroy": ["R1", "R2", "R3"] })).await;
    assert_eq!(resp["destroyed"], json!(["R1", "R2", "R3"]), "{resp}");
    for id in ["R1", "R2", "R3"] {
        assert_eq!(stored(&backend, id).await, Value::Null, "{id}");
    }
    let got: Vec<(Value, Value)> = events(&backend)
        .iter()
        .map(|e| (e["recordingId"].clone(), e["state"].clone()))
        .collect();
    assert_eq!(
        got,
        [
            (json!("R1"), json!("stopped")),
            (json!("R2"), json!("stopped")),
        ]
    );
    for e in events(&backend) {
        assert_eq!(e["callId"], "C1");
        assert_eq!(e["initiatedBy"], BOB);
    }
}

#[tokio::test]
async fn query_defaults_to_started_at_descending() {
    let backend = media_backend();
    seed_as(
        &backend,
        "VTCRecording",
        stored_recording("R1", "stopped", "2026-06-05T14:40:00Z"),
    );
    seed_as(
        &backend,
        "VTCRecording",
        stored_recording("R2", "recording", "2026-06-05T15:00:00Z"),
    );
    // L1127 "Default sort: `startedAt` descending."
    let resp = call!(handle_vtc_recording_query, &backend, BOB, json!({}));
    assert_eq!(resp["ids"], json!(["R2", "R1"]), "{resp}");
    // L1113 `state` "Filter by recording state."
    let resp = call!(
        handle_vtc_recording_query,
        &backend,
        BOB,
        json!({ "filter": { "state": "stopped" } }),
    );
    assert_eq!(resp["ids"], json!(["R1"]), "{resp}");
}
