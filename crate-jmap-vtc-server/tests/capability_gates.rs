//! The account-level capability gates of `VTCCall/set`
//! (draft-atwood-jmap-vtc-00 §VTCCall/set). Line numbers cite
//! draft-atwood-jmap-vtc-00.md at jmap-chat-spec 03a4b76:
//!
//! - L779 "The account-level capability gates the following, on `create`
//!   and, for `lobbyEnabled`, on `update`:"
//! - L781 "`lobbyEnabled: true` when `supportsLobby` is `false`:
//!   `invalidProperties` naming `lobbyEnabled`."
//! - L782 "A non-null `parentCallId` when `supportsBreakoutRooms` is
//!   `false`: `invalidProperties` naming `parentCallId`."
//! - L783 "A create that would give the account more than
//!   `maxConcurrentCalls` calls in `"creating"`, `"ringing"`, `"active"` or
//!   `"pending"` state: `overQuota`."
//! - L784 "A create with a `parentCallId` whose call already has
//!   `maxBreakoutRooms` breakout rooms: `overQuota`."

mod common;

use common::*;
use serde_json::{Value, json};

fn room_call() -> Value {
    json!({ "callType": "room", "mediaTypes": ["audio"] })
}

fn capability_with(key: &str, value: Value) -> Value {
    let mut cap = capability();
    cap[key] = value;
    cap
}

#[tokio::test]
async fn lobby_on_create_without_supports_lobby_is_invalid() {
    let backend = backend_with(capability_with("supportsLobby", json!(false)));
    let mut call = room_call();
    call["lobbyEnabled"] = json!(true);
    // L781: `invalidProperties` naming `lobbyEnabled`.
    let err = create_error(&backend, BOB, call).await;
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["lobbyEnabled"]), "{err}");

    // L781 names only `true`: `false` is no lobby, which needs no support.
    let mut call = room_call();
    call["lobbyEnabled"] = json!(false);
    create(&backend, BOB, call).await;
}

#[tokio::test]
async fn lobby_on_update_without_supports_lobby_is_invalid() {
    let backend = backend_with(capability_with("supportsLobby", json!(false)));
    seed(
        &backend,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    // L779 "for `lobbyEnabled`, on `update`"; L781.
    let err = update(&backend, BOB, "C1", json!({ "lobbyEnabled": true }))
        .await
        .expect_err("lobby needs supportsLobby");
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["lobbyEnabled"]), "{err}");
    assert_eq!(get(&backend, "C1").await["lobbyEnabled"], false);
}

#[tokio::test]
async fn lobby_is_allowed_when_supported() {
    // The fixture capability has `"supportsLobby": true`.
    let backend = backend();
    let mut call = room_call();
    call["lobbyEnabled"] = json!(true);
    assert!(create(&backend, BOB, call).await["id"].is_string());
}

#[tokio::test]
async fn breakout_room_without_supports_breakout_rooms_is_invalid() {
    let mut cap = capability_with("supportsBreakoutRooms", json!(false));
    // L266 `maxBreakoutRooms`: "Absent when `supportsBreakoutRooms` is
    // `false`."
    cap.as_object_mut()
        .expect("object")
        .remove("maxBreakoutRooms");
    let backend = backend_with(cap);
    let parent = create(&backend, BOB, room_call()).await["id"].clone();
    let mut room = room_call();
    room["parentCallId"] = parent;
    // L782: `invalidProperties` naming `parentCallId`.
    let err = create_error(&backend, BOB, room).await;
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["parentCallId"]), "{err}");

    // L782 says "non-null": an explicit `null` is no breakout room.
    let mut call = room_call();
    call["parentCallId"] = Value::Null;
    create(&backend, BOB, call).await;
}

#[tokio::test]
async fn create_over_max_concurrent_calls_is_over_quota() {
    let backend = backend_with(capability_with("maxConcurrentCalls", json!(4)));
    // One call in each state L783 counts.
    for (id, kind, state) in [
        ("C1", "ring", "creating"),
        ("C2", "ring", "ringing"),
        ("C3", "room", "active"),
        ("C4", "scheduled", "pending"),
    ] {
        seed(
            &backend,
            stored_call(id, kind, state, "2026-06-05T14:30:00Z"),
        );
    }
    // L783: a fifth would give the account more than 4.
    let err = create_error(&backend, BOB, room_call()).await;
    assert_eq!(err["type"], "overQuota", "{err}");
}

#[tokio::test]
async fn ended_calls_do_not_count_toward_max_concurrent_calls() {
    let backend = backend_with(capability_with("maxConcurrentCalls", json!(1)));
    let mut ended = stored_call("C1", "room", "ended", "2026-06-05T14:30:00Z");
    ended["endedAt"] = json!("2026-06-05T15:00:00Z");
    ended["endReason"] = json!("completed");
    seed(&backend, ended);
    // L783 counts only "`"creating"`, `"ringing"`, `"active"` or
    // `"pending"`" calls: an ended one leaves room for one more.
    create(&backend, BOB, room_call()).await;
    let err = create_error(&backend, BOB, room_call()).await;
    assert_eq!(err["type"], "overQuota", "{err}");
}

#[tokio::test]
async fn max_concurrent_calls_counts_calls_the_caller_cannot_access() {
    let backend = backend_with(capability_with("maxConcurrentCalls", json!(1)));
    seed(
        &backend,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    backend.deny_call_access(ALICE, "C1");
    // L783 "give the account more than `maxConcurrentCalls` calls": the
    // count is the account's, not the caller's view of it.
    let err = create_error(&backend, ALICE, room_call()).await;
    assert_eq!(err["type"], "overQuota", "{err}");
}

#[tokio::test]
async fn breakout_room_over_max_breakout_rooms_is_over_quota() {
    let backend = backend_with(capability_with("maxBreakoutRooms", json!(1)));
    let parent = create(&backend, BOB, room_call()).await["id"].clone();
    let mut room = room_call();
    room["parentCallId"] = parent.clone();
    create(&backend, BOB, room.clone()).await;
    // L784: the parent "already has `maxBreakoutRooms` breakout rooms".
    let err = create_error(&backend, BOB, room).await;
    assert_eq!(err["type"], "overQuota", "{err}");
    let parent = get(&backend, parent.as_str().expect("id")).await;
    assert_eq!(parent["breakoutRoomIds"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn null_max_breakout_rooms_is_no_limit() {
    // L266 `maxBreakoutRooms`: "`null` means no limit." The fixture
    // capability carries `null`.
    let backend = backend();
    let parent = create(&backend, BOB, room_call()).await["id"].clone();
    let mut room = room_call();
    room["parentCallId"] = parent;
    for _ in 0..3 {
        create(&backend, BOB, room.clone()).await;
    }
}
