//! Shared fixtures. Line numbers cite draft-atwood-jmap-vtc-00.md at
//! jmap-chat-spec 3227c0f.

#![allow(dead_code)]

use jmap_server::HandlerOutput;
use jmap_types::Id;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{handle_vtc_call_get, handle_vtc_call_query, handle_vtc_call_set};
use jmap_vtc_types::VtcCapability;
use serde_json::{Value, json};

pub const ACCOUNT: &str = "A1";
pub const BOB: &str = "user:bob@example.com";
pub const ALICE: &str = "user:alice@example.com";
pub const EVE: &str = "user:eve@example.com";
pub const CHAT_URI: &str = "urn:ietf:params:jmap:chat";

/// An account-level capability object, hand-written from L148-187, with
/// every call type, lobby and breakout rooms enabled.
pub fn capability() -> Value {
    json!({
        "mayCreateCall": true,
        "supportsRingCalls": true,
        "supportsRoomCalls": true,
        "supportsScheduledCalls": true,
        "supportsRecording": false,
        "supportsLivestream": false,
        "supportsLobby": true,
        "supportsBreakoutRooms": true,
        "supportedMediaTypes": ["audio", "video"],
        "maxParticipantsPerCall": null,
        "maxConcurrentCalls": null,
        "maxBreakoutRooms": null
    })
}

/// A backend with [`ACCOUNT`] registered under `capability`.
pub fn backend_with(capability: Value) -> MemoryBackend {
    let backend = MemoryBackend::new();
    let cap: VtcCapability = serde_json::from_value(capability).expect("capability fixture");
    backend.register_account(&Id::from(ACCOUNT), cap);
    backend
}

/// A backend with [`ACCOUNT`] registered under [`capability`].
pub fn backend() -> MemoryBackend {
    backend_with(capability())
}

/// Run `VTCCall/set` on [`ACCOUNT`] as `caller` with `args` (without
/// `accountId`) and return the response.
pub async fn set(backend: &MemoryBackend, caller: &str, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    let HandlerOutput { primary, .. } = handle_vtc_call_set(backend, &caller.to_owned(), args)
        .await
        .expect("VTCCall/set");
    primary
}

/// Create one call as `caller` and return its `created` entry.
pub async fn create(backend: &MemoryBackend, caller: &str, call: Value) -> Value {
    let resp = set(backend, caller, json!({ "create": { "c0": call } })).await;
    assert!(resp["notCreated"].is_null(), "create failed: {resp}");
    resp["created"]["c0"].clone()
}

/// The `notCreated` entry for one create, which must fail.
pub async fn create_error(backend: &MemoryBackend, caller: &str, call: Value) -> Value {
    let resp = set(backend, caller, json!({ "create": { "c0": call } })).await;
    assert!(resp["created"].is_null(), "create must fail: {resp}");
    resp["notCreated"]["c0"].clone()
}

/// Apply one update as `caller`; returns `Ok(updated echo)` or
/// `Err(notUpdated entry)`.
pub async fn update(
    backend: &MemoryBackend,
    caller: &str,
    id: &str,
    patch: Value,
) -> Result<Value, Value> {
    let resp = set(backend, caller, json!({ "update": { id: patch } })).await;
    match resp["notUpdated"].get(id) {
        Some(err) => Err(err.clone()),
        None => Ok(resp["updated"][id].clone()),
    }
}

/// The stored call `id`.
pub async fn get(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_vtc_call_get(
        backend,
        &BOB.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [id] }),
    )
    .await
    .expect("VTCCall/get");
    primary["list"][0].clone()
}

/// `VTCCall/query` on [`ACCOUNT`] with `args` (without `accountId`).
pub async fn query(
    backend: &MemoryBackend,
    mut args: Value,
) -> Result<Value, jmap_types::JmapError> {
    args["accountId"] = json!(ACCOUNT);
    handle_vtc_call_query(backend, &BOB.to_owned(), args)
        .await
        .map(|o| o.primary)
}

/// A stored call, hand-written from the field list at L342-406, for states
/// the `VTCCall/set` methods cannot reach on their own.
pub fn stored_call(id: &str, call_type: &str, state: &str, created_at: &str) -> Value {
    json!({
        "id": id,
        "accountId": ACCOUNT,
        "callType": call_type,
        "state": state,
        "createdAt": created_at,
        "endedAt": null,
        "endReason": null,
        "initiatorId": BOB,
        "subject": null,
        "joinUri": "https://meet.example.com/room/xkz",
        "joinPassword": null,
        "mediaTypes": ["audio"],
        "activeParticipantCount": 0,
        "lobbyEnabled": false,
        "parentCallId": null,
        "breakoutRoomIds": [],
        "scheduledStartAt": null,
        "gatewayPin": null,
        "e2eeEnabled": false
    })
}

/// Seed `call` (from [`stored_call`]) into the backend.
pub fn seed(backend: &MemoryBackend, call: Value) {
    let id = call["id"].as_str().expect("id").to_owned();
    backend.insert_object_for_test("VTCCall", &Id::from(ACCOUNT), &id, call);
}

/// [`capability`] with `supportsRecording` and `supportsLivestream`
/// (L163-166) enabled.
pub fn media_capability() -> Value {
    let mut cap = capability();
    cap["supportsRecording"] = json!(true);
    cap["supportsLivestream"] = json!(true);
    cap
}

/// A backend under [`media_capability`] holding call `C1`: a room call
/// in state `"active"` whose initiator is [`BOB`] ("The call initiator
/// receives `"moderator"` by default", L294).
pub fn media_backend() -> MemoryBackend {
    let backend = backend_with(media_capability());
    seed(
        &backend,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    backend
}

/// Run handler `$method` on [`ACCOUNT`] as `$caller` with `$args` (without
/// `accountId`) and return the response.
#[allow(unused_macros)]
macro_rules! call {
    ($method:path, $backend:expr, $caller:expr, $args:expr $(,)?) => {{
        let mut args: serde_json::Value = $args;
        args["accountId"] = serde_json::json!($crate::common::ACCOUNT);
        $method($backend, &$caller.to_owned(), args)
            .await
            .expect("method")
            .primary
    }};
}

/// A stored recording, hand-written from the field list at L461-489.
pub fn stored_recording(id: &str, state: &str, started_at: &str) -> Value {
    json!({
        "id": id,
        "callId": "C1",
        "state": state,
        "startedAt": started_at,
        "stoppedAt": null,
        "initiatedBy": BOB,
        "blobId": null,
        "size": null,
        "duration": null,
        "mediaType": null
    })
}

/// A stored livestream, hand-written from the field list at L495-517 with
/// the stream values of the L1170-1176 create example.
pub fn stored_livestream(id: &str, state: &str, started_at: Value) -> Value {
    json!({
        "id": id,
        "callId": "C1",
        "state": state,
        "platform": "youtube",
        "streamUri": "rtmp://live.example.com/ingest",
        "streamKey": "sk_live_abc123def456",
        "startedAt": started_at,
        "stoppedAt": null
    })
}

/// Seed `obj` as object of `type_name` into the backend.
pub fn seed_as(backend: &MemoryBackend, type_name: &'static str, obj: Value) {
    let id = obj["id"].as_str().expect("id").to_owned();
    backend.insert_object_for_test(type_name, &Id::from(ACCOUNT), &id, obj);
}
