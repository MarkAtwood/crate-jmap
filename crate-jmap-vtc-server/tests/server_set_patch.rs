//! Server-set values and empty patches in `VTCCall/set` and
//! `VTCParticipant/set` updates (bd:JMAP-t8ej.25.2).
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

mod common;

use common::*;
use jmap_vtc_server::handle_vtc_participant_set;
use jmap_vtc_server::memory::MemoryBackend;
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
        .unwrap_or_else(|| panic!("join failed: {resp}"))
        .to_owned();
    jmap_vtc_server::handle_vtc_participant_get(
        backend,
        &BOB.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [id] }),
    )
    .await
    .expect("VTCParticipant/get")
    .primary["list"][0]
        .clone()
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
}

/// Decision on JMAP-t8ej.25: a literal `{}` is a no-op success.
#[tokio::test]
async fn participant_empty_patch_is_a_noop() {
    let backend = backend();
    let before = joined(&backend).await;
    let id = before["id"].as_str().expect("id").to_owned();
    let resp = pset(&backend, json!({ "update": { &id: {} } })).await;
    assert_noop(&resp, &id);
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
