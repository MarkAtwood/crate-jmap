//! `Chat.activeCallId` follows the bound VTCCall's state.
//!
//! Oracle: chat L570: "When a VTCCall with a `chatId` matching this Chat's
//! `id` transitions to state `"active"`, the server MUST set this field to
//! that VTCCall's `id` and emit a `StateChange` for the Chat type. When
//! that VTCCall transitions to state `"ended"`, the server MUST clear this
//! field to `null` and emit a `StateChange`. These updates are side
//! effects of the VTCCall state transition and MUST be applied atomically
//! with it from the client's perspective — a client polling `/changes`
//! after receiving the VTCCall `StateChange` MUST see the corresponding
//! Chat `activeCallId` update."
//!
//! Chat L661-662 (`activeCallAccountId`): "The id of the account hosting
//! the VTCCall named by `activeCallId` ... The server sets and clears this
//! field together with `activeCallId`, in the same state change: it is
//! non-null exactly when `activeCallId` is non-null."

mod common;

use common::*;
use serde_json::{Value, json};

const CHAT: &str = "chat1";

fn bound_with_chat() -> Bound {
    let backend = Bound::new();
    backend.seed_chat_object("Chat", group_chat(CHAT, &[ACCOUNT, "c-bob"]));
    backend
}

/// vtc L1946: "The call is immediately `"active"`. The bound Chat's
/// `activeCallId` is set to the new call's id (when {{JMAP-CHAT}} is
/// present)."
#[tokio::test]
async fn room_call_start_sets_active_call_id_with_state_change() {
    let backend = bound_with_chat();
    let before = chat_state(&backend).await;

    let call = create_call(
        &backend,
        json!({ "callType": "room", "mediaTypes": ["audio"], "chatId": CHAT }),
    )
    .await;
    let call_id = call["id"].as_str().expect("call id");

    assert_eq!(get_call(&backend, call_id).await["state"], "active");
    let chat = get_chat(&backend, CHAT).await;
    assert_eq!(chat["activeCallId"], call_id);
    assert_eq!(chat["activeCallAccountId"], ACCOUNT);
    assert_eq!(chat_updated_since(&backend, &before).await, json!([CHAT]));
}

#[tokio::test]
async fn ending_the_call_clears_active_call_id_with_state_change() {
    let backend = bound_with_chat();
    let call = create_call(
        &backend,
        json!({ "callType": "room", "mediaTypes": ["audio"], "chatId": CHAT }),
    )
    .await;
    let call_id = call["id"].as_str().expect("call id");
    let before = chat_state(&backend).await;

    let resp = vtc_set(
        &backend,
        OWNER,
        json!({ "update": { call_id: { "state": "ended" } } }),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "end failed: {resp}");

    assert_eq!(get_call(&backend, call_id).await["state"], "ended");
    let chat = get_chat(&backend, CHAT).await;
    assert_eq!(chat["activeCallId"], Value::Null);
    assert_eq!(chat["activeCallAccountId"], Value::Null);
    assert_eq!(chat_updated_since(&backend, &before).await, json!([CHAT]));
}

/// Only the `"active"` transition sets the field: a ring call is created
/// in `"ringing"` (vtc L434: "Server transitions automatically when the
/// call is created and ring notifications have been dispatched"), so the
/// Chat is not touched.
#[tokio::test]
async fn ringing_call_leaves_active_call_id_null() {
    let backend = bound_with_chat();
    let before = chat_state(&backend).await;

    let call = create_call(
        &backend,
        json!({
            "callType": "ring",
            "mediaTypes": ["audio"],
            "chatId": CHAT,
            "targetParticipantIds": [BOB]
        }),
    )
    .await;
    let call_id = call["id"].as_str().expect("call id");

    assert_eq!(get_call(&backend, call_id).await["state"], "ringing");
    let chat = get_chat(&backend, CHAT).await;
    assert_eq!(chat["activeCallId"], Value::Null);
    assert_eq!(chat["activeCallAccountId"], Value::Null);
    assert_eq!(chat_updated_since(&backend, &before).await, json!([]));
}
