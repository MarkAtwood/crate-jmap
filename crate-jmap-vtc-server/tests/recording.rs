//! `VTCRecording/*` (draft-atwood-jmap-vtc-00 §VTCRecording Methods,
//! L1242-1338) and the `VTCRecordingStateEvent` consent signal (L1789-1809).
//! Line numbers cite draft-atwood-jmap-vtc-00.md at jmap-chat-spec 77c3474.

#[macro_use]
mod common;

use common::*;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    handle_vtc_recording_get, handle_vtc_recording_query, handle_vtc_recording_set,
};
use jmap_vtc_types::VtcEvent;
use serde_json::{Value, json};

async fn set(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(ids handle_vtc_recording_set, backend, caller, args)
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

/// L1271-1279: the example create body, with this fixture's call id.
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

    // L1267 "The server sets `id`, `state` (to `"recording"`), `startedAt`,
    // and `initiatedBy`." L548 `stoppedAt` "`null` while recording is
    // active or paused."
    assert_eq!(created["state"], "recording");
    assert_eq!(created["initiatedBy"], BOB);
    assert!(created["startedAt"].is_string(), "{created}");
    assert_eq!(created["stoppedAt"], Value::Null);
    assert_eq!(stored(&backend, id).await["state"], "recording");

    // vtc L1283 "On success, the server SHOULD deliver a
    // `VTCRecordingStateEvent`
    // ... with `state: "recording"` to all participants in the call."
    // Shape: the L2163-2169 example event, plus the hosting account's
    // `accountId` (draft-atwood-jmap-vtc-wss-00 L294-301).
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
    // jmap-chat-spec 77c3474 L1258 "Only participants with
    // `role: "moderator"` on the associated VTCCall MAY create, update, or
    // destroy VTCRecording objects. Non-moderators SHOULD receive
    // `forbidden`." This crate enforces it by choice. Alice is no
    // moderator of C1.
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
    // L1258 "Non-moderators SHOULD receive `forbidden`." This crate
    // enforces it by choice.
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
    // L1281 "When the call has `e2eeEnabled: true`, a recording that would
    // require server-side media access is rejected as
    // {{e2ee-media-participants}} describes." L1834 "The server SHOULD
    // reject with `forbidden` a recording or livestream request that can
    // only be met by such a path." This crate enforces it by choice. This
    // server has no key-holding recorder (L1825-1836).
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
    // L1281 "The server MUST return `forbidden` when the account-level
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
    // L1281 "The server MUST return `invalidProperties` naming `callId` when
    // the call is not in state `"active"`."
    let err = create(&backend, BOB, start()).await.expect_err("pending");
    assert_type(&err, "invalidProperties");
    assert_eq!(err["properties"], json!(["callId"]));
}

#[tokio::test]
async fn server_set_properties_are_refused_on_create() {
    let backend = media_backend();
    // L1267 "The server sets `id`, `state` ..., `startedAt`, and
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
    // vtc L1289 "`"recording"` → `"paused"` ... The server delivers a
    // `VTCRecordingStateEvent` with `state: "paused"`"; L1290 "`"paused"` →
    // `"recording"` ... with `state: "recording"`"; L1291 "→ `"stopped"`:
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

/// jmap-chat-spec 125749a §Pausing, Resuming, and Stopping a Recording:
/// "The server MUST return `invalidProperties` naming `state` ... for
/// `"recording"` → `"paused"` when the account-level
/// `supportsRecordingPause` is `false`."
#[tokio::test]
async fn pause_is_invalid_state_without_supports_recording_pause() {
    let mut cap = media_capability();
    cap["supportsRecordingPause"] = json!(false);
    let backend = backend_with(cap);
    seed(
        &backend,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    seed_as(
        &backend,
        "VTCRecording",
        stored_recording("R1", "recording", "2026-06-05T14:40:00Z"),
    );
    let err = update(&backend, BOB, "R1", json!({ "state": "paused" }))
        .await
        .expect_err("pause");
    assert_type(&err, "invalidProperties");
    assert_eq!(err["properties"], json!(["state"]));
    assert_eq!(stored(&backend, "R1").await["state"], "recording");
    assert!(events(&backend).is_empty());
    assert!(backend.media_effects_for_test().is_empty());
    // Stopping is unaffected.
    update(&backend, BOB, "R1", json!({ "state": "stopped" }))
        .await
        .expect("stop");
}

/// jmap-chat-spec 125749a §Account-Level Capability Object:
/// `supportsRecordingPause` "Default: `true`". The common capability
/// leaves it out, and a pause succeeds (as in
/// `pause_resume_stop_each_signal_consent`); here it is explicitly `true`.
#[tokio::test]
async fn pause_is_allowed_when_supports_recording_pause_is_true() {
    let mut cap = media_capability();
    cap["supportsRecordingPause"] = json!(true);
    let backend = backend_with(cap);
    seed(
        &backend,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    seed_as(
        &backend,
        "VTCRecording",
        stored_recording("R1", "recording", "2026-06-05T14:40:00Z"),
    );
    update(&backend, BOB, "R1", json!({ "state": "paused" }))
        .await
        .expect("pause");
    assert_eq!(stored(&backend, "R1").await["state"], "paused");
}

/// jmap-chat-spec 125749a §Starting a Recording: "`layout` (String|null,
/// optional): The composition layout"; §VTCRecording `layout` "Default:
/// `null`"; §Composition Layouts: "The server MUST return
/// `invalidProperties` naming `layout` when a create or update sets a
/// value not listed in the account-level `layouts`." The fixture lists
/// `grid` and `speaker`.
#[tokio::test]
async fn create_layout_defaults_to_null_and_must_be_listed() {
    let backend = media_backend();
    let created = create(&backend, BOB, start()).await.expect("no layout");
    assert_eq!(created["layout"], Value::Null);

    let created = create(
        &backend,
        BOB,
        json!({ "callId": "C1", "layout": "speaker" }),
    )
    .await
    .expect("listed layout");
    assert_eq!(created["layout"], "speaker");
    let id = created["id"].as_str().expect("id");
    assert_eq!(stored(&backend, id).await["layout"], "speaker");

    for bad in [
        json!("single-speaker"),
        json!("com.example.stage"),
        json!(3),
    ] {
        let err = create(&backend, BOB, json!({ "callId": "C1", "layout": bad }))
            .await
            .expect_err("unlisted layout");
        assert_type(&err, "invalidProperties");
        assert_eq!(err["properties"], json!(["layout"]), "{bad}");
    }
}

/// jmap-chat-spec 125749a §Pausing, Resuming, and Stopping a Recording:
/// "`update` may also set `layout` while the recording is `"recording"` or
/// `"paused"`; the new layout applies from then on. The server MUST return
/// `invalidProperties` naming `layout` for a change in any other state."
#[tokio::test]
async fn layout_updates_only_while_recording_or_paused() {
    let backend = media_backend();
    for (id, state) in [
        ("R1", "recording"),
        ("R2", "paused"),
        ("R3", "stopped"),
        ("R4", "available"),
    ] {
        seed_as(
            &backend,
            "VTCRecording",
            stored_recording(id, state, "2026-06-05T14:40:00Z"),
        );
    }
    for id in ["R1", "R2"] {
        update(&backend, BOB, id, json!({ "layout": "grid" }))
            .await
            .expect("layout change");
        assert_eq!(stored(&backend, id).await["layout"], "grid");
    }
    for id in ["R3", "R4"] {
        let err = update(&backend, BOB, id, json!({ "layout": "grid" }))
            .await
            .expect_err("layout change after stop");
        assert_type(&err, "invalidProperties");
        assert_eq!(err["properties"], json!(["layout"]), "{id}");
        assert_eq!(stored(&backend, id).await["layout"], Value::Null);
    }
    // §Composition Layouts: an unlisted value is refused in any state.
    let err = update(&backend, BOB, "R1", json!({ "layout": "single-speaker" }))
        .await
        .expect_err("unlisted layout");
    assert_eq!(err["properties"], json!(["layout"]));
    // `null` returns to the server's default.
    update(&backend, BOB, "R1", json!({ "layout": null }))
        .await
        .expect("back to default");
    assert_eq!(stored(&backend, "R1").await["layout"], Value::Null);
    // A layout change is no state change: no consent event.
    assert!(events(&backend).is_empty());
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
    // L1305 "The server MUST return `invalidProperties` naming `state` for
    // any state transition not listed above (e.g., `"stopped"` →
    // `"recording"`, `"available"` → anything)." L1305 also: the post-stop
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
    // L1309 "`destroy` removes the VTCRecording metadata."
    let resp = set(&backend, BOB, json!({ "destroy": ["R1"] })).await;
    assert_eq!(resp["destroyed"], json!(["R1"]), "{resp}");
    assert_eq!(stored(&backend, "R1").await, Value::Null);
}

#[tokio::test]
async fn destroy_leaves_a_shared_blob_referenced() {
    // jmap-vtc-recording-guide.md L444-448 (jmap-chat-spec 120db2c): "The
    // blob referenced by `blobId` may still be referenced by other objects
    // (for example, a FileNode object linking to the same blob). The
    // server MUST NOT delete the blob if other references exist (RFC 8620
    // blob lifecycle)."
    //
    // This crate owns no blob store (the deployment's does), so the
    // observable other reference here is a second recording carrying the
    // same blobId: destroying R1 removes only R1's metadata, and R2 still
    // reads back with the blob, its size and its media type.
    let backend = media_backend();
    for id in ["R1", "R2"] {
        let mut r = stored_recording(id, "available", "2026-06-05T14:40:00Z");
        r["stoppedAt"] = json!("2026-06-05T15:10:00Z");
        r["blobId"] = json!("Bshared");
        r["size"] = json!(52_428_800);
        r["duration"] = json!(1800);
        r["mediaType"] = json!("video/webm");
        seed_as(&backend, "VTCRecording", r);
    }
    let resp = set(&backend, BOB, json!({ "destroy": ["R1"] })).await;
    assert_eq!(resp["destroyed"], json!(["R1"]), "{resp}");
    assert_eq!(stored(&backend, "R1").await, Value::Null);
    let other = stored(&backend, "R2").await;
    assert_eq!(other["blobId"], "Bshared", "{other}");
    assert_eq!(other["size"], 52_428_800, "{other}");
    assert_eq!(other["mediaType"], "video/webm", "{other}");
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
    // L1309: "If the recording is in state
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
    // L1334 "Default sort: `startedAt` descending."
    let resp = call!(handle_vtc_recording_query, &backend, BOB, json!({}));
    assert_eq!(resp["ids"], json!(["R2", "R1"]), "{resp}");
    // L1321 `state` "Filter by recording state."
    let resp = call!(
        handle_vtc_recording_query,
        &backend,
        BOB,
        json!({ "filter": { "state": "stopped" } }),
    );
    assert_eq!(resp["ids"], json!(["R1"]), "{resp}");
}

#[tokio::test]
async fn query_other_sort_property_is_unsupported() {
    let backend = media_backend();
    // L1334 "Sort properties: `startedAt`."; RFC 8620 §5.5
    // "unsupportedSort: The sort is syntactically valid, but includes a
    // property the server does not support sorting on".
    let Err(err) = handle_vtc_recording_query(
        &backend,
        &jmap_types::Id::from(BOB),
        json!({ "accountId": ACCOUNT, "sort": [{ "property": "state" }] }),
    )
    .await
    else {
        panic!("expected unsupportedSort");
    };
    assert_eq!(err.error_type.to_string(), "unsupportedSort");
}
