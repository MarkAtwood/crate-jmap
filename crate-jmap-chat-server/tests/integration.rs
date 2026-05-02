// Integration test entry point for jmap-chat-server.
//
// The common module provides MemoryBackend — an in-memory ChatBackend used
// as the test harness for all handler integration tests.
#![allow(async_fn_in_trait)]

mod common;

use common::{FaultyBackend, MemoryBackend};
use jmap_chat_server::{
    handle_ban_get, handle_ban_set, handle_chat_changes, handle_chat_get, handle_chat_query,
    handle_chat_query_changes, handle_chat_set, handle_contact_changes, handle_contact_get,
    handle_contact_query, handle_contact_query_changes, handle_contact_set, handle_invite_get,
    handle_invite_set, handle_message_changes, handle_message_get, handle_message_query,
    handle_message_set, handle_position_get, handle_position_set, handle_presence_changes,
    handle_presence_get, handle_presence_set, handle_space_get, handle_space_join,
    handle_space_set, ChatBackend, JmapBackend,
};
use jmap_chat_types::{Chat, ChatKind};
use jmap_types::Id;
use serde_json::json;

// ---------------------------------------------------------------------------
// MemoryBackend smoke tests
// ---------------------------------------------------------------------------

/// Oracle: initial state for any type in a fresh backend is "0".
/// RFC 8620 §5.2 — sinceState "0" means "no prior synchronization".
#[tokio::test]
async fn memory_backend_initial_state_is_zero() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    let state = backend
        .get_state::<Chat>(&account_id)
        .await
        .expect("get_state must not fail on fresh backend");
    assert_eq!(state.as_ref(), "0", "initial state must be \"0\"");
}

/// Oracle: state advances after a successful create_object call.
#[tokio::test]
async fn memory_backend_state_advances_after_create() {
    use jmap_types::UTCDate;

    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let state_before = backend
        .get_state::<Chat>(&account_id)
        .await
        .expect("get_state");

    let chat = Chat::new(
        Id::from("placeholder"),
        ChatKind::Group,
        UTCDate::from("2026-01-01T00:00:00Z"),
        0,
        vec![],
        false,
        true,
    );
    backend
        .create_object::<Chat>(&account_id, "c0", chat)
        .await
        .expect("create_object");

    let state_after = backend
        .get_state::<Chat>(&account_id)
        .await
        .expect("get_state");
    assert_ne!(
        state_before, state_after,
        "state must change after create_object"
    );
}

/// Oracle: created objects are retrievable by id.
#[tokio::test]
async fn memory_backend_create_then_get() {
    use jmap_types::UTCDate;

    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");

    let chat = Chat::new(
        Id::from("placeholder"),
        ChatKind::Group,
        UTCDate::from("2026-01-01T00:00:00Z"),
        0,
        vec![],
        false,
        true,
    );
    let (server_id, _) = backend
        .create_object::<Chat>(&account_id, "c0", chat)
        .await
        .expect("create_object");

    let (found, not_found) = backend
        .get_objects::<Chat>(&account_id, Some(&[server_id.clone()]), None)
        .await
        .expect("get_objects");

    assert_eq!(found.len(), 1);
    assert!(not_found.is_empty());
    assert_eq!(found[0].id, server_id);
}

// ---------------------------------------------------------------------------
// Chat/get
// ---------------------------------------------------------------------------

/// Oracle: Chat/get on empty backend returns empty list, state "0", notFound null.
#[tokio::test]
async fn chat_get_empty() {
    let backend = MemoryBackend::new();
    let (resp, invocations) = handle_chat_get(&backend, json!({ "accountId": "a1", "ids": null }))
        .await
        .expect("handle_chat_get");

    assert!(invocations.is_empty());
    assert_eq!(resp["accountId"], "a1");
    assert_eq!(resp["state"], "0");
    assert_eq!(resp["list"], json!([]));
    assert_eq!(resp["notFound"], json!([]));
}

/// Oracle: Chat/get with a missing id returns notFound entry.
#[tokio::test]
async fn chat_get_not_found() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_chat_get(&backend, json!({ "accountId": "a1", "ids": ["missing1"] }))
        .await
        .expect("handle_chat_get");

    assert_eq!(resp["list"], json!([]));
    assert_eq!(resp["notFound"], json!(["missing1"]));
}

/// Oracle: Chat/get without accountId returns invalidArguments error.
#[tokio::test]
async fn chat_get_missing_account_id() {
    let backend = MemoryBackend::new();
    let err = handle_chat_get(&backend, json!({})).await.unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}

// ---------------------------------------------------------------------------
// Chat/set — create
// ---------------------------------------------------------------------------

/// Oracle: Chat/set create with kind "group" succeeds and returns the created object.
#[tokio::test]
async fn chat_set_create_group() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "c0": { "kind": "group", "name": "My Group" }
            }
        }),
    )
    .await
    .expect("handle_chat_set");

    assert!(resp["created"]["c0"].is_object(), "c0 must be in created");
    assert_eq!(resp["notCreated"], json!(null));
    assert_ne!(resp["newState"], resp["oldState"], "state must advance");
}

/// Oracle: Chat/set create with kind "direct" and contactId succeeds.
#[tokio::test]
async fn chat_set_create_direct_with_contact_id() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "c0": { "kind": "direct", "contactId": "u1" }
            }
        }),
    )
    .await
    .expect("handle_chat_set");

    assert!(resp["created"]["c0"].is_object());
    assert_eq!(resp["notCreated"], json!(null));
}

/// Oracle: Chat/set create with kind "direct" and no contactId is rejected.
#[tokio::test]
async fn chat_set_create_direct_missing_contact_id() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "c0": { "kind": "direct" }
            }
        }),
    )
    .await
    .expect("handle_chat_set");

    assert_eq!(resp["created"], json!(null));
    assert!(resp["notCreated"]["c0"].is_object());
    assert_eq!(resp["notCreated"]["c0"]["type"], "invalidProperties");
}

/// Oracle: Chat/set create with kind "channel" and no spaceId is rejected.
#[tokio::test]
async fn chat_set_create_channel_missing_space_id() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "c0": { "kind": "channel" }
            }
        }),
    )
    .await
    .expect("handle_chat_set");

    assert!(resp["notCreated"]["c0"].is_object());
    assert_eq!(resp["notCreated"]["c0"]["type"], "invalidProperties");
    let props = resp["notCreated"]["c0"]["properties"]
        .as_array()
        .expect("properties");
    assert!(props.iter().any(|p| p == "spaceId"));
}

/// Oracle: Chat/set create without kind is rejected with invalidProperties.
#[tokio::test]
async fn chat_set_create_missing_kind() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "c0": { "name": "No Kind" }
            }
        }),
    )
    .await
    .expect("handle_chat_set");

    assert!(resp["notCreated"]["c0"].is_object());
    let props = resp["notCreated"]["c0"]["properties"]
        .as_array()
        .expect("properties");
    assert!(props.iter().any(|p| p == "kind"));
}

/// Oracle: Chat/set update of a server-set field (createdAt) is rejected.
#[tokio::test]
async fn chat_set_update_readonly_field_rejected() {
    let backend = MemoryBackend::new();

    // Create a chat first.
    let (create_resp, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "c0": { "kind": "group", "name": "G" } }
        }),
    )
    .await
    .expect("create");
    let chat_id = create_resp["created"]["c0"]["id"].as_str().expect("id");

    let (resp, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": {
                chat_id: { "createdAt": "2020-01-01T00:00:00Z" }
            }
        }),
    )
    .await
    .expect("update");

    assert!(resp["notUpdated"][chat_id].is_object());
    assert_eq!(resp["notUpdated"][chat_id]["type"], "invalidProperties");
}

/// Oracle: Chat/set destroy removes the object.
#[tokio::test]
async fn chat_set_destroy() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "c0": { "kind": "group", "name": "Temp" } }
        }),
    )
    .await
    .expect("create");
    let chat_id = create_resp["created"]["c0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (destroy_resp, _) =
        handle_chat_set(&backend, json!({ "accountId": "a1", "destroy": [chat_id] }))
            .await
            .expect("destroy");

    assert!(destroy_resp["destroyed"].as_array().is_some());
    assert_eq!(destroy_resp["notDestroyed"], json!(null));
}

/// Oracle: Chat/set ifInState mismatch returns stateMismatch error.
#[tokio::test]
async fn chat_set_if_in_state_mismatch() {
    let backend = MemoryBackend::new();
    let err = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "ifInState": "999",
            "create": { "c0": { "kind": "group" } }
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.as_str(), "stateMismatch");
}

// ---------------------------------------------------------------------------
// Chat/changes
// ---------------------------------------------------------------------------

/// Oracle: Chat/changes with sinceState "0" on empty backend returns empty deltas.
#[tokio::test]
async fn chat_changes_empty() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_chat_changes(&backend, json!({ "accountId": "a1", "sinceState": "0" }))
        .await
        .expect("handle_chat_changes");

    assert_eq!(resp["created"], json!([]));
    assert_eq!(resp["updated"], json!([]));
    assert_eq!(resp["destroyed"], json!([]));
    assert!(!resp["hasMoreChanges"].as_bool().unwrap_or(true));
}

/// Oracle: Chat/changes missing sinceState returns invalidArguments.
#[tokio::test]
async fn chat_changes_missing_since_state() {
    let backend = MemoryBackend::new();
    let err = handle_chat_changes(&backend, json!({ "accountId": "a1" }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}

/// Oracle: after creating a chat, Chat/changes from state "0" shows it in created.
#[tokio::test]
async fn chat_changes_after_create() {
    let backend = MemoryBackend::new();

    handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "c0": { "kind": "group", "name": "G" } }
        }),
    )
    .await
    .expect("create");

    let (resp, _) = handle_chat_changes(&backend, json!({ "accountId": "a1", "sinceState": "0" }))
        .await
        .expect("changes");

    let created = resp["created"].as_array().expect("created array");
    assert_eq!(created.len(), 1, "one created id expected");
}

// ---------------------------------------------------------------------------
// Chat/query
// ---------------------------------------------------------------------------

/// Oracle: Chat/query on empty backend returns empty ids and position 0.
#[tokio::test]
async fn chat_query_empty() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_chat_query(&backend, json!({ "accountId": "a1" }))
        .await
        .expect("handle_chat_query");

    assert_eq!(resp["ids"], json!([]));
    assert_eq!(resp["position"], 0);
}

/// Oracle: Chat/query with calculateTotal=true includes total field.
#[tokio::test]
async fn chat_query_with_calculate_total() {
    let backend = MemoryBackend::new();
    handle_chat_set(
        &backend,
        json!({ "accountId": "a1", "create": { "c0": { "kind": "group", "name": "G" } } }),
    )
    .await
    .expect("create");

    let (resp, _) = handle_chat_query(
        &backend,
        json!({ "accountId": "a1", "calculateTotal": true }),
    )
    .await
    .expect("query");

    assert!(resp["total"].is_number(), "total must be present");
    assert_eq!(resp["total"], json!(1u64));
}

// ---------------------------------------------------------------------------
// Chat/queryChanges
// ---------------------------------------------------------------------------

/// Oracle: Chat/queryChanges missing sinceQueryState returns invalidArguments.
#[tokio::test]
async fn chat_query_changes_missing_since() {
    let backend = MemoryBackend::new();
    let err = handle_chat_query_changes(&backend, json!({ "accountId": "a1" }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}

/// Oracle: after creating a chat, queryChanges from "0" shows it in added.
#[tokio::test]
async fn chat_query_changes_after_create() {
    let backend = MemoryBackend::new();

    handle_chat_set(
        &backend,
        json!({ "accountId": "a1", "create": { "c0": { "kind": "group", "name": "G" } } }),
    )
    .await
    .expect("create");

    let (resp, _) = handle_chat_query_changes(
        &backend,
        json!({ "accountId": "a1", "sinceQueryState": "0" }),
    )
    .await
    .expect("queryChanges");

    let added = resp["added"].as_array().expect("added array");
    assert_eq!(added.len(), 1);
    assert!(added[0]["id"].is_string());
    assert!(added[0]["index"].is_number());
}

// ---------------------------------------------------------------------------
// Message/get and Message/set
// ---------------------------------------------------------------------------

/// Oracle: Message/set create requires chatId and body.
#[tokio::test]
async fn message_set_create_missing_chat_id() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "m0": { "body": "Hello" } }
        }),
    )
    .await
    .expect("handle_message_set");

    assert!(resp["notCreated"]["m0"].is_object());
    let props = resp["notCreated"]["m0"]["properties"]
        .as_array()
        .expect("props");
    assert!(props.iter().any(|p| p == "chatId"));
}

/// Oracle: Message/set create with chatId and body succeeds.
#[tokio::test]
async fn message_set_create_success() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "m0": { "chatId": "c1", "body": "Hello, world!", "sentAt": "2024-01-01T00:00:00Z" }
            }
        }),
    )
    .await
    .expect("handle_message_set");

    assert!(resp["created"]["m0"].is_object());
    assert_eq!(resp["notCreated"], json!(null));
    assert_eq!(resp["created"]["m0"]["body"], "Hello, world!");
}

/// Oracle: Message/set update of server-set field deliveryState is rejected.
#[tokio::test]
async fn message_set_update_readonly_field() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"].as_str().expect("id");

    let (resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": {
                msg_id: { "deliveryState": "delivered" }
            }
        }),
    )
    .await
    .expect("update");

    assert!(resp["notUpdated"][msg_id].is_object());
    assert_eq!(resp["notUpdated"][msg_id]["type"], "invalidProperties");
}

/// Oracle: Message/get returns created message.
#[tokio::test]
async fn message_get_returns_created() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "Test body", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (get_resp, _) = handle_message_get(&backend, json!({ "accountId": "a1", "ids": [msg_id] }))
        .await
        .expect("get");

    assert_eq!(get_resp["list"].as_array().expect("list").len(), 1);
    assert_eq!(get_resp["list"][0]["body"], "Test body");
}

/// Oracle: Message/changes after create shows new message in created list.
#[tokio::test]
async fn message_changes_after_create() {
    let backend = MemoryBackend::new();

    handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "Hi", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create");

    let (resp, _) =
        handle_message_changes(&backend, json!({ "accountId": "a1", "sinceState": "0" }))
            .await
            .expect("changes");

    assert_eq!(resp["created"].as_array().expect("created").len(), 1);
}

/// Oracle: Message/query returns messages in deterministic order.
#[tokio::test]
async fn message_query_order() {
    let backend = MemoryBackend::new();

    for i in 0..3u32 {
        handle_message_set(
            &backend,
            json!({
                "accountId": "a1",
                "create": { format!("m{i}"): { "chatId": "c1", "body": format!("msg {i}"), "sentAt": "2024-01-01T00:00:00Z" } }
            }),
        )
        .await
        .expect("create");
    }

    let (resp, _) = handle_message_query(
        &backend,
        json!({ "accountId": "a1", "calculateTotal": true, "filter": { "chatId": "c1" } }),
    )
    .await
    .expect("query");

    assert_eq!(resp["total"], json!(3u64));
    let ids = resp["ids"].as_array().expect("ids");
    assert_eq!(ids.len(), 3);
}

/// Oracle: updating body injects server-side editedAt.
#[tokio::test]
async fn message_set_update_body_injects_edited_at() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "original", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (update_resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "body": "edited" } }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][&msg_id], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) =
        handle_message_get(&backend, json!({ "accountId": "a1", "ids": [&msg_id] }))
            .await
            .expect("get");
    let msg = &get_resp["list"][0];
    assert_eq!(msg["body"], "edited", "body must be updated");
    assert!(
        msg["editedAt"].is_string(),
        "editedAt must be present after body update"
    );
}

/// Oracle: setting deletedForAll: true injects server-side deletedAt.
#[tokio::test]
async fn message_set_update_delete_for_all_injects_deleted_at() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "hello", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (update_resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "deletedForAll": true } }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][&msg_id], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) =
        handle_message_get(&backend, json!({ "accountId": "a1", "ids": [&msg_id] }))
            .await
            .expect("get");
    let msg = &get_resp["list"][0];
    assert_eq!(msg["deletedForAll"], true, "deletedForAll must be true");
    assert!(
        msg["deletedAt"].is_string(),
        "deletedAt must be present after deletedForAll update"
    );
}

/// Oracle: setting readAt stores the client-provided timestamp.
#[tokio::test]
async fn message_set_update_mark_as_read() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "read me", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let read_ts = "2024-01-01T00:00:00Z";
    let (update_resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "readAt": read_ts } }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][&msg_id], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) =
        handle_message_get(&backend, json!({ "accountId": "a1", "ids": [&msg_id] }))
            .await
            .expect("get");
    assert_eq!(
        get_resp["list"][0]["readAt"], read_ts,
        "readAt must match the client-provided timestamp"
    );
}

/// Oracle: spec §557 + §1029 — server MUST store "displayed" when readAt is set without readDisposition.
#[tokio::test]
async fn message_set_read_at_defaults_disposition() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        json!({ "accountId": "a1", "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2026-01-01T00:00:00Z" } } }),
    ).await.expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (update_resp, _) = handle_message_set(
        &backend,
        json!({ "accountId": "a1", "update": { msg_id.as_str(): { "readAt": "2026-01-05T10:00:00Z" } } }),
    ).await.expect("update");
    assert_eq!(update_resp["updated"][msg_id.as_str()], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        json!({ "accountId": "a1", "ids": [msg_id.as_str()] }),
    )
    .await
    .expect("get");
    let msg = &get_resp["list"][0];
    assert_eq!(msg["readAt"], "2026-01-05T10:00:00Z");
    assert_eq!(
        msg["readDisposition"], "displayed",
        "server must store \"displayed\" when readDisposition is omitted"
    );
}

/// Oracle: spec §1029 — explicit readDisposition is preserved, not overridden to "displayed".
#[tokio::test]
async fn message_set_explicit_disposition_preserved() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        json!({ "accountId": "a1", "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2026-01-01T00:00:00Z" } } }),
    ).await.expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (update_resp, _) = handle_message_set(
        &backend,
        json!({ "accountId": "a1", "update": { msg_id.as_str(): { "readAt": "2026-01-05T10:00:00Z", "readDisposition": "deleted" } } }),
    ).await.expect("update");
    assert_eq!(update_resp["updated"][msg_id.as_str()], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        json!({ "accountId": "a1", "ids": [msg_id.as_str()] }),
    )
    .await
    .expect("get");
    let msg = &get_resp["list"][0];
    assert_eq!(
        msg["readDisposition"], "deleted",
        "explicit \"deleted\" disposition must be preserved"
    );
}

/// Oracle: spec §335 — unrecognized readDisposition values MUST be stored as-is.
#[tokio::test]
async fn message_set_extension_disposition_stored_as_is() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        json!({ "accountId": "a1", "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2026-01-01T00:00:00Z" } } }),
    ).await.expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (update_resp, _) = handle_message_set(
        &backend,
        json!({ "accountId": "a1", "update": { msg_id.as_str(): { "readAt": "2026-01-05T10:00:00Z", "readDisposition": "voice-listened" } } }),
    ).await.expect("update");
    assert_eq!(update_resp["updated"][msg_id.as_str()], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        json!({ "accountId": "a1", "ids": [msg_id.as_str()] }),
    )
    .await
    .expect("get");
    let msg = &get_resp["list"][0];
    assert_eq!(
        msg["readDisposition"], "voice-listened",
        "unrecognized extension value must be stored as-is per spec §335"
    );
}

/// Oracle: spec §557 — readDisposition is absent when readAt is not set.
#[tokio::test]
async fn message_set_no_read_at_no_disposition_injected() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        json!({ "accountId": "a1", "create": { "m0": { "chatId": "c1", "body": "original", "sentAt": "2026-01-01T00:00:00Z" } } }),
    ).await.expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (update_resp, _) = handle_message_set(
        &backend,
        json!({ "accountId": "a1", "update": { msg_id.as_str(): { "body": "edited text" } } }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][msg_id.as_str()], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        json!({ "accountId": "a1", "ids": [msg_id.as_str()] }),
    )
    .await
    .expect("get");
    let msg = &get_resp["list"][0];
    assert_eq!(msg["body"], "edited text");
    assert_eq!(
        msg["readDisposition"],
        json!(null),
        "readDisposition must be absent when readAt is not set"
    );
}

/// Oracle: Message/set create with replyTo set stores and returns the field.
#[tokio::test]
async fn message_set_create_with_reply_to() {
    let backend = MemoryBackend::new();

    // Create the message that will be replied to.
    let (first_resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "original", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create first message");
    let first_id = first_resp["created"]["m0"]["id"]
        .as_str()
        .expect("first message id")
        .to_owned();

    // Create a reply pointing to the first message.
    let (resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "m1": {
                    "chatId": "c1",
                    "body": "reply",
                    "replyTo": first_id,
                    "sentAt": "2024-01-01T00:00:00Z"
                }
            }
        }),
    )
    .await
    .expect("create reply");

    assert!(resp["created"]["m1"].is_object(), "m1 must be in created");
    assert_eq!(resp["notCreated"], json!(null));
    assert_eq!(
        resp["created"]["m1"]["replyTo"], first_id,
        "replyTo must equal the first message id"
    );
}

/// Oracle: Message/set create with a past senderExpiresAt is rejected with
/// invalidProperties: ["senderExpiresAt"].
#[tokio::test]
async fn message_set_create_with_past_expiry_rejected() {
    let backend = MemoryBackend::new();

    let (resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "expires in the past",
                    "sentAt": "2024-01-01T00:00:00Z",
                    "senderExpiresAt": "2020-01-01T00:00:00Z"
                }
            }
        }),
    )
    .await
    .expect("handle_message_set");

    assert_eq!(resp["created"], json!(null));
    assert!(resp["notCreated"]["m0"].is_object());
    assert_eq!(resp["notCreated"]["m0"]["type"], "invalidProperties");
    let props = resp["notCreated"]["m0"]["properties"]
        .as_array()
        .expect("properties array");
    assert!(
        props.iter().any(|p| p == "senderExpiresAt"),
        "senderExpiresAt must be listed in rejected properties"
    );
}

// ---------------------------------------------------------------------------
// Space/get and Space/set
// ---------------------------------------------------------------------------

/// Oracle: Space/set create requires name.
#[tokio::test]
async fn space_set_create_missing_name() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_space_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "s0": { "isPublic": false } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(resp["notCreated"]["s0"].is_object());
    let props = resp["notCreated"]["s0"]["properties"]
        .as_array()
        .expect("props");
    assert!(props.iter().any(|p| p == "name"));
}

/// Oracle: Space/set create with name succeeds.
#[tokio::test]
async fn space_set_create_success() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_space_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "s0": { "name": "My Server", "isPublic": true } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(resp["created"]["s0"].is_object());
    assert_eq!(resp["created"]["s0"]["name"], "My Server");
}

/// Oracle: Space/get returns created space.
#[tokio::test]
async fn space_get_returns_created() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        json!({ "accountId": "a1", "create": { "s0": { "name": "The Space" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (get_resp, _) = handle_space_get(&backend, json!({ "accountId": "a1", "ids": [space_id] }))
        .await
        .expect("get");

    assert_eq!(get_resp["list"][0]["name"], "The Space");
}

/// Oracle: Space/set update rejects direct writes to server-managed array fields.
#[tokio::test]
async fn space_set_update_readonly_fields_rejected() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        json!({ "accountId": "a1", "create": { "s0": { "name": "Readonly Test" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    for field in &["roles", "members", "categories", "uncategorizedChannelIds"] {
        let (resp, _) = handle_space_set(
            &backend,
            json!({
                "accountId": "a1",
                "update": { &space_id: { (*field): [] } }
            }),
        )
        .await
        .expect("handle_space_set");

        assert!(
            resp["notUpdated"][&space_id].is_object(),
            "field {field} should be rejected"
        );
        assert_eq!(
            resp["notUpdated"][&space_id]["type"], "invalidProperties",
            "field {field} should yield invalidProperties"
        );
    }
}

/// Oracle: Space/set update rejects named mutation keys with forbidden.
#[tokio::test]
async fn space_set_update_mutation_keys_forbidden() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        json!({ "accountId": "a1", "create": { "s0": { "name": "Mutation Test" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    for key in &[
        "addRoles",
        "removeRoles",
        "updateRoles",
        "addMembers",
        "removeMembers",
        "updateMembers",
        "addChannels",
        "removeChannels",
        "updateChannels",
        "addCategories",
        "removeCategories",
        "updateCategories",
    ] {
        let (resp, _) = handle_space_set(
            &backend,
            json!({
                "accountId": "a1",
                "update": { &space_id: { (*key): [] } }
            }),
        )
        .await
        .expect("handle_space_set");

        assert!(
            resp["notUpdated"][&space_id].is_object(),
            "key {key} should be rejected"
        );
        assert_eq!(
            resp["notUpdated"][&space_id]["type"], "forbidden",
            "key {key} should yield forbidden"
        );
    }
}

/// Oracle: Space/set update accepts metadata fields (name, description, isPublic, etc.).
#[tokio::test]
async fn space_set_update_metadata_success() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        json!({ "accountId": "a1", "create": { "s0": { "name": "Original Name" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "name": "New Name",
                    "description": "Updated description",
                    "isPublic": true
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"].is_null(),
        "metadata update should not produce notUpdated: {:?}",
        resp["notUpdated"]
    );
    assert!(
        resp["updated"][&space_id].is_null(),
        "updated entry for the space should be present (null sentinel)"
    );
    // Verify the field was actually persisted via Space/get.
    let (get_resp, _) =
        handle_space_get(&backend, json!({ "accountId": "a1", "ids": [&space_id] }))
            .await
            .expect("handle_space_get");
    assert_eq!(get_resp["list"][0]["name"], "New Name");
    assert_eq!(get_resp["list"][0]["description"], "Updated description");
    assert_eq!(get_resp["list"][0]["isPublic"], true);
}

// ---------------------------------------------------------------------------
// ChatContact/get and ChatContact/set
// ---------------------------------------------------------------------------

/// Oracle: ChatContact/set create is always forbidden (server-managed objects).
#[tokio::test]
async fn contact_set_create_forbidden() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_contact_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "ct0": { "login": "alice@example.com", "blocked": false } }
        }),
    )
    .await
    .expect("handle_contact_set");

    assert!(resp["created"].is_null());
    assert!(resp["notCreated"]["ct0"].is_object());
    assert_eq!(resp["notCreated"]["ct0"]["type"], "forbidden");
}

/// Oracle: ChatContact/set destroy is always forbidden (server-managed objects).
#[tokio::test]
async fn contact_set_destroy_forbidden() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_contact_set(
        &backend,
        json!({
            "accountId": "a1",
            "destroy": ["some-id"]
        }),
    )
    .await
    .expect("handle_contact_set");

    assert!(resp["destroyed"].is_null());
    assert!(resp["notDestroyed"]["some-id"].is_object());
    assert_eq!(resp["notDestroyed"]["some-id"]["type"], "forbidden");
}

/// Oracle: ChatContact/get on an empty backend returns an empty list.
#[tokio::test]
async fn contact_get_empty() {
    let backend = MemoryBackend::new();
    let (get_resp, _) = handle_contact_get(&backend, json!({ "accountId": "a1", "ids": null }))
        .await
        .expect("get");

    assert_eq!(get_resp["list"].as_array().expect("list").len(), 0);
}

/// Oracle: ChatContact/changes on an empty backend returns empty change lists.
#[tokio::test]
async fn contact_changes_empty() {
    let backend = MemoryBackend::new();
    let (resp, _) =
        handle_contact_changes(&backend, json!({ "accountId": "a1", "sinceState": "0" }))
            .await
            .expect("changes");

    assert_eq!(resp["created"].as_array().expect("created").len(), 0);
    assert_eq!(resp["updated"].as_array().expect("updated").len(), 0);
    assert_eq!(resp["destroyed"].as_array().expect("destroyed").len(), 0);
}

// ---------------------------------------------------------------------------
// ReadPosition/get and ReadPosition/set
// ---------------------------------------------------------------------------

/// Oracle: ReadPosition/set create requires chatId.
#[tokio::test]
async fn position_set_create_missing_chat_id() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_position_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "rp0": { "lastReadMessageId": "m1" } }
        }),
    )
    .await
    .expect("handle_position_set");

    assert!(resp["notCreated"]["rp0"].is_object());
    let props = resp["notCreated"]["rp0"]["properties"]
        .as_array()
        .expect("props");
    assert!(props.iter().any(|p| p == "chatId"));
}

/// Oracle: ReadPosition/set create with chatId succeeds.
#[tokio::test]
async fn position_set_create_success() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_position_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "rp0": { "chatId": "c1" } }
        }),
    )
    .await
    .expect("handle_position_set");

    assert!(resp["created"]["rp0"].is_object());
    assert_eq!(resp["created"]["rp0"]["chatId"], "c1");
}

/// Oracle: ReadPosition/set update of chatId is rejected (immutable).
#[tokio::test]
async fn position_set_update_chat_id_rejected() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_position_set(
        &backend,
        json!({ "accountId": "a1", "create": { "rp0": { "chatId": "c1" } } }),
    )
    .await
    .expect("create");
    let rp_id = create_resp["created"]["rp0"]["id"].as_str().expect("id");

    let (resp, _) = handle_position_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": { rp_id: { "chatId": "c2" } }
        }),
    )
    .await
    .expect("update");

    assert!(resp["notUpdated"][rp_id].is_object());
    assert_eq!(resp["notUpdated"][rp_id]["type"], "invalidProperties");
}

/// Oracle: ReadPosition/get returns created position.
#[tokio::test]
async fn position_get_returns_created() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_position_set(
        &backend,
        json!({ "accountId": "a1", "create": { "rp0": { "chatId": "c99" } } }),
    )
    .await
    .expect("create");
    let rp_id = create_resp["created"]["rp0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (get_resp, _) = handle_position_get(&backend, json!({ "accountId": "a1", "ids": [rp_id] }))
        .await
        .expect("get");

    assert_eq!(get_resp["list"][0]["chatId"], "c99");
}

// ---------------------------------------------------------------------------
// Backend error propagation
// ---------------------------------------------------------------------------

/// Oracle: when backend.get_objects fails, Chat/get returns serverFail error.
#[tokio::test]
async fn chat_get_backend_error_maps_to_server_fail() {
    let backend = FaultyBackend;
    let err = handle_chat_get(&backend, json!({ "accountId": "a1", "ids": null }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type.as_str(), "serverFail");
}

/// Oracle: when backend.get_state fails at the start of set, Chat/set returns serverFail.
#[tokio::test]
async fn chat_set_backend_error_maps_to_server_fail() {
    let backend = FaultyBackend;
    let err = handle_chat_set(
        &backend,
        json!({ "accountId": "a1", "create": { "c0": { "kind": "group" } } }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.as_str(), "serverFail");
}

// ---------------------------------------------------------------------------
// Deduplication of direct chats
// ---------------------------------------------------------------------------

/// Oracle: creating a second direct chat with the same contactId is rejected
/// with SetError type "alreadyExists" and the existingId set to the first chat.
#[tokio::test]
async fn chat_set_create_direct_duplicate_rejected() {
    let backend = MemoryBackend::new();

    // First create succeeds.
    let (resp1, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "c0": { "kind": "direct", "contactId": "u1" }
            }
        }),
    )
    .await
    .expect("first create");
    assert!(
        resp1["created"]["c0"].is_object(),
        "first create must succeed"
    );
    let existing_id = resp1["created"]["c0"]["id"]
        .as_str()
        .expect("id in created response")
        .to_owned();

    // Second create with same contactId must be rejected.
    let (resp2, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "c1": { "kind": "direct", "contactId": "u1" }
            }
        }),
    )
    .await
    .expect("second create call");

    assert_eq!(
        resp2["created"],
        json!(null),
        "duplicate must not be created"
    );
    let err = &resp2["notCreated"]["c1"];
    assert!(err.is_object(), "notCreated entry must be present");
    assert_eq!(err["type"], "alreadyExists");
    assert_eq!(err["existingId"], existing_id);
}

/// Oracle: direct chats with different contactIds are allowed (no false positive dedup).
#[tokio::test]
async fn chat_set_create_direct_different_contacts_allowed() {
    let backend = MemoryBackend::new();

    let (resp1, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "c0": { "kind": "direct", "contactId": "u1" } }
        }),
    )
    .await
    .expect("create u1");
    assert!(resp1["created"]["c0"].is_object());

    let (resp2, _) = handle_chat_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "c1": { "kind": "direct", "contactId": "u2" } }
        }),
    )
    .await
    .expect("create u2");
    assert!(
        resp2["created"]["c1"].is_object(),
        "different contactId must succeed"
    );
    assert_eq!(resp2["notCreated"], json!(null));
}

// ---------------------------------------------------------------------------
// Cross-account isolation
// ---------------------------------------------------------------------------

/// Oracle: objects created in account A are not visible in account B.
#[tokio::test]
async fn accounts_are_isolated() {
    let backend = MemoryBackend::new();

    handle_chat_set(
        &backend,
        json!({ "accountId": "a1", "create": { "c0": { "kind": "group", "name": "A's group" } } }),
    )
    .await
    .expect("create in a1");

    let (resp, _) = handle_chat_get(&backend, json!({ "accountId": "a2", "ids": null }))
        .await
        .expect("get from a2");

    assert_eq!(
        resp["list"].as_array().expect("list").len(),
        0,
        "a2 must not see a1's chats"
    );
}

// ---------------------------------------------------------------------------
// ChatContact/query and ChatContact/queryChanges
// ---------------------------------------------------------------------------

/// Oracle: ChatContact/query on empty backend returns empty ids and position 0.
#[tokio::test]
async fn contact_query_empty() {
    let backend = MemoryBackend::new();
    let (resp, invocations) = handle_contact_query(&backend, json!({ "accountId": "a1" }))
        .await
        .expect("handle_contact_query");

    assert!(invocations.is_empty());
    assert_eq!(resp["accountId"], "a1");
    assert_eq!(resp["ids"], json!([]));
    assert_eq!(resp["position"], 0);
}

/// Oracle: ChatContact/queryChanges without sinceQueryState returns invalidArguments.
#[tokio::test]
async fn contact_query_changes_requires_since_state() {
    let backend = MemoryBackend::new();
    let err = handle_contact_query_changes(&backend, json!({ "accountId": "a1" }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}

// ---------------------------------------------------------------------------
// SpaceInvite
// ---------------------------------------------------------------------------

/// Oracle: SpaceInvite/get on empty backend returns empty list.
#[tokio::test]
async fn invite_get_empty() {
    let backend = MemoryBackend::new();
    let (resp, invocations) =
        handle_invite_get(&backend, json!({ "accountId": "a1", "ids": null }))
            .await
            .expect("handle_invite_get");

    assert!(invocations.is_empty());
    assert_eq!(resp["accountId"], "a1");
    assert_eq!(resp["list"], json!([]));
    assert_eq!(resp["notFound"], json!([]));
}

/// Oracle: SpaceInvite/set create without spaceId returns notCreated invalidProperties.
#[tokio::test]
async fn invite_set_create_missing_space_id() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_invite_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "i0": { "maxUses": 5 } }
        }),
    )
    .await
    .expect("handle_invite_set");

    assert!(resp["notCreated"]["i0"].is_object());
    let props = resp["notCreated"]["i0"]["properties"]
        .as_array()
        .expect("properties");
    assert!(props.iter().any(|p| p == "spaceId"));
}

/// Oracle: SpaceInvite/set create with valid spaceId succeeds and returns a code field.
#[tokio::test]
async fn invite_set_create_success() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_invite_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "i0": { "spaceId": "s1" } }
        }),
    )
    .await
    .expect("handle_invite_set");

    assert!(resp["created"]["i0"].is_object(), "should be in created");
    assert_eq!(resp["created"]["i0"]["spaceId"], "s1");
    let code = resp["created"]["i0"]["code"].as_str().expect("code field");
    assert!(!code.is_empty(), "code must be non-empty");
    // uses starts at 0
    assert_eq!(resp["created"]["i0"]["uses"], 0);
}

/// Oracle: SpaceInvite/set update always returns notUpdated forbidden.
#[tokio::test]
async fn invite_set_update_forbidden() {
    let backend = MemoryBackend::new();

    // Create an invite first so we have a real id.
    let (create_resp, _) = handle_invite_set(
        &backend,
        json!({ "accountId": "a1", "create": { "i0": { "spaceId": "s1" } } }),
    )
    .await
    .expect("create");
    let invite_id = create_resp["created"]["i0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    // Any update attempt must be rejected with forbidden.
    let (resp, _) = handle_invite_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": { &invite_id: { "maxUses": 99 } }
        }),
    )
    .await
    .expect("handle_invite_set");

    assert!(
        resp["notUpdated"][&invite_id].is_object(),
        "update must be rejected"
    );
    assert_eq!(
        resp["notUpdated"][&invite_id]["type"], "forbidden",
        "error type must be forbidden"
    );
}

/// Oracle: SpaceInvite/get returns the invite that was just created.
#[tokio::test]
async fn invite_get_returns_created() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_invite_set(
        &backend,
        json!({ "accountId": "a1", "create": { "i0": { "spaceId": "s2", "maxUses": 10 } } }),
    )
    .await
    .expect("create");
    let invite_id = create_resp["created"]["i0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (resp, _) = handle_invite_get(&backend, json!({ "accountId": "a1", "ids": [invite_id] }))
        .await
        .expect("handle_invite_get");

    let list = resp["list"].as_array().expect("list");
    assert_eq!(list.len(), 1, "one invite should be returned");
    assert_eq!(list[0]["spaceId"], "s2");
    assert_eq!(list[0]["maxUses"], 10);
    assert_eq!(list[0]["uses"], 0);
}

// ---------------------------------------------------------------------------
// CustomEmoji
// ---------------------------------------------------------------------------

/// Oracle: CustomEmoji/get on empty backend returns empty list.
#[tokio::test]
async fn emoji_get_empty() {
    use jmap_chat_server::handle_emoji_get;

    let backend = MemoryBackend::new();
    let (resp, invocations) = handle_emoji_get(&backend, json!({ "accountId": "a1", "ids": null }))
        .await
        .expect("handle_emoji_get");

    assert!(invocations.is_empty());
    assert_eq!(resp["accountId"], "a1");
    assert_eq!(resp["state"], "0");
    assert_eq!(resp["list"], json!([]));
    assert_eq!(resp["notFound"], json!([]));
}

/// Oracle: CustomEmoji/set create without name is rejected with invalidProperties.
#[tokio::test]
async fn emoji_set_create_missing_name() {
    use jmap_chat_server::handle_emoji_set;

    let backend = MemoryBackend::new();
    let (resp, _) = handle_emoji_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "e0": { "blobId": "b1" } }
        }),
    )
    .await
    .expect("handle_emoji_set");

    assert!(resp["notCreated"]["e0"].is_object());
    let props = resp["notCreated"]["e0"]["properties"]
        .as_array()
        .expect("properties");
    assert!(props.iter().any(|p| p == "name"));
}

/// Oracle: CustomEmoji/set create with invalid name (uppercase + punctuation) is rejected.
#[tokio::test]
async fn emoji_set_create_invalid_name() {
    use jmap_chat_server::handle_emoji_set;

    let backend = MemoryBackend::new();
    let (resp, _) = handle_emoji_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "e0": { "name": "UPPERCASE!", "blobId": "b1" } }
        }),
    )
    .await
    .expect("handle_emoji_set");

    assert!(resp["notCreated"]["e0"].is_object());
    assert_eq!(resp["notCreated"]["e0"]["type"], "invalidProperties");
    let props = resp["notCreated"]["e0"]["properties"]
        .as_array()
        .expect("properties");
    assert!(props.iter().any(|p| p == "name"));
}

/// Oracle: CustomEmoji/set create with valid name and blobId succeeds.
#[tokio::test]
async fn emoji_set_create_success() {
    use jmap_chat_server::handle_emoji_set;

    let backend = MemoryBackend::new();
    let (resp, _) = handle_emoji_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "e0": { "name": "party-blob_2", "blobId": "b1" } }
        }),
    )
    .await
    .expect("handle_emoji_set");

    assert!(resp["created"]["e0"].is_object(), "e0 must be in created");
    assert_eq!(resp["notCreated"], json!(null));
    assert_eq!(resp["created"]["e0"]["name"], "party-blob_2");
    assert_ne!(resp["newState"], resp["oldState"], "state must advance");
}

/// Oracle: CustomEmoji/get returns the created emoji.
#[tokio::test]
async fn emoji_get_returns_created() {
    use jmap_chat_server::{handle_emoji_get, handle_emoji_set};

    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_emoji_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "e0": { "name": "rocket", "blobId": "b99" } }
        }),
    )
    .await
    .expect("create");
    let emoji_id = create_resp["created"]["e0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (get_resp, _) = handle_emoji_get(&backend, json!({ "accountId": "a1", "ids": [emoji_id] }))
        .await
        .expect("get");

    assert_eq!(get_resp["list"].as_array().expect("list").len(), 1);
    assert_eq!(get_resp["list"][0]["name"], "rocket");
    assert_eq!(get_resp["list"][0]["blobId"], "b99");
    assert_eq!(get_resp["notFound"], json!([]));
}

// ---------------------------------------------------------------------------
// SpaceBan
// ---------------------------------------------------------------------------

/// Oracle: SpaceBan/get on empty backend returns empty list.
#[tokio::test]
async fn ban_get_empty() {
    let backend = MemoryBackend::new();
    let (resp, invocations) = handle_ban_get(&backend, json!({ "accountId": "a1", "ids": null }))
        .await
        .expect("handle_ban_get");

    assert!(invocations.is_empty());
    assert_eq!(resp["accountId"], "a1");
    assert_eq!(resp["list"], json!([]));
    assert_eq!(resp["notFound"], json!([]));
}

/// Oracle: SpaceBan/set create without spaceId returns notCreated invalidProperties.
#[tokio::test]
async fn ban_set_create_missing_space_id() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_ban_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "b0": { "userId": "u2" } }
        }),
    )
    .await
    .expect("handle_ban_set");

    assert!(resp["notCreated"]["b0"].is_object());
    let props = resp["notCreated"]["b0"]["properties"]
        .as_array()
        .expect("properties");
    assert!(props.iter().any(|p| p == "spaceId"));
}

/// Oracle: SpaceBan/set create without userId returns notCreated invalidProperties.
#[tokio::test]
async fn ban_set_create_missing_user_id() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_ban_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "b0": { "spaceId": "s1" } }
        }),
    )
    .await
    .expect("handle_ban_set");

    assert!(resp["notCreated"]["b0"].is_object());
    let props = resp["notCreated"]["b0"]["properties"]
        .as_array()
        .expect("properties");
    assert!(props.iter().any(|p| p == "userId"));
}

/// Oracle: SpaceBan/set create with valid spaceId and userId succeeds; bannedBy equals accountId.
#[tokio::test]
async fn ban_set_create_success() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_ban_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "b0": { "spaceId": "s1", "userId": "u2" } }
        }),
    )
    .await
    .expect("handle_ban_set");

    assert!(resp["created"]["b0"].is_object(), "should be in created");
    assert_eq!(resp["created"]["b0"]["spaceId"], "s1");
    assert_eq!(resp["created"]["b0"]["userId"], "u2");
    // bannedBy must be the accountId, set server-side.
    assert_eq!(resp["created"]["b0"]["bannedBy"], "a1");
    assert_eq!(resp["notCreated"], json!(null));
}

/// Oracle: SpaceBan/get after create returns the created object.
#[tokio::test]
async fn ban_get_returns_created() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_ban_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "b0": { "spaceId": "s1", "userId": "u3", "reason": "spam" } }
        }),
    )
    .await
    .expect("create");
    let ban_id = create_resp["created"]["b0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (get_resp, _) = handle_ban_get(&backend, json!({ "accountId": "a1", "ids": [ban_id] }))
        .await
        .expect("get");

    assert_eq!(get_resp["list"].as_array().expect("list").len(), 1);
    assert_eq!(get_resp["list"][0]["spaceId"], "s1");
    assert_eq!(get_resp["list"][0]["userId"], "u3");
    assert_eq!(get_resp["list"][0]["reason"], "spam");
    assert_eq!(get_resp["list"][0]["bannedBy"], "a1");
    assert_eq!(get_resp["notFound"], json!([]));
}

// ---------------------------------------------------------------------------
// PresenceStatus
// ---------------------------------------------------------------------------

/// Oracle: PresenceStatus/get on empty backend returns empty list, state "0".
#[tokio::test]
async fn presence_get_empty() {
    let backend = MemoryBackend::new();
    let (resp, invocations) =
        handle_presence_get(&backend, json!({ "accountId": "a1", "ids": null }))
            .await
            .expect("handle_presence_get");

    assert!(invocations.is_empty());
    assert_eq!(resp["accountId"], "a1");
    assert_eq!(resp["state"], "0");
    assert_eq!(resp["list"], json!([]));
    assert_eq!(resp["notFound"], json!([]));
}

/// Oracle: PresenceStatus/set create is always rejected with forbidden.
#[tokio::test]
async fn presence_set_create_forbidden() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_presence_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": {
                "ps0": { "presence": "online", "receiptSharing": true }
            }
        }),
    )
    .await
    .expect("handle_presence_set");

    assert_eq!(
        resp["notCreated"]["ps0"]["type"], "forbidden",
        "create must be forbidden"
    );
    assert_eq!(resp["created"], json!(null));
}

/// Oracle: PresenceStatus/set destroy is always rejected with forbidden.
#[tokio::test]
async fn presence_set_destroy_forbidden() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_presence_set(
        &backend,
        json!({
            "accountId": "a1",
            "destroy": ["ps1"]
        }),
    )
    .await
    .expect("handle_presence_set");

    assert_eq!(
        resp["notDestroyed"]["ps1"]["type"], "forbidden",
        "destroy must be forbidden"
    );
    assert_eq!(resp["destroyed"], json!(null));
}

/// Oracle: PresenceStatus/set update with readonly field `id` returns invalidProperties.
#[tokio::test]
async fn presence_set_update_readonly_id_rejected() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_presence_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": {
                "ps1": { "id": "ps1", "presence": "away" }
            }
        }),
    )
    .await
    .expect("handle_presence_set");

    assert_eq!(
        resp["notUpdated"]["ps1"]["type"], "invalidProperties",
        "id must be rejected as readonly"
    );
}

/// Oracle: PresenceStatus/set update with readonly field `updatedAt` returns invalidProperties.
#[tokio::test]
async fn presence_set_update_readonly_updated_at_rejected() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_presence_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": {
                "ps1": { "updatedAt": "2026-01-01T00:00:00Z", "presence": "away" }
            }
        }),
    )
    .await
    .expect("handle_presence_set");

    assert_eq!(
        resp["notUpdated"]["ps1"]["type"], "invalidProperties",
        "updatedAt must be rejected as readonly"
    );
}

/// Oracle: PresenceStatus/set update on non-existent id returns notUpdated (notFound from backend).
#[tokio::test]
async fn presence_set_update_not_found() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_presence_set(
        &backend,
        json!({
            "accountId": "a1",
            "update": {
                "ps1": { "presence": "away" }
            }
        }),
    )
    .await
    .expect("handle_presence_set");

    // Backend returns notFound SetError for unknown ids.
    assert!(
        resp["notUpdated"]["ps1"].is_object(),
        "non-existent id must be in notUpdated"
    );
    assert_eq!(resp["updated"], json!(null));
}

/// Oracle: PresenceStatus/changes on empty backend returns empty arrays.
#[tokio::test]
async fn presence_changes_empty() {
    let backend = MemoryBackend::new();
    let (resp, invocations) =
        handle_presence_changes(&backend, json!({ "accountId": "a1", "sinceState": "0" }))
            .await
            .expect("handle_presence_changes");

    assert!(invocations.is_empty());
    assert_eq!(resp["accountId"], "a1");
    assert_eq!(resp["oldState"], "0");
    assert_eq!(resp["created"], json!([]));
    assert_eq!(resp["updated"], json!([]));
    assert_eq!(resp["destroyed"], json!([]));
    assert_eq!(resp["hasMoreChanges"], false);
}

/// Oracle: PresenceStatus/changes without sinceState returns invalidArguments.
#[tokio::test]
async fn presence_changes_requires_since_state() {
    let backend = MemoryBackend::new();
    let err = handle_presence_changes(&backend, json!({ "accountId": "a1" }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}

// ---------------------------------------------------------------------------
// Space/join
// ---------------------------------------------------------------------------

/// Oracle: Space/join with neither inviteCode nor spaceId returns invalidArguments.
#[tokio::test]
async fn space_join_requires_exactly_one_arg() {
    let backend = MemoryBackend::new();
    let err = handle_space_join(&backend, json!({ "accountId": "a1" }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}

/// Oracle: Space/join with both inviteCode and spaceId returns invalidArguments.
#[tokio::test]
async fn space_join_both_args_rejected() {
    let backend = MemoryBackend::new();
    let err = handle_space_join(
        &backend,
        json!({
            "accountId": "a1",
            "inviteCode": "abc",
            "spaceId": "s1"
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}

/// Oracle: Space/join with an unknown invite code returns invalidArguments.
#[tokio::test]
async fn space_join_invalid_invite_code() {
    let backend = MemoryBackend::new();
    let err = handle_space_join(
        &backend,
        json!({
            "accountId": "a1",
            "inviteCode": "doesnotexist"
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}

/// Oracle: Space/join with an unknown spaceId returns forbidden.
#[tokio::test]
async fn space_join_public_space_not_found() {
    let backend = MemoryBackend::new();
    let err = handle_space_join(
        &backend,
        json!({
            "accountId": "a1",
            "spaceId": "nosuchspace"
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.as_str(), "forbidden");
}

/// Oracle: Space/join via a valid invite code returns accountId and spaceId,
/// increments the invite's uses count, and adds the caller as a member.
#[tokio::test]
async fn space_join_via_invite_code_success() {
    use jmap_chat_server::handle_invite_get;

    let backend = MemoryBackend::new();

    // Create a real Space so the member-add step has an object to patch.
    let (space_resp, _) = handle_space_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "s0": { "name": "Invite Space" } }
        }),
    )
    .await
    .expect("create Space");
    let space_id = space_resp["created"]["s0"]["id"]
        .as_str()
        .expect("space id")
        .to_owned();

    // Create an invite for that space.
    let (create_resp, _) = handle_invite_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "i0": { "spaceId": space_id } }
        }),
    )
    .await
    .expect("create SpaceInvite");

    let invite_id = create_resp["created"]["i0"]["id"]
        .as_str()
        .expect("invite id")
        .to_owned();
    let code = create_resp["created"]["i0"]["code"]
        .as_str()
        .expect("code")
        .to_owned();

    // Join using the invite code.
    let (resp, invocations) = handle_space_join(
        &backend,
        json!({
            "accountId": "a1",
            "inviteCode": code
        }),
    )
    .await
    .expect("handle_space_join");

    assert!(invocations.is_empty());
    assert_eq!(resp["accountId"], "a1");
    assert_eq!(resp["spaceId"].as_str().expect("spaceId"), space_id);

    // Verify uses was incremented to 1.
    let (invite_list, _) =
        handle_invite_get(&backend, json!({ "accountId": "a1", "ids": [invite_id] }))
            .await
            .expect("handle_invite_get");
    assert_eq!(
        invite_list["list"][0]["uses"], 1,
        "uses must be incremented"
    );

    // Verify caller was added as a member.
    let (space_list, _) =
        handle_space_get(&backend, json!({ "accountId": "a1", "ids": [space_id] }))
            .await
            .expect("handle_space_get");
    let members = space_list["list"][0]["members"]
        .as_array()
        .expect("members array");
    assert_eq!(members.len(), 1, "one member should have been added");
    assert_eq!(members[0]["id"], "a1");
}

/// Oracle: Space/join via an invite with uses >= maxUses returns invalidArguments.
/// An invite with maxUses=1 and uses=1 is pre-exhausted; joining must fail.
#[tokio::test]
async fn space_join_invite_at_max_uses_rejected() {
    use jmap_chat_types::SpaceInvite;
    use jmap_types::UTCDate;

    let backend = MemoryBackend::new();
    let account_id = Id::from("a1");

    // Insert a pre-exhausted invite directly via the backend:
    // uses=1, maxUses=Some(1) → uses >= max → join must be rejected.
    let invite = SpaceInvite::new(
        Id::from("placeholder"),
        "exhaustedcode",
        Id::from("s1"),
        account_id.clone(),
        1,
        UTCDate::from("2026-01-01T00:00:00Z"),
        None,
        None,
        Some(1),
    );
    backend
        .create_object::<SpaceInvite>(&account_id, "i0", invite)
        .await
        .expect("create SpaceInvite");

    let err = handle_space_join(
        &backend,
        json!({
            "accountId": "a1",
            "inviteCode": "exhaustedcode"
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}

/// Oracle: Space/join via spaceId where the space has isPublic=false returns forbidden.
#[tokio::test]
async fn space_join_private_space_forbidden() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "s0": { "name": "Private Space", "isPublic": false } }
        }),
    )
    .await
    .expect("create Space");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let err = handle_space_join(
        &backend,
        json!({
            "accountId": "a1",
            "spaceId": space_id
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.as_str(), "forbidden");
}

/// Oracle: Space/join via spaceId where the space has isPublic=true returns accountId and
/// spaceId.
#[tokio::test]
async fn space_join_public_space_success() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "s0": { "name": "Public Space", "isPublic": true } }
        }),
    )
    .await
    .expect("create Space");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (resp, invocations) = handle_space_join(
        &backend,
        json!({
            "accountId": "a1",
            "spaceId": space_id
        }),
    )
    .await
    .expect("handle_space_join");

    assert!(invocations.is_empty());
    assert_eq!(resp["accountId"], "a1");
    assert_eq!(resp["spaceId"].as_str().expect("spaceId"), space_id);
}

// ---------------------------------------------------------------------------
// Quota / capability limit tests
// ---------------------------------------------------------------------------

/// Oracle: Message/set create with body exceeding 100,000 bytes is rejected.
#[tokio::test]
async fn message_set_create_body_too_large() {
    let backend = MemoryBackend::new();
    let long_body: String = "a".repeat(100_001);
    let (resp, _) = handle_message_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": long_body } }
        }),
    )
    .await
    .expect("handle_message_set");

    assert!(resp["notCreated"]["m0"].is_object());
    let props = resp["notCreated"]["m0"]["properties"]
        .as_array()
        .expect("props");
    assert!(props.iter().any(|p| p == "body"));
    assert_eq!(resp["created"], json!(null));
}

/// Oracle: CustomEmoji/set create with name of 65 chars is rejected.
#[tokio::test]
async fn emoji_set_create_name_too_long() {
    use jmap_chat_server::handle_emoji_set;

    let backend = MemoryBackend::new();
    let long_name: String = "a".repeat(65);
    let (resp, _) = handle_emoji_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "e0": { "name": long_name, "blobId": "b1" } }
        }),
    )
    .await
    .expect("handle_emoji_set");

    assert!(resp["notCreated"]["e0"].is_object());
    let props = resp["notCreated"]["e0"]["properties"]
        .as_array()
        .expect("props");
    assert!(props.iter().any(|p| p == "name"));
    assert_eq!(resp["created"], json!(null));
}

/// Oracle: Space/set create with name of 257 chars is rejected.
#[tokio::test]
async fn space_set_create_name_too_long() {
    let backend = MemoryBackend::new();
    let long_name: String = "a".repeat(257);
    let (resp, _) = handle_space_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "s0": { "name": long_name } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(resp["notCreated"]["s0"].is_object());
    let props = resp["notCreated"]["s0"]["properties"]
        .as_array()
        .expect("props");
    assert!(props.iter().any(|p| p == "name"));
    assert_eq!(resp["created"], json!(null));
}

/// Oracle: SpaceInvite/set create with maxUses=0 is rejected.
#[tokio::test]
async fn invite_set_create_max_uses_zero() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_invite_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "i0": { "spaceId": "s1", "maxUses": 0 } }
        }),
    )
    .await
    .expect("handle_invite_set");

    assert!(resp["notCreated"]["i0"].is_object());
    let props = resp["notCreated"]["i0"]["properties"]
        .as_array()
        .expect("props");
    assert!(props.iter().any(|p| p == "maxUses"));
    assert_eq!(resp["created"], json!(null));
}

/// Oracle: SpaceBan/set create with reason exceeding 1,000 chars is rejected.
#[tokio::test]
async fn ban_set_create_reason_too_long() {
    let backend = MemoryBackend::new();
    let long_reason: String = "a".repeat(1001);
    let (resp, _) = handle_ban_set(
        &backend,
        json!({
            "accountId": "a1",
            "create": { "b0": { "spaceId": "s1", "userId": "u1", "reason": long_reason } }
        }),
    )
    .await
    .expect("handle_ban_set");

    assert!(resp["notCreated"]["b0"].is_object());
    let props = resp["notCreated"]["b0"]["properties"]
        .as_array()
        .expect("props");
    assert!(props.iter().any(|p| p == "reason"));
    assert_eq!(resp["created"], json!(null));
}
