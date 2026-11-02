//! Server-set values and empty patches in `VTCCall/set`,
//! `VTCParticipant/set` (bd:JMAP-t8ej.25.2), `VTCRecording/set` and
//! `VTCLivestream/set` (bd:JMAP-t8ej.27.3) updates.
//!
//! Oracle, RFC 8620 §5.3 (update): "Any server-set properties MAY be
//! included in the patch if their value is identical to the current server
//! value (before applying the patches to the object).  Otherwise, the
//! update MUST be rejected with an "invalidProperties" SetError."
//!
//! An empty patch `{}`, literal or left empty once the identical
//! server-set values are dropped, succeeds as a no-op: `updated` maps the
//! id to null, nothing is written, and the state is unchanged (decision
//! recorded on JMAP-t8ej.25).

#[macro_use]
mod common;

use common::*;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    handle_vtc_livestream_get, handle_vtc_livestream_set, handle_vtc_participant_get,
    handle_vtc_participant_set, handle_vtc_recording_get, handle_vtc_recording_set,
};
use serde_json::{Value, json};

/// Assert `resp` reports `id` as a no-op update: `updated[id]` is null,
/// nothing failed, and the state did not move.
fn assert_noop(resp: &Value, id: &str) {
    assert!(resp["notUpdated"].is_null(), "{resp}");
    let updated = resp["updated"]
        .as_object()
        .unwrap_or_else(|| panic!("{resp}"));
    assert_eq!(updated.get(id), Some(&Value::Null), "{resp}");
    assert_eq!(resp["oldState"], resp["newState"], "{resp}");
}

/// `obj` restricted to `keys`.
fn pick(obj: &Value, keys: &[&str]) -> Value {
    keys.iter()
        .map(|k| ((*k).to_owned(), obj[*k].clone()))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

async fn room_call(backend: &MemoryBackend) -> String {
    create(
        backend,
        BOB,
        json!({ "callType": "room", "mediaTypes": ["audio"] }),
    )
    .await["id"]
        .as_str()
        .expect("id")
        .to_owned()
}

/// Run `VTCParticipant/set` as [`BOB`] with `args` (without `accountId`).
async fn pset(backend: &MemoryBackend, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    handle_vtc_participant_set(
        backend,
        &BOB.to_owned(),
        &std::collections::BTreeMap::new(),
        args,
    )
    .await
    .expect("VTCParticipant/set")
    .primary
}

/// [`BOB`]'s view of VTCParticipant `id`, as `/get` returns it.
async fn pget(backend: &MemoryBackend, id: &str) -> Value {
    handle_vtc_participant_get(
        backend,
        &BOB.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [id] }),
    )
    .await
    .expect("VTCParticipant/get")
    .primary["list"][0]
        .clone()
}

/// [`BOB`] joins a new room call; returns his VTCParticipant as `/get`
/// returns it.
async fn joined(backend: &MemoryBackend) -> Value {
    let call_id = room_call(backend).await;
    let resp = pset(
        backend,
        json!({ "create": { "p0": { "callId": call_id, "joinMethod": "webrtc" } } }),
    )
    .await;
    let id = resp["created"]["p0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("join failed: {resp}"));
    pget(backend, id).await
}

/// The VTCCall server-set and immutable properties a room call has.
const CALL_READ_ONLY: &[&str] = &[
    "id",
    "createdAt",
    "endedAt",
    "endReason",
    "initiatorId",
    "callType",
];

/// The VTCParticipant server-set and immutable properties.
const PARTICIPANT_READ_ONLY: &[&str] = &[
    "id",
    "userId",
    "joinedAt",
    "speakerTimeMs",
    "e2eeFingerprint",
    "callId",
];

/// §5.3: server-set and immutable VTCCall values sent as `/get` returns
/// them are allowed; with nothing else, the update is a no-op.
#[tokio::test]
async fn call_identical_server_set_values_are_a_noop() {
    let backend = backend();
    let id = room_call(&backend).await;
    let before = get(&backend, &id).await;
    let patch = pick(&before, CALL_READ_ONLY);
    let resp = set(&backend, BOB, json!({ "update": { &id: patch } })).await;
    assert_noop(&resp, &id);
    assert_eq!(get(&backend, &id).await, before);
}

/// Decision on JMAP-t8ej.25: a literal `{}` is a no-op success.
#[tokio::test]
async fn call_empty_patch_is_a_noop() {
    let backend = backend();
    let id = room_call(&backend).await;
    let before = get(&backend, &id).await;
    let resp = set(&backend, BOB, json!({ "update": { &id: {} } })).await;
    assert_noop(&resp, &id);
    assert_eq!(get(&backend, &id).await, before);
}

/// §5.3 "Otherwise, the update MUST be rejected with an
/// "invalidProperties" SetError."
#[tokio::test]
async fn call_changed_server_set_value_is_invalid_properties() {
    let backend = backend();
    let id = room_call(&backend).await;
    let err = update(
        &backend,
        BOB,
        &id,
        json!({ "createdAt": "2000-01-01T00:00:00Z" }),
    )
    .await
    .expect_err("a changed createdAt must be rejected");
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["createdAt"]), "{err}");
}

/// An empty patch of an unknown id is `notFound`, not a no-op.
#[tokio::test]
async fn call_empty_patch_of_unknown_id_is_not_found() {
    let backend = backend();
    let err = update(&backend, BOB, "nosuchcall", json!({}))
        .await
        .expect_err("unknown id");
    assert_eq!(err["type"], "notFound", "{err}");
}

/// §5.3: server-set and immutable VTCParticipant values sent as `/get`
/// returns them are allowed; with nothing else, the update is a no-op.
#[tokio::test]
async fn participant_identical_server_set_values_are_a_noop() {
    let backend = backend();
    let before = joined(&backend).await;
    let id = before["id"].as_str().expect("id").to_owned();
    let patch = pick(&before, PARTICIPANT_READ_ONLY);
    let resp = pset(&backend, json!({ "update": { &id: patch } })).await;
    assert_noop(&resp, &id);
    assert_eq!(pget(&backend, &id).await, before);
}

/// Decision on JMAP-t8ej.25: a literal `{}` is a no-op success.
#[tokio::test]
async fn participant_empty_patch_is_a_noop() {
    let backend = backend();
    let before = joined(&backend).await;
    let id = before["id"].as_str().expect("id").to_owned();
    let resp = pset(&backend, json!({ "update": { &id: {} } })).await;
    assert_noop(&resp, &id);
    assert_eq!(pget(&backend, &id).await, before);
}

/// §5.3 "Otherwise, the update MUST be rejected with an
/// "invalidProperties" SetError."
#[tokio::test]
async fn participant_changed_server_set_value_is_invalid_properties() {
    let backend = backend();
    let before = joined(&backend).await;
    let id = before["id"].as_str().expect("id").to_owned();
    let resp = pset(
        &backend,
        json!({ "update": { &id: { "speakerTimeMs": 12345 } } }),
    )
    .await;
    let err = &resp["notUpdated"][&id];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(err["properties"], json!(["speakerTimeMs"]), "{resp}");
}

/// The VTCRecording server-set and immutable properties,
/// draft-atwood-jmap-vtc-00 L535-563 (jmap-chat-spec 77c3474).
const RECORDING_READ_ONLY: &[&str] = &[
    "id",
    "callId",
    "startedAt",
    "stoppedAt",
    "initiatedBy",
    "blobId",
    "size",
    "duration",
    "mediaType",
];

/// The VTCLivestream server-set and immutable properties this crate
/// stores, draft-atwood-jmap-vtc-00 L572-609 (jmap-chat-spec 77c3474).
const LIVESTREAM_READ_ONLY: &[&str] = &["id", "callId", "startedAt", "stoppedAt"];

/// Run `VTCRecording/set` as `caller` with `args`.
async fn rset(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(ids handle_vtc_recording_set, backend, caller, args)
}

/// VTCRecording `id` as `/get` returns it to [`BOB`].
async fn rget(backend: &MemoryBackend, id: &str) -> Value {
    call!(
        handle_vtc_recording_get,
        backend,
        BOB,
        json!({ "ids": [id] })
    )["list"][0]
        .clone()
}

/// Run `VTCLivestream/set` as [`BOB`] with `args`.
async fn lset(backend: &MemoryBackend, args: Value) -> Value {
    call!(ids handle_vtc_livestream_set, backend, BOB, args)
}

/// VTCLivestream `id` as `/get` returns it to [`BOB`], a moderator.
async fn lget(backend: &MemoryBackend, id: &str) -> Value {
    call!(
        handle_vtc_livestream_get,
        backend,
        BOB,
        json!({ "ids": [id] })
    )["list"][0]
        .clone()
}

/// A `media_backend` with recording R1 in state `"recording"`.
fn recording_backend() -> MemoryBackend {
    let backend = media_backend();
    seed_as(
        &backend,
        "VTCRecording",
        stored_recording("R1", "recording", "2026-06-05T14:40:00Z"),
    );
    backend
}

/// A `media_backend` with livestream L1 in state `"live"`.
fn livestream_backend() -> MemoryBackend {
    let backend = media_backend();
    seed_as(
        &backend,
        "VTCLivestream",
        stored_livestream("L1", "live", json!("2026-06-05T14:41:00Z")),
    );
    backend
}

/// §5.3: server-set and immutable VTCRecording values sent as `/get`
/// returns them are allowed; with nothing else, the update is a no-op, so
/// no `VTCRecordingStateEvent` is delivered either.
#[tokio::test]
async fn recording_identical_server_set_values_are_a_noop() {
    let backend = recording_backend();
    let before = rget(&backend, "R1").await;
    let patch = pick(&before, RECORDING_READ_ONLY);
    let resp = rset(&backend, BOB, json!({ "update": { "R1": patch } })).await;
    assert_noop(&resp, "R1");
    assert_eq!(rget(&backend, "R1").await, before);
    assert!(backend.recording_events_for_test().is_empty());
}

/// Decision on JMAP-t8ej.25: a literal `{}` is a no-op success.
#[tokio::test]
async fn recording_empty_patch_is_a_noop() {
    let backend = recording_backend();
    let before = rget(&backend, "R1").await;
    let resp = rset(&backend, BOB, json!({ "update": { "R1": {} } })).await;
    assert_noop(&resp, "R1");
    assert_eq!(rget(&backend, "R1").await, before);
}

/// §5.3: identical server-set values do not block the `state` change
/// beside them (draft-atwood-jmap-vtc-00 L1289 `"recording"` ->
/// `"paused"`).
#[tokio::test]
async fn recording_identical_values_beside_a_state_change_apply() {
    let backend = recording_backend();
    let mut patch = pick(&rget(&backend, "R1").await, RECORDING_READ_ONLY);
    patch["state"] = json!("paused");
    let resp = rset(&backend, BOB, json!({ "update": { "R1": patch } })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(rget(&backend, "R1").await["state"], "paused");
}

/// §5.3 "Otherwise, the update MUST be rejected with an
/// "invalidProperties" SetError."
#[tokio::test]
async fn recording_changed_server_set_value_is_invalid_properties() {
    let backend = recording_backend();
    let resp = rset(
        &backend,
        BOB,
        json!({ "update": { "R1": { "startedAt": "2000-01-01T00:00:00Z" } } }),
    )
    .await;
    let err = &resp["notUpdated"]["R1"];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(err["properties"], json!(["startedAt"]), "{resp}");
}

/// L1258 "Non-moderators SHOULD receive `forbidden`", checked before the
/// no-op exit: ALICE is no moderator of C1.
#[tokio::test]
async fn recording_empty_patch_by_non_moderator_is_forbidden() {
    let backend = recording_backend();
    let resp = rset(&backend, ALICE, json!({ "update": { "R1": {} } })).await;
    assert_eq!(resp["notUpdated"]["R1"]["type"], "forbidden", "{resp}");
}

/// §5.3: server-set and immutable VTCLivestream values sent as `/get`
/// returns them are allowed; with nothing else, the update is a no-op.
#[tokio::test]
async fn livestream_identical_server_set_values_are_a_noop() {
    let backend = livestream_backend();
    let before = lget(&backend, "L1").await;
    let patch = pick(&before, LIVESTREAM_READ_ONLY);
    let resp = lset(&backend, json!({ "update": { "L1": patch } })).await;
    assert_noop(&resp, "L1");
    assert_eq!(lget(&backend, "L1").await, before);
}

/// Decision on JMAP-t8ej.25: a literal `{}` is a no-op success.
#[tokio::test]
async fn livestream_empty_patch_is_a_noop() {
    let backend = livestream_backend();
    let before = lget(&backend, "L1").await;
    let resp = lset(&backend, json!({ "update": { "L1": {} } })).await;
    assert_noop(&resp, "L1");
    assert_eq!(lget(&backend, "L1").await, before);
}

/// §5.3: identical server-set values do not block the stop beside them
/// (draft-atwood-jmap-vtc-00 L1462 "`update` with `state: "stopped"` ends
/// the stream").
#[tokio::test]
async fn livestream_identical_values_beside_a_stop_apply() {
    let backend = livestream_backend();
    let mut patch = pick(&lget(&backend, "L1").await, LIVESTREAM_READ_ONLY);
    patch["state"] = json!("stopped");
    let resp = lset(&backend, json!({ "update": { "L1": patch } })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(lget(&backend, "L1").await["state"], "stopped");
}

/// §5.3 "Otherwise, the update MUST be rejected with an
/// "invalidProperties" SetError."
#[tokio::test]
async fn livestream_changed_server_set_value_is_invalid_properties() {
    let backend = livestream_backend();
    let resp = lset(
        &backend,
        json!({ "update": { "L1": { "stoppedAt": "2000-01-01T00:00:00Z" } } }),
    )
    .await;
    let err = &resp["notUpdated"]["L1"];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(err["properties"], json!(["stoppedAt"]), "{resp}");
}
