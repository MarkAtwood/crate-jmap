// Integration test entry point for jmap-chat-server.
//
// The common module provides MemoryBackend — an in-memory ChatBackend used
// as the test harness for all handler integration tests.
#![allow(async_fn_in_trait)]

mod common;

use common::{FaultyBackend, MemoryBackend, TrackingBackend};
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
        .get_state::<Chat>(&(), &account_id)
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
        .get_state::<Chat>(&(), &account_id)
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
        .create_object::<Chat>(&(), &account_id, "c0", chat)
        .await
        .expect("create_object");

    let state_after = backend
        .get_state::<Chat>(&(), &account_id)
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
        .create_object::<Chat>(&(), &account_id, "c0", chat)
        .await
        .expect("create_object");

    let (found, not_found) = backend
        .get_objects::<Chat>(
            &(),
            &account_id,
            Some(std::slice::from_ref(&server_id)),
            None,
        )
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
    let account_id = Id::from("a1");
    backend.register_account(&account_id);
    let (resp, invocations) =
        handle_chat_get(&backend, &(), json!({ "accountId": "a1", "ids": null }))
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
    let account_id = Id::from("a1");
    backend.register_account(&account_id);
    let (resp, _) = handle_chat_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": ["missing1"] }),
    )
    .await
    .expect("handle_chat_get");

    assert_eq!(resp["list"], json!([]));
    assert_eq!(resp["notFound"], json!(["missing1"]));
}

/// Oracle: Chat/get without accountId returns invalidArguments error.
#[tokio::test]
async fn chat_get_missing_account_id() {
    let backend = MemoryBackend::new();
    let err = handle_chat_get(&backend, &(), json!({})).await.unwrap_err();
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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

/// Oracle: Chat/set create with a kind value NOT in the spec
/// (draft-atwood-jmap-chat-00 enumerates "direct", "group", "channel")
/// is rejected with invalidProperties on the kind field. The
/// ChatKind::Other(_) deserialize fallback is for round-trip fidelity
/// when READING from a future-version server, not for accepting any
/// client-supplied string on create (bd:JMAP-x2gd.12).
#[tokio::test]
async fn chat_set_create_unknown_kind_rejected() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_chat_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "c0": { "kind": "vampire", "name": "Junk" }
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
    assert!(
        props.iter().any(|p| p == "kind"),
        "expected 'kind' in properties, got {props:?}"
    );
    // And the bogus Chat must not be in storage.
    assert!(resp["created"].as_object().is_none_or(|m| m.is_empty()));
}

/// Oracle: bd:JMAP-wlip.1 — a Chat/set update with a patch nested deeper
/// than `MAX_MERGE_PATCH_DEPTH` (32 levels) MUST be rejected with
/// `invalidPatch` rather than silently truncated. The stored object MUST
/// be unchanged.
///
/// Test vector: a 200-level-deep nested patch on a Chat created via the
/// normal handler path. Pre-fix the call silently succeeded with the
/// deeply-nested field neither stored nor reported as failed; post-fix it
/// returns `invalidPatch` and the original name is preserved.
///
/// The oracle is hand-built — neither the depth (200) nor the assertion
/// values come from the function under test.
#[tokio::test]
async fn chat_set_update_too_deep_patch_rejected_not_silently_truncated() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_chat_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "c0": { "kind": "group", "name": "Original" } }
        }),
    )
    .await
    .expect("create");
    let chat_id = create_resp["created"]["c0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    // Build a 200-level-deep nested patch object. The patch field name
    // is arbitrary — it does not matter for the depth-cap test whether
    // the field exists on Chat; the merge-patch happens against the
    // stored JSON Value before any typed validation.
    const DEPTH: usize = 200;
    let mut patch = json!({ "leaf": "ignored" });
    for _ in 0..DEPTH {
        patch = json!({ "a": patch });
    }

    let (resp, _) = handle_chat_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &chat_id: patch }
        }),
    )
    .await
    .expect("handle_chat_set");

    // Must surface as invalidPatch, NOT silently succeed.
    assert_eq!(
        resp["notUpdated"][&chat_id]["type"], "invalidPatch",
        "deeply-nested patch must surface as invalidPatch, \
         not silently truncate (bd:JMAP-wlip.1): {:?}",
        resp["notUpdated"][&chat_id]
    );
    assert!(
        resp["updated"].get(&chat_id).is_none(),
        "deeply-nested patch must NOT appear in updated: {:?}",
        resp["updated"]
    );

    // Stored object MUST be unchanged — verify by reading back the name.
    let (get_resp, _) = handle_chat_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&chat_id] }),
    )
    .await
    .expect("handle_chat_get");
    assert_eq!(
        get_resp["list"][0]["name"], "Original",
        "stored Chat.name must be unchanged after a rejected too-deep patch: {:?}",
        get_resp["list"][0]
    );
}

/// Oracle: Chat/set update of a server-set field (createdAt) is rejected.
#[tokio::test]
async fn chat_set_update_readonly_field_rejected() {
    let backend = MemoryBackend::new();

    // Create a chat first.
    let (create_resp, _) = handle_chat_set(
        &backend,
        &(),
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
        &(),
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
        &(),
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

    let (destroy_resp, _) = handle_chat_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "destroy": [chat_id] }),
    )
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
        &(),
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
    let account_id = Id::from("a1");
    backend.register_account(&account_id);
    let (resp, _) = handle_chat_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": "0" }),
    )
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
    let account_id = Id::from("a1");
    backend.register_account(&account_id);
    let err = handle_chat_changes(&backend, &(), json!({ "accountId": "a1" }))
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
        &(),
        json!({
            "accountId": "a1",
            "create": { "c0": { "kind": "group", "name": "G" } }
        }),
    )
    .await
    .expect("create");

    let (resp, _) = handle_chat_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": "0" }),
    )
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
    // Register the account so account_exists() returns true (the generic
    // handle_query checks this before querying).
    backend.register_account(&jmap_types::Id::from("a1"));
    let (resp, _) = handle_chat_query(&backend, &(), json!({ "accountId": "a1" }))
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
        &(),
        json!({ "accountId": "a1", "create": { "c0": { "kind": "group", "name": "G" } } }),
    )
    .await
    .expect("create");

    let (resp, _) = handle_chat_query(
        &backend,
        &(),
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
    let account_id = Id::from("a1");
    backend.register_account(&account_id);
    let err = handle_chat_query_changes(&backend, &(), json!({ "accountId": "a1" }))
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
        &(),
        json!({ "accountId": "a1", "create": { "c0": { "kind": "group", "name": "G" } } }),
    )
    .await
    .expect("create");

    let (resp, _) = handle_chat_query_changes(
        &backend,
        &(),
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
// Chat/typing — blocked-sender suppression hook
// (draft-atwood-jmap-chat-00 commit d68b4e3)
// ---------------------------------------------------------------------------

/// Oracle: `Chat/typing` returns a successful response (echoing
/// `accountId`) regardless of whether the requesting account is
/// blocked. The kit's wire shape is identical in either case — the
/// blocked-sender suppression contract is implemented by the
/// consumer's transport layer, which consults
/// `ChatBackend::is_contact_blocked` independently. The kit handler's
/// only job at this site is to validate input and forward.
#[tokio::test]
async fn chat_typing_returns_success_when_contact_not_blocked() {
    use jmap_chat_server::handle_chat_typing;

    let backend = MemoryBackend::new();
    // Seed a direct chat referencing a known contact id.
    backend.insert_object_for_test(
        "Chat",
        "a1",
        "c1",
        json!({
            "id": "c1",
            "kind": "direct",
            "contactId": "u1",
            "createdAt": "2024-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "muted": false,
            "receiveTypingIndicators": true
        }),
    );
    // No ChatContact record — `is_contact_blocked` returns Ok(false).

    let (resp, _) = handle_chat_typing(
        &backend,
        &(),
        json!({ "accountId": "a1", "chatId": "c1", "typing": true }),
    )
    .await
    .expect("handle_chat_typing");

    assert_eq!(resp["accountId"], "a1");
}

/// Oracle: `Chat/typing` returns the same success response when the
/// recipient has marked the sender as blocked. Per the bead and
/// draft-atwood-jmap-chat-00 commit `d68b4e3`, the kit handler's wire
/// response is unchanged; suppression is the consumer transport
/// layer's job (which calls `is_contact_blocked` before each fan-out
/// event).
#[tokio::test]
async fn chat_typing_returns_success_when_contact_blocked() {
    use jmap_chat_server::handle_chat_typing;

    let backend = MemoryBackend::new();
    backend.insert_object_for_test(
        "Chat",
        "a1",
        "c1",
        json!({
            "id": "c1",
            "kind": "direct",
            "contactId": "u1",
            "createdAt": "2024-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "muted": false,
            "receiveTypingIndicators": true
        }),
    );
    backend.insert_object_for_test(
        "ChatContact",
        "a1",
        "u1",
        json!({
            "id": "u1",
            "login": "blocked-user@example.org",
            "firstSeenAt": "2024-01-01T00:00:00Z",
            "lastSeenAt": "2024-01-02T00:00:00Z",
            "blocked": true
        }),
    );

    let (resp, _) = handle_chat_typing(
        &backend,
        &(),
        json!({ "accountId": "a1", "chatId": "c1", "typing": true }),
    )
    .await
    .expect("handle_chat_typing");

    assert_eq!(
        resp["accountId"], "a1",
        "wire response is unchanged when sender is blocked — suppression is consumer-side"
    );
}

/// Oracle: `MemoryBackend::is_contact_blocked` reads
/// `ChatContact.blocked` from the in-memory store. This is the kit's
/// reference impl exercised directly, mirroring the way a consumer's
/// transport layer would consult the predicate before a fan-out.
#[tokio::test]
async fn memory_backend_is_contact_blocked_reads_chat_contact_blocked_field() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("a1");
    let blocked_id = Id::from("blocked-user");
    let allowed_id = Id::from("allowed-user");

    backend.insert_object_for_test(
        "ChatContact",
        "a1",
        "blocked-user",
        json!({
            "id": "blocked-user",
            "login": "b@example.org",
            "firstSeenAt": "2024-01-01T00:00:00Z",
            "lastSeenAt": "2024-01-02T00:00:00Z",
            "blocked": true
        }),
    );
    backend.insert_object_for_test(
        "ChatContact",
        "a1",
        "allowed-user",
        json!({
            "id": "allowed-user",
            "login": "a@example.org",
            "firstSeenAt": "2024-01-01T00:00:00Z",
            "lastSeenAt": "2024-01-02T00:00:00Z",
            "blocked": false
        }),
    );

    assert!(
        backend
            .is_contact_blocked(&(), &account_id, &blocked_id)
            .await
            .expect("is_contact_blocked"),
        "blocked: true contact must report blocked"
    );
    assert!(
        !backend
            .is_contact_blocked(&(), &account_id, &allowed_id)
            .await
            .expect("is_contact_blocked"),
        "blocked: false contact must report not-blocked"
    );

    // Unknown contact id → not blocked (open-by-default for an
    // unrecognised principal).
    let unknown = Id::from("never-seen");
    assert!(
        !backend
            .is_contact_blocked(&(), &account_id, &unknown)
            .await
            .expect("is_contact_blocked"),
        "unknown contact id must report not-blocked"
    );
}

/// Oracle: `handle_chat_typing` reaches the `is_contact_blocked`
/// consultation point on the direct-chat path. Counted via
/// `TrackingBackend::is_contact_blocked_call_count`, since the kit's
/// wire response is unchanged regardless of the predicate's result.
#[tokio::test]
async fn chat_typing_consults_is_contact_blocked_on_direct_chat() {
    use jmap_chat_server::handle_chat_typing;

    let backend = TrackingBackend::new();
    backend.inner().insert_object_for_test(
        "Chat",
        "a1",
        "c1",
        json!({
            "id": "c1",
            "kind": "direct",
            "contactId": "u1",
            "createdAt": "2024-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "muted": false,
            "receiveTypingIndicators": true
        }),
    );

    let (_, _) = handle_chat_typing(
        &backend,
        &(),
        json!({ "accountId": "a1", "chatId": "c1", "typing": true }),
    )
    .await
    .expect("handle_chat_typing");

    assert_eq!(
        backend.is_contact_blocked_call_count(),
        1,
        "handle_chat_typing must consult is_contact_blocked exactly once on a direct chat"
    );
}

/// Oracle: `handle_chat_typing` skips the `is_contact_blocked`
/// consultation for non-direct chats. Group and channel fan-out is
/// per-recipient and lives entirely in the consumer transport layer;
/// the kit handler has no way to enumerate fan-out recipients.
#[tokio::test]
async fn chat_typing_skips_is_contact_blocked_on_group_chat() {
    use jmap_chat_server::handle_chat_typing;

    let backend = TrackingBackend::new();
    backend.inner().insert_object_for_test(
        "Chat",
        "a1",
        "c1",
        json!({
            "id": "c1",
            "kind": "group",
            "createdAt": "2024-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "muted": false,
            "receiveTypingIndicators": true
        }),
    );

    let (_, _) = handle_chat_typing(
        &backend,
        &(),
        json!({ "accountId": "a1", "chatId": "c1", "typing": true }),
    )
    .await
    .expect("handle_chat_typing");

    assert_eq!(
        backend.is_contact_blocked_call_count(),
        0,
        "group/channel chats are skipped — fan-out is consumer-side"
    );
}

/// Oracle: `handle_chat_typing` does not consult
/// `is_contact_blocked` when the supplied `chatId` does not resolve
/// to any stored Chat. A missing target is not a consultation
/// opportunity — the kit returns its standard success response
/// without invoking the predicate.
#[tokio::test]
async fn chat_typing_skips_is_contact_blocked_when_chat_not_found() {
    use jmap_chat_server::handle_chat_typing;

    let backend = TrackingBackend::new();
    // No Chat seeded.

    let (resp, _) = handle_chat_typing(
        &backend,
        &(),
        json!({ "accountId": "a1", "chatId": "ghost", "typing": true }),
    )
    .await
    .expect("handle_chat_typing");

    assert_eq!(resp["accountId"], "a1");
    assert_eq!(
        backend.is_contact_blocked_call_count(),
        0,
        "is_contact_blocked must not be consulted for a non-existent chatId"
    );
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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

    let (get_resp, _) =
        handle_message_get(&backend, &(), json!({ "accountId": "a1", "ids": [msg_id] }))
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
        &(),
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "Hi", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create");

    let (resp, _) = handle_message_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": "0" }),
    )
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
            &(),
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
        &(),
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
        &(),
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
        &(),
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "body": "edited" } }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][&msg_id], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id] }),
    )
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
        &(),
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
        &(),
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "deletedForAll": true } }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][&msg_id], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id] }),
    )
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
        &(),
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
        &(),
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "readAt": read_ts } }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][&msg_id], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id] }),
    )
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
        &(),
        json!({ "accountId": "a1", "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2026-01-01T00:00:00Z" } } }),
    ).await.expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (update_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "update": { msg_id.as_str(): { "readAt": "2026-01-05T10:00:00Z" } } }),
    ).await.expect("update");
    assert_eq!(update_resp["updated"][msg_id.as_str()], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
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
        &(),
        json!({ "accountId": "a1", "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2026-01-01T00:00:00Z" } } }),
    ).await.expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (update_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "update": { msg_id.as_str(): { "readAt": "2026-01-05T10:00:00Z", "readDisposition": "deleted" } } }),
    ).await.expect("update");
    assert_eq!(update_resp["updated"][msg_id.as_str()], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
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
        &(),
        json!({ "accountId": "a1", "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2026-01-01T00:00:00Z" } } }),
    ).await.expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (update_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "update": { msg_id.as_str(): { "readAt": "2026-01-05T10:00:00Z", "readDisposition": "voice-listened" } } }),
    ).await.expect("update");
    assert_eq!(update_resp["updated"][msg_id.as_str()], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
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
        &(),
        json!({ "accountId": "a1", "create": { "m0": { "chatId": "c1", "body": "original", "sentAt": "2026-01-01T00:00:00Z" } } }),
    ).await.expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (update_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "update": { msg_id.as_str(): { "body": "edited text" } } }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][msg_id.as_str()], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
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

// ---------------------------------------------------------------------------
// Burn-on-read (draft-atwood-jmap-chat-00 §Message burnOnRead)
// ---------------------------------------------------------------------------

/// Oracle: draft-atwood-jmap-chat-00 §Message `burnOnRead` — a message created
/// with `burnOnRead: true` MUST be hard-deleted as soon as a `Message/set`
/// update sets `readAt`. The id MUST appear in `destroyed` of subsequent
/// `Message/changes` results (row removal, not tombstone).
#[tokio::test]
async fn message_set_burn_on_read_fires_on_read_at() {
    let backend = MemoryBackend::new();

    // Create a burn-on-read message.
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "this will burn",
                    "sentAt": "2024-01-01T00:00:00Z",
                    "burnOnRead": true
                }
            }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();
    assert_eq!(
        create_resp["created"]["m0"]["burnOnRead"],
        json!(true),
        "burnOnRead must be stored verbatim on create"
    );

    // Mark the message as read. This should fire the burn.
    let (update_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "readAt": "2024-01-02T00:00:00Z" } }
        }),
    )
    .await
    .expect("update");
    assert_eq!(
        update_resp["updated"][&msg_id],
        json!(null),
        "the readAt patch itself succeeds"
    );
    assert_eq!(update_resp["notUpdated"], json!(null));

    // Message/get for the id must report notFound — the row is gone.
    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id] }),
    )
    .await
    .expect("get");
    assert_eq!(
        get_resp["list"].as_array().expect("list").len(),
        0,
        "the burnt message must not be returned by Message/get"
    );
    let not_found = get_resp["notFound"].as_array().expect("notFound");
    assert_eq!(not_found.len(), 1);
    assert_eq!(not_found[0], msg_id);

    // Message/changes from "0" must report the id as destroyed.
    let (changes_resp, _) = handle_message_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": "0" }),
    )
    .await
    .expect("changes");
    let destroyed = changes_resp["destroyed"]
        .as_array()
        .expect("destroyed array");
    assert!(
        destroyed.iter().any(|v| v == &json!(&msg_id)),
        "destroyed must contain the burnt message id (got {destroyed:?})"
    );
}

/// Oracle: an update that does NOT set `readAt` must not fire burn-on-read,
/// even when the message has `burnOnRead: true`. Burn-on-read is tied to the
/// `readAt` event specifically, not to any mutation of a burnable message.
#[tokio::test]
async fn message_set_burn_on_read_no_fire_without_read_at() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "burnable but unread",
                    "sentAt": "2024-01-01T00:00:00Z",
                    "burnOnRead": true
                }
            }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    // Update the body — readAt is NOT in the patch.
    let (update_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "body": "edited body" } }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][&msg_id], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    // Message must still be there with burnOnRead intact.
    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id] }),
    )
    .await
    .expect("get");
    assert_eq!(get_resp["list"].as_array().expect("list").len(), 1);
    assert_eq!(get_resp["list"][0]["body"], "edited body");
    assert_eq!(get_resp["list"][0]["burnOnRead"], json!(true));
    assert_eq!(get_resp["list"][0]["readAt"], json!(null));
}

/// Oracle: a message that is NOT burn-on-read survives `readAt` being set —
/// only the `burnOnRead: true` precondition triggers the hard-delete.
#[tokio::test]
async fn message_set_no_burn_when_burn_on_read_absent() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "not burnable",
                    "sentAt": "2024-01-01T00:00:00Z"
                }
            }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    // Set readAt. burnOnRead is absent, so no burn.
    let (update_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "readAt": "2024-01-02T00:00:00Z" } }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][&msg_id], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    // Message still present with readAt set.
    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id] }),
    )
    .await
    .expect("get");
    assert_eq!(get_resp["list"].as_array().expect("list").len(), 1);
    assert_eq!(get_resp["list"][0]["readAt"], "2024-01-02T00:00:00Z");
}

/// Oracle: a patch that clears `readAt` via `readAt: null` is a PatchObject
/// removal (RFC 8620 §5.3), not a "mark as read" event. Burn-on-read MUST
/// NOT fire on a clear.
#[tokio::test]
async fn message_set_burn_on_read_no_fire_on_read_at_null() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "still here",
                    "sentAt": "2024-01-01T00:00:00Z",
                    "burnOnRead": true
                }
            }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    // PatchObject clear of readAt — readAt has never been set, so this is a
    // no-op on the wire. burnOnRead semantics MUST treat it as not a burn
    // trigger regardless.
    let (update_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "readAt": serde_json::Value::Null } }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][&msg_id], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id] }),
    )
    .await
    .expect("get");
    assert_eq!(get_resp["list"].as_array().expect("list").len(), 1);
    assert_eq!(get_resp["list"][0]["burnOnRead"], json!(true));
}

/// Oracle: `ChatBackend::expire_message` on the reference `MemoryBackend`
/// hard-deletes the message and records the id in `destroyed` of subsequent
/// `Message/changes` results. This is the scheduler-side path for
/// `senderExpiresAt` firing — drivers call this method directly when a
/// timer elapses.
#[tokio::test]
async fn memory_backend_expire_message_direct_call() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "scheduled expiry",
                    "sentAt": "2024-01-01T00:00:00Z"
                }
            }
        }),
    )
    .await
    .expect("create");
    let msg_id_str = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let account_id = Id::from("a1");
    let msg_id = Id::from(msg_id_str.as_str());

    // Direct backend call — simulates a scheduler firing on senderExpiresAt.
    backend
        .expire_message(&(), &account_id, &msg_id)
        .await
        .expect("expire_message must succeed on the reference backend");

    // Message/get for the id must report notFound.
    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id_str] }),
    )
    .await
    .expect("get");
    assert_eq!(get_resp["list"].as_array().expect("list").len(), 0);
    let not_found = get_resp["notFound"].as_array().expect("notFound");
    assert_eq!(not_found.len(), 1);
    assert_eq!(not_found[0], msg_id_str);

    // Message/changes must report the id as destroyed.
    let (changes_resp, _) = handle_message_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": "0" }),
    )
    .await
    .expect("changes");
    let destroyed = changes_resp["destroyed"]
        .as_array()
        .expect("destroyed array");
    assert!(
        destroyed.iter().any(|v| v == &json!(&msg_id_str)),
        "destroyed must contain the expired message id (got {destroyed:?})"
    );
}

/// Oracle: `ChatBackend::expire_message` on a non-existent message id is a
/// no-op success on the reference backend. The contract is idempotent so a
/// scheduler that re-fires after a crash, or a handler whose atomic
/// `update_object` already removed the row, does not produce a spurious
/// SetError::NotFound.
#[tokio::test]
async fn memory_backend_expire_message_idempotent_on_missing() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("a1");
    let missing_id = Id::from("never-existed");

    backend
        .expire_message(&(), &account_id, &missing_id)
        .await
        .expect("expire_message on missing id must be Ok(())");

    // The state token must not advance for a no-op.
    let state_after: jmap_types::State = backend
        .get_state::<jmap_chat_types::Message>(&(), &account_id)
        .await
        .expect("get_state");
    assert_eq!(
        state_after.as_ref(),
        "0",
        "expire_message on missing id must not bump the state counter"
    );
}

// ---------------------------------------------------------------------------
// Burn-on-read serverFail redaction canary (bd:JMAP-x2gd.99)
//
// The bd:JMAP-x2gd.91 fix routed a failed `expire_message` (the spec-mandated
// hard-delete after readAt is set on a burnOnRead message) through
// `server_fail_value_from_backend`, which substitutes the static
// `SERVER_FAIL_INTERNAL_DESC` for the backend's Display text. The previous
// shape leaked the backend error message into the wire `notUpdated[id]`
// description.
//
// This test injects a backend whose `expire_message` returns an error
// carrying the canary literal `INJECTABLE_BACKEND_CANARY`, drives the
// Message/set update that triggers expire_message, and asserts the wire
// response contains no canary anywhere. Mirrors the canonical
// jmap-mail-server `set_per_id_server_fail_redacts_backend_display_*`
// pattern.
// ---------------------------------------------------------------------------

/// Oracle: when `expire_message` fails after a burnOnRead readAt patch
/// lands, the wire-format `notUpdated[id]` description MUST be the
/// static [`jmap_server::SERVER_FAIL_INTERNAL_DESC`] — the backend
/// error's Display text MUST NOT appear in the response.
#[tokio::test]
async fn message_set_burn_on_read_expire_failure_redacts_backend_display() {
    use common::{InjectableBackend, INJECTABLE_BACKEND_CANARY};

    let backend = InjectableBackend::new();
    backend.inner.register_account(&Id::from("a1"));

    // Create a burn-on-read message via the normal handler path. The
    // setup write goes through the inner MemoryBackend with no fault
    // injected.
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "will fail to burn",
                    "sentAt": "2024-01-01T00:00:00Z",
                    "burnOnRead": true
                }
            }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id in created response")
        .to_owned();

    // Inject the fault on expire_message. The readAt patch itself will
    // land (update_object is not injected); the spec-mandated hard-delete
    // hook will fail with a MemoryError whose Display contains the canary.
    backend.inject("Message", "expire");

    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &msg_id: { "readAt": "2024-01-02T00:00:00Z" } }
        }),
    )
    .await
    .expect("update");

    // The canary MUST NOT appear anywhere in the wire response.
    let wire = resp.to_string();
    assert!(
        !wire.contains(INJECTABLE_BACKEND_CANARY),
        "backend-error Display canary must not appear in /set response \
         (burn-on-read expire_message failure Value path must redact); wire: {wire}"
    );

    // Positive control: notUpdated[id] must exist with type=serverFail and
    // description=SERVER_FAIL_INTERNAL_DESC, proving the redaction helper
    // produced the expected wire shape.
    assert_eq!(
        resp["notUpdated"][&msg_id]["type"].as_str(),
        Some("serverFail"),
        "notUpdated[{msg_id}] must have type serverFail; resp: {resp}"
    );
    assert_eq!(
        resp["notUpdated"][&msg_id]["description"].as_str(),
        Some(jmap_server::SERVER_FAIL_INTERNAL_DESC),
        "description must be the static SERVER_FAIL_INTERNAL_DESC; resp: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Edit-history retention gate
// (draft-atwood-jmap-chat-00 commit 0783fc4 + §Message editHistory)
// ---------------------------------------------------------------------------

/// Oracle: with the reference backend's default
/// `retains_edit_history() == false`, `Message/get` MUST omit the
/// `editHistory` field from every returned Message, even when the
/// underlying stored Message carries it. Spec MUST.
#[tokio::test]
async fn message_get_default_omits_edit_history() {
    let backend = MemoryBackend::new();
    backend.insert_object_for_test(
        "Message",
        "a1",
        "m1",
        json!({
            "id": "m1",
            "senderMsgId": "smsg1",
            "senderId": "self",
            "chatId": "c1",
            "body": "edited",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "actions": [],
            "reactions": {},
            "sentAt": "2024-01-01T00:00:00Z",
            "receivedAt": "2024-01-01T00:00:01Z",
            "deliveryState": "delivered",
            "editedAt": "2024-01-02T00:00:00Z",
            "editHistory": [
                {
                    "body": "original",
                    "bodyType": "text/plain",
                    "editedAt": "2024-01-02T00:00:00Z"
                }
            ]
        }),
    );

    let (resp, _) = handle_message_get(&backend, &(), json!({ "accountId": "a1", "ids": ["m1"] }))
        .await
        .expect("handle_message_get");

    let msg = &resp["list"][0];
    assert_eq!(msg["body"], "edited", "the message itself round-trips");
    assert_eq!(
        msg["editHistory"],
        json!(null),
        "editHistory MUST be omitted (default retains_edit_history == false)"
    );
}

/// Oracle: when the backend reports `retains_edit_history() == true`,
/// `Message/get` returns the stored `editHistory` verbatim.
#[tokio::test]
async fn message_get_with_retention_returns_edit_history() {
    let backend = MemoryBackend::new();
    backend.set_retains_edit_history_for_test(true);
    backend.insert_object_for_test(
        "Message",
        "a1",
        "m1",
        json!({
            "id": "m1",
            "senderMsgId": "smsg1",
            "senderId": "self",
            "chatId": "c1",
            "body": "edited",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "actions": [],
            "reactions": {},
            "sentAt": "2024-01-01T00:00:00Z",
            "receivedAt": "2024-01-01T00:00:01Z",
            "deliveryState": "delivered",
            "editedAt": "2024-01-02T00:00:00Z",
            "editHistory": [
                {
                    "body": "original",
                    "bodyType": "text/plain",
                    "editedAt": "2024-01-02T00:00:00Z"
                }
            ]
        }),
    );

    let (resp, _) = handle_message_get(&backend, &(), json!({ "accountId": "a1", "ids": ["m1"] }))
        .await
        .expect("handle_message_get");

    let history = resp["list"][0]["editHistory"]
        .as_array()
        .expect("editHistory must be an array when retention is on");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["body"], "original");
    assert_eq!(history[0]["editedAt"], "2024-01-02T00:00:00Z");
}

/// Oracle: a client that explicitly asks for `editHistory` via the
/// `properties` filter MUST still see it omitted when the backend
/// reports `retains_edit_history() == false`. The spec MUST overrides
/// the client's properties selector.
#[tokio::test]
async fn message_get_properties_does_not_force_edit_history_when_retention_off() {
    let backend = MemoryBackend::new();
    backend.insert_object_for_test(
        "Message",
        "a1",
        "m1",
        json!({
            "id": "m1",
            "senderMsgId": "smsg1",
            "senderId": "self",
            "chatId": "c1",
            "body": "edited",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "actions": [],
            "reactions": {},
            "sentAt": "2024-01-01T00:00:00Z",
            "receivedAt": "2024-01-01T00:00:01Z",
            "deliveryState": "delivered",
            "editedAt": "2024-01-02T00:00:00Z",
            "editHistory": [
                {
                    "body": "original",
                    "bodyType": "text/plain",
                    "editedAt": "2024-01-02T00:00:00Z"
                }
            ]
        }),
    );

    let (resp, _) = handle_message_get(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "ids": ["m1"],
            "properties": ["editHistory"]
        }),
    )
    .await
    .expect("handle_message_get");

    let msg = &resp["list"][0];
    assert_eq!(
        msg["editHistory"],
        json!(null),
        "editHistory MUST be omitted even when the client explicitly asks for it"
    );
}

/// Oracle: `ChatBackend::retains_edit_history()` default impl returns
/// `false`. This is the workspace's "kit defines the hook; consumer
/// enforces the policy" posture — the default is conservative
/// (no retention) and production backends opt in.
#[tokio::test]
async fn memory_backend_retains_edit_history_default_is_false() {
    let backend = MemoryBackend::new();
    assert!(
        !backend.retains_edit_history(),
        "default MemoryBackend must report retains_edit_history() == false"
    );
}

// ---------------------------------------------------------------------------
// Slow-mode rate-limit gate (draft-atwood-jmap-chat-00 §Chat slowModeSeconds
// + spec commit de60acb)
// ---------------------------------------------------------------------------

/// Oracle: the reference `MemoryBackend` has no rate-tracker; every
/// `Message/set` create succeeds regardless of `slowModeSeconds`. The kit
/// defines the hook, the consumer enforces the throttle policy.
#[tokio::test]
async fn message_set_create_no_slow_mode_on_memory_backend() {
    let backend = MemoryBackend::new();

    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "send fast, send often",
                    "sentAt": "2024-01-01T00:00:00Z"
                }
            }
        }),
    )
    .await
    .expect("handle_message_set");

    assert!(
        resp["created"]["m0"].is_object(),
        "MemoryBackend's default slow_mode_check is Ok(()); create must succeed"
    );
    assert_eq!(resp["notCreated"], json!(null));
}

/// Oracle: when `ChatBackend::slow_mode_check` returns
/// `Err(SlowModeError)`, the `Message/set` create handler MUST reject the
/// create with a `rateLimited` SetError carrying `serverRetryAfter` set to
/// the backend-supplied UTCDate, per draft-atwood-jmap-chat-00 §Chat
/// `slowModeSeconds` + commit `de60acb`. The wire field name
/// `serverRetryAfter` is the workspace convention read by
/// `jmap_chat_client::server_retry_after`.
///
/// The error type wire string is exactly `"rateLimited"` (past tense, with
/// `d`) — NOT `"rateLimit"` from `jmap_server::SetErrorType::RateLimit`.
#[tokio::test]
async fn message_set_create_slow_mode_rejects_with_retry_after() {
    // Backend-supplied retry-after — hardcoded to a far-future UTCDate
    // so the test does not depend on wall-clock arithmetic.
    let retry_after = jmap_types::UTCDate::from("2099-12-31T00:00:00Z");
    let backend = TrackingBackend::with_slow_mode_blocking(retry_after.clone());

    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "throttled",
                    "sentAt": "2024-01-01T00:00:00Z"
                }
            }
        }),
    )
    .await
    .expect("handle_message_set");

    assert_eq!(
        resp["created"],
        json!(null),
        "create map must be absent (or null) when no entry succeeded"
    );

    let not_created_m0 = &resp["notCreated"]["m0"];
    assert!(
        not_created_m0.is_object(),
        "m0 must appear in notCreated (got {resp})"
    );
    assert_eq!(
        not_created_m0["type"], "rateLimited",
        "SetError.type must be exactly \"rateLimited\" (spec wire string, not the workspace \"rateLimit\" variant)"
    );
    assert_eq!(
        not_created_m0["serverRetryAfter"],
        retry_after.as_ref(),
        "serverRetryAfter must equal the UTCDate returned by slow_mode_check"
    );
}

/// Oracle: a `TrackingBackend` with no slow-mode block configured falls
/// through to the wrapped `MemoryBackend`'s default no-op
/// `slow_mode_check`. This is the control case for the rejection test
/// above — it verifies the wrapper is not silently injecting a throttle.
#[tokio::test]
async fn message_set_create_tracking_backend_default_allows() {
    let backend = TrackingBackend::new();

    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "no throttle configured",
                    "sentAt": "2024-01-01T00:00:00Z"
                }
            }
        }),
    )
    .await
    .expect("handle_message_set");

    assert!(
        resp["created"]["m0"].is_object(),
        "TrackingBackend without slow_mode_block must forward to MemoryBackend (Ok(()))"
    );
    assert_eq!(resp["notCreated"], json!(null));
}

/// Oracle: slow-mode check happens AFTER wire validation. A malformed
/// create that fails validation (e.g., missing `chatId`) MUST surface
/// `invalidProperties`, NOT `rateLimited` — even if the configured
/// backend would have throttled. Validation rejections never consume a
/// rate-tracker slot.
#[tokio::test]
async fn message_set_create_slow_mode_skipped_when_validation_fails() {
    let retry_after = jmap_types::UTCDate::from("2099-12-31T00:00:00Z");
    let backend = TrackingBackend::with_slow_mode_blocking(retry_after);

    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    // chatId deliberately missing
                    "body": "throttled but also malformed",
                    "sentAt": "2024-01-01T00:00:00Z"
                }
            }
        }),
    )
    .await
    .expect("handle_message_set");

    let not_created_m0 = &resp["notCreated"]["m0"];
    assert_eq!(
        not_created_m0["type"], "invalidProperties",
        "validation must short-circuit before slow_mode_check is consulted"
    );
    assert_eq!(
        not_created_m0["serverRetryAfter"],
        json!(null),
        "serverRetryAfter must be absent on a validation failure"
    );
}

/// Oracle: Message/set create with replyTo set stores and returns the field.
#[tokio::test]
async fn message_set_create_with_reply_to() {
    let backend = MemoryBackend::new();

    // Create the message that will be replied to.
    let (first_resp, _) = handle_message_set(
        &backend,
        &(),
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
        &(),
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

/// Oracle: Message/set create with a malformed senderExpiresAt (not RFC 8620
/// §1.4 UTCDate form) is rejected with invalidProperties: ["senderExpiresAt"].
///
/// The wire format is YYYY-MM-DDTHH:MM:SSZ (exactly 20 chars, 'Z' suffix).
/// A non-conforming value such as "tomorrow" must produce invalidProperties
/// rather than silently flowing into a downstream string compare with
/// undefined ordering semantics.
#[tokio::test]
async fn message_set_create_with_malformed_expiry_rejected() {
    let backend = MemoryBackend::new();

    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "expires sometime",
                    "sentAt": "2024-01-01T00:00:00Z",
                    "senderExpiresAt": "tomorrow"
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

/// Oracle: Message/set create with a past senderExpiresAt is rejected with
/// invalidProperties: ["senderExpiresAt"].
#[tokio::test]
async fn message_set_create_with_past_expiry_rejected() {
    let backend = MemoryBackend::new();

    let (resp, _) = handle_message_set(
        &backend,
        &(),
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

/// Oracle: Message/set create with a malformed `sentAt` wire shape
/// (not the 20-char `YYYY-MM-DDTHH:MM:SSZ` UTCDate per RFC 8620 §1.4)
/// is rejected with `invalidProperties: ["sentAt"]`. Regression
/// bead bd:JMAP-x2gd.7. Before that bead, the handler wrapped the
/// client string via `UTCDate::from` and let any value flow through
/// to storage where lex-compares and 19-byte slices in
/// `helpers::iso8601_before` assume a validated shape.
#[tokio::test]
async fn message_set_create_with_malformed_sent_at_rejected() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "ok",
                    "sentAt": "yesterday"
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
        props.iter().any(|p| p == "sentAt"),
        "sentAt must be listed in rejected properties: {props:?}"
    );
}

/// Oracle: ReadPosition/set create with a malformed `lastReadAt`
/// wire shape is rejected with `invalidProperties: ["lastReadAt"]`.
/// Regression bead bd:JMAP-x2gd.7. Same exposure as the Message/set
/// `sentAt` site — `UTCDate::from` accepted any string and the
/// malformed value flowed through to storage.
#[tokio::test]
async fn position_set_create_with_malformed_last_read_at_rejected() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_position_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "rp0": {
                    "chatId": "c1",
                    "lastReadAt": "not-a-date"
                }
            }
        }),
    )
    .await
    .expect("handle_position_set");

    assert!(resp["created"].is_null());
    assert!(resp["notCreated"]["rp0"].is_object());
    assert_eq!(resp["notCreated"]["rp0"]["type"], "invalidProperties");
    let props = resp["notCreated"]["rp0"]["properties"]
        .as_array()
        .expect("properties array");
    assert!(
        props.iter().any(|p| p == "lastReadAt"),
        "lastReadAt must be listed in rejected properties: {props:?}"
    );
}

// ---------------------------------------------------------------------------
// Reaction patches on Message/set update
// (draft-atwood-jmap-chat-00 §Message/set, §Reaction)
// ---------------------------------------------------------------------------

/// Oracle: a `reactions/{senderReactionId}` patch with an emoji-only
/// object adds a new Reaction whose stored `senderId` is `"self"`
/// (`SenderId::Owner`). The client supplies just `emoji`; the server
/// injects `senderId` and `sentAt`.
#[tokio::test]
async fn message_set_update_reaction_add_overrides_sender_id_to_self() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "react to me", "sentAt": "2024-01-01T00:00:00Z" } }
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
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &msg_id: { "reactions/abc": { "emoji": "👍" } }
            }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["updated"][&msg_id], json!(null));
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id] }),
    )
    .await
    .expect("get");
    let stored = &get_resp["list"][0]["reactions"]["abc"];
    assert!(
        stored.is_object(),
        "reactions.abc must be stored as an object"
    );
    assert_eq!(stored["emoji"], "👍");
    assert_eq!(
        stored["senderId"], "self",
        "server MUST set senderId=\"self\" on owner-authored reactions"
    );
    assert!(
        stored["sentAt"].is_string(),
        "server MUST inject sentAt when client omits it"
    );
}

/// Oracle: when the client supplies an explicit `senderId` on a
/// reaction patch value, the server MUST override it to `"self"` —
/// defense in depth per draft-atwood-jmap-chat-00 §Reaction.
#[tokio::test]
async fn message_set_update_reaction_add_overrides_client_supplied_sender_id() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "test", "sentAt": "2024-01-01T00:00:00Z" } }
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
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &msg_id: { "reactions/bar": { "emoji": "❤️", "senderId": "someone-else" } }
            }
        }),
    )
    .await
    .expect("update");
    assert_eq!(update_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id] }),
    )
    .await
    .expect("get");
    assert_eq!(
        get_resp["list"][0]["reactions"]["bar"]["senderId"], "self",
        "client-supplied senderId MUST be overridden to \"self\""
    );
}

/// Oracle: removing a reaction authored by someone other than the
/// caller MUST be rejected with a `forbidden` SetError. Spec MUST.
#[tokio::test]
async fn message_set_update_reaction_remove_others_rejected_forbidden() {
    let backend = MemoryBackend::new();
    // Seed a Message with a pre-existing reaction whose senderId is
    // NOT "self" — simulates a reaction authored by a peer.
    backend.insert_object_for_test(
        "Message",
        "a1",
        "m1",
        json!({
            "id": "m1",
            "senderMsgId": "smsg1",
            "senderId": "self",
            "chatId": "c1",
            "body": "Hello",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "actions": [],
            "reactions": {
                "foo": {
                    "emoji": "👎",
                    "senderId": "someone-else",
                    "sentAt": "2024-01-01T00:00:00Z"
                }
            },
            "sentAt": "2024-01-01T00:00:00Z",
            "receivedAt": "2024-01-01T00:00:01Z",
            "deliveryState": "delivered"
        }),
    );

    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                "m1": { "reactions/foo": serde_json::Value::Null }
            }
        }),
    )
    .await
    .expect("handle_message_set");

    assert_eq!(resp["updated"], json!(null));
    let nu = &resp["notUpdated"]["m1"];
    assert!(nu.is_object());
    assert_eq!(nu["type"], "forbidden");
    assert!(
        nu["description"]
            .as_str()
            .is_some_and(|s| s.contains("foo")),
        "description must name the rejected reaction key"
    );

    // The reaction must still be present after the forbidden rejection
    // (the patch should not have been applied).
    let (get_resp, _) =
        handle_message_get(&backend, &(), json!({ "accountId": "a1", "ids": ["m1"] }))
            .await
            .expect("get");
    assert_eq!(
        get_resp["list"][0]["reactions"]["foo"]["emoji"], "👎",
        "the foreign reaction must survive the forbidden rejection unchanged"
    );
}

/// Oracle: modifying (rather than removing) a reaction authored by
/// someone else is also rejected with `forbidden`. Only the original
/// sender may modify their own reactions.
#[tokio::test]
async fn message_set_update_reaction_modify_others_rejected_forbidden() {
    let backend = MemoryBackend::new();
    backend.insert_object_for_test(
        "Message",
        "a1",
        "m1",
        json!({
            "id": "m1",
            "senderMsgId": "smsg1",
            "senderId": "self",
            "chatId": "c1",
            "body": "Hello",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "actions": [],
            "reactions": {
                "foo": {
                    "emoji": "👎",
                    "senderId": "someone-else",
                    "sentAt": "2024-01-01T00:00:00Z"
                }
            },
            "sentAt": "2024-01-01T00:00:00Z",
            "receivedAt": "2024-01-01T00:00:01Z",
            "deliveryState": "delivered"
        }),
    );

    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                "m1": { "reactions/foo": { "emoji": "🎉" } }
            }
        }),
    )
    .await
    .expect("handle_message_set");

    assert_eq!(resp["notUpdated"]["m1"]["type"], "forbidden");
}

/// Oracle: a `reactions/{id}` key containing `/` or `~` is rejected
/// with `invalidPatch`. RFC 6901 reserves these as JSON Pointer
/// escape characters; the chat-client also rejects them.
#[tokio::test]
async fn message_set_update_reaction_pointer_with_slash_rejected() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "test", "sentAt": "2024-01-01T00:00:00Z" } }
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
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &msg_id: { "reactions/a/b": { "emoji": "👍" } }
            }
        }),
    )
    .await
    .expect("update");

    assert_eq!(update_resp["notUpdated"][&msg_id]["type"], "invalidPatch");
}

/// Oracle: a non-null non-object value at `reactions/{id}` is
/// rejected with `invalidPatch`. The Reaction wire shape requires an
/// object; a bare string / number / boolean is malformed.
#[tokio::test]
async fn message_set_update_reaction_non_object_value_rejected() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "test", "sentAt": "2024-01-01T00:00:00Z" } }
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
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &msg_id: { "reactions/abc": "👍" }
            }
        }),
    )
    .await
    .expect("update");

    assert_eq!(update_resp["notUpdated"][&msg_id]["type"], "invalidPatch");
}

/// Oracle: removing one's own reaction (sender_id == "self")
/// succeeds. The reaction is removed from the stored Message.
#[tokio::test]
async fn message_set_update_reaction_remove_self_succeeds() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "test", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create");
    let msg_id = create_resp["created"]["m0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    // Add a reaction.
    let (add_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &msg_id: { "reactions/r1": { "emoji": "👍" } }
            }
        }),
    )
    .await
    .expect("add");
    assert_eq!(add_resp["notUpdated"], json!(null));

    // Now remove it.
    let (remove_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &msg_id: { "reactions/r1": serde_json::Value::Null }
            }
        }),
    )
    .await
    .expect("remove");
    assert_eq!(remove_resp["notUpdated"], json!(null));

    let (get_resp, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&msg_id] }),
    )
    .await
    .expect("get");
    assert!(
        get_resp["list"][0]["reactions"].get("r1").is_none()
            || get_resp["list"][0]["reactions"]["r1"].is_null(),
        "removed reaction must not appear in stored reactions"
    );
}

/// Oracle: a patch combining `reactions/{id}` entries with a
/// top-level `reactions` entry is rejected with `invalidPatch`. The
/// two have incompatible semantics (wholesale replace vs per-key
/// merge) and a single patch must not attempt both.
#[tokio::test]
async fn message_set_update_reaction_mixed_top_level_and_pointer_rejected() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "m0": { "chatId": "c1", "body": "test", "sentAt": "2024-01-01T00:00:00Z" } }
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
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &msg_id: {
                    "reactions": {},
                    "reactions/abc": { "emoji": "👍" }
                }
            }
        }),
    )
    .await
    .expect("update");

    assert_eq!(update_resp["notUpdated"][&msg_id]["type"], "invalidPatch");
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
        &(),
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
        &(),
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
        &(),
        json!({ "accountId": "a1", "create": { "s0": { "name": "The Space" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [space_id] }),
    )
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
        &(),
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
            &(),
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

/// Oracle: Space/set update accepts well-formed Role/Member structural
/// mutation keys and dispatches them through
/// ChatBackend::apply_space_patch (bd:JMAP-g7wu.2.4.2 +
/// bd:JMAP-g7wu.2.4.3 implementation).
///
/// Each variant's per-op semantics are exercised in detail by the
/// `space_set_role_*` / `space_set_member_*` tests further down. This
/// test is the cross-variant smoke check: every Role/Member wire key
/// reaches the backend successfully through the parsing layer.
///
/// MemoryBackend's `CallerCtx = ()` puts the backend in single-user
/// mode (criterion 7 of bd:JMAP-g7wu.2.4.3): identity-dependent
/// gates are skipped, so AddRole / AddMember succeed unconditionally
/// and Remove/Update against non-existent ids surface as per-op
/// NotFound rather than Forbidden.
#[tokio::test]
async fn space_set_update_role_member_variants_dispatch_to_backend() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "create": { "s0": { "name": "Mutation Test" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    // AddRole then references the assigned id on the subsequent ops.
    let (add_role_resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "addRoles": [
                { "id": "placeholder", "name": "Mod", "permissions": ["chat:read"], "position": 1 }
            ]}}
        }),
    )
    .await
    .expect("handle_space_set (addRoles)");
    assert!(
        add_role_resp["updated"][&space_id].is_object()
            || add_role_resp["updated"][&space_id].is_null(),
        "addRoles should land in `updated`, got {add_role_resp:?}"
    );
    assert!(
        add_role_resp["notUpdated"][&space_id].is_null(),
        "addRoles should NOT produce notUpdated in single-user mode: {add_role_resp:?}"
    );

    // Discover the server-assigned RoleId via Space/get.
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("Space/get");
    let role_id = get_resp["list"][0]["roles"][0]["id"]
        .as_str()
        .expect("server-assigned role id")
        .to_owned();

    // UpdateRole on the assigned id succeeds.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "updateRoles": [
                { "id": role_id, "name": "Renamed" }
            ]}}
        }),
    )
    .await
    .expect("handle_space_set (updateRoles)");
    assert!(
        resp["notUpdated"][&space_id].is_null(),
        "updateRoles on existing id should succeed: {resp:?}"
    );

    // AddMember with a valid roleIds reference succeeds.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "addMembers": [
                { "id": "u1", "roleIds": [role_id] }
            ]}}
        }),
    )
    .await
    .expect("handle_space_set (addMembers)");
    assert!(
        resp["notUpdated"][&space_id].is_null(),
        "addMembers should succeed in single-user mode: {resp:?}"
    );

    // UpdateMember nick: succeeds.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "updateMembers": [
                { "id": "u1", "nick": "Mark" }
            ]}}
        }),
    )
    .await
    .expect("handle_space_set (updateMembers)");
    assert!(
        resp["notUpdated"][&space_id].is_null(),
        "updateMembers should succeed: {resp:?}"
    );

    // RemoveMember on the freshly-added user: succeeds.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "removeMembers": ["u1"] }}
        }),
    )
    .await
    .expect("handle_space_set (removeMembers)");
    assert!(
        resp["notUpdated"][&space_id].is_null(),
        "removeMembers on existing member should succeed: {resp:?}"
    );

    // RemoveRole on the assigned id: succeeds.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "removeRoles": [role_id] }}
        }),
    )
    .await
    .expect("handle_space_set (removeRoles)");
    assert!(
        resp["notUpdated"][&space_id].is_null(),
        "removeRoles on existing id should succeed: {resp:?}"
    );

    // Removing a non-existent role id surfaces as NotFound, not Forbidden.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "removeRoles": ["nonexistent"] }}
        }),
    )
    .await
    .expect("handle_space_set (removeRoles nonexistent)");
    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "notFound",
        "removeRoles on missing id should be NotFound: {resp:?}"
    );
}

/// Oracle: Space/set update with an empty structural array is treated as
/// an empty patch (no fields to apply) and rejected as `invalidPatch`.
#[tokio::test]
async fn space_set_update_structural_empty_array_is_empty_patch() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "create": { "s0": { "name": "Empty Test" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "addRoles": [] } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "invalidPatch",
        "empty structural array with no other fields is an empty patch"
    );
}

/// Oracle: Space/set update with malformed structural entries is rejected
/// at the parsing layer with `invalidProperties` naming the offending key.
#[tokio::test]
async fn space_set_update_malformed_structural_rejected() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "create": { "s0": { "name": "Malformed Test" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    // addRoles wants an array of objects; a bare string is malformed.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "addRoles": ["not-an-object"] } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "invalidProperties",
        "malformed entry must produce invalidProperties, not forbidden: {:?}",
        resp["notUpdated"][&space_id]
    );
    assert_eq!(resp["notUpdated"][&space_id]["properties"][0], "addRoles");

    // Whole structural payload not an array → invalidProperties.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "removeMembers": "not-an-array" } }
        }),
    )
    .await
    .expect("handle_space_set");
    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "invalidProperties",
        "non-array structural payload must be invalidProperties: {:?}",
        resp["notUpdated"][&space_id]
    );
    assert_eq!(
        resp["notUpdated"][&space_id]["properties"][0],
        "removeMembers"
    );
}

/// Oracle: Space/set addRoles with `position: 0` is rejected as
/// `invalidProperties` per draft-atwood-jmap-chat-00 §SpaceRole commit
/// `c3ea5d9` — position 0 is reserved for the implicit @everyone role.
#[tokio::test]
async fn space_set_add_roles_position_zero_rejected() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "create": { "s0": { "name": "Test" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addRoles": [{
                        "id": "placeholder",
                        "name": "BadRole",
                        "permissions": [],
                        "position": 0
                    }]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    let nu = &resp["notUpdated"][&space_id];
    assert!(nu.is_object(), "{} must appear in notUpdated", &space_id);
    assert_eq!(
        nu["type"], "invalidProperties",
        "wire type must be invalidProperties (the only SetError that carries a `properties` array)"
    );
    let props = nu["properties"].as_array().expect("properties array");
    assert!(
        props.iter().any(|p| p == "position"),
        "properties must include `position`, got {props:?}"
    );
    assert!(
        nu["description"]
            .as_str()
            .is_some_and(|s| s.contains("@everyone")),
        "description must mention the @everyone reservation, got {nu:?}"
    );
}

/// Oracle: Space/set addRoles with `position: 1` (the smallest permitted
/// value) passes the handler's position-zero check and reaches the
/// backend. The reference `MemoryBackend` currently rejects every
/// AddRole with `forbidden` (tracked under bd:JMAP-g7wu.2.4.3), so the
/// wire response is `forbidden` — NOT `invalidProperties` with
/// properties=["position"]. The wire-shape difference is the proof that
/// the handler-level position check did not fire.
#[tokio::test]
async fn space_set_add_roles_position_one_passes_handler_check() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "create": { "s0": { "name": "Test" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addRoles": [{
                        "id": "placeholder",
                        "name": "Mod",
                        "permissions": [],
                        "position": 1
                    }]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    let nu = &resp["notUpdated"][&space_id];
    // Either the response is the backend's forbidden (proving the
    // handler accepted position=1 and dispatched), or it would be
    // invalidProperties with properties=["position"] (proving the
    // handler-level check rejected). The latter would be the bug.
    assert_ne!(
        nu["type"], "invalidProperties",
        "position=1 must NOT be rejected by the handler-level position check"
    );
    let props = nu["properties"].as_array();
    assert!(
        !props.is_some_and(|arr| arr.iter().any(|p| p == "position")),
        "the rejection must not list `position` as the offending field"
    );
}

/// Oracle: Space/set updateRoles with a patch setting `position: 0` is
/// rejected as `invalidProperties`. The validation applies to update
/// patches just as it does to add entries (spec MUST).
#[tokio::test]
async fn space_set_update_roles_position_zero_rejected() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "create": { "s0": { "name": "Test" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "updateRoles": [{
                        "id": "some-role-id",
                        "position": 0
                    }]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    let nu = &resp["notUpdated"][&space_id];
    assert!(nu.is_object());
    assert_eq!(nu["type"], "invalidProperties");
    assert_eq!(nu["properties"][0], "position");
}

/// Oracle: a single position-0 violation rejects the whole update target
/// per RFC 8620 §5.3 per-target atomicity. Even if other ops in the same
/// `addRoles` entry are valid, the entire update target lands in
/// `notUpdated`.
#[tokio::test]
async fn space_set_add_roles_position_zero_rejects_whole_target_atomically() {
    let backend = MemoryBackend::new();
    let (create_resp, _) = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "create": { "s0": { "name": "Atomic" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "name": "RenamedAttempt",
                    "addRoles": [
                        { "id": "p1", "name": "Mod", "permissions": [], "position": 1 },
                        { "id": "p2", "name": "Bad", "permissions": [], "position": 0 }
                    ]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "invalidProperties",
        "whole-target atomicity: one position-0 violation rejects the entire update"
    );

    // The metadata rename must NOT have applied (per-target atomicity).
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("handle_space_get");
    assert_eq!(
        get_resp["list"][0]["name"], "Atomic",
        "structural failure must abort the metadata write"
    );
}

/// Oracle: Space/set update with structural ops AND metadata in the same
/// patch dispatches structural to the backend; metadata is skipped when
/// any structural op fails (per RFC 8620 §5.3 per-target atomicity).
///
/// Uses `updateRoles` against a non-existent role id to deterministically
/// trigger a per-op NotFound from the backend without depending on any
/// identity-gated rejection (MemoryBackend's single-user mode allows all
/// identity-checked Role/Member ops).
#[tokio::test]
async fn space_set_update_mixed_structural_and_metadata_partial_fail() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        &(),
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
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "name": "Renamed",
                    "updateRoles": [{ "id": "nonexistent", "name": "Mod" }],
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    // The structural updateRoles op surfaces NotFound (no such role);
    // per-target atomicity means the mixed-patch target lands in
    // `notUpdated` with that error and the metadata rename is NOT
    // applied.
    assert_eq!(resp["notUpdated"][&space_id]["type"], "notFound");

    // Re-fetch and confirm the name was NOT changed.
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("handle_space_get");
    assert_eq!(
        get_resp["list"][0]["name"], "Original Name",
        "structural failure must abort the metadata write — per-target atomicity"
    );
}

/// Oracle: Space/set update with an unknown property alongside valid keys
/// is rejected as `invalidProperties` naming the unknown key.
#[tokio::test]
async fn space_set_update_unknown_property_rejected() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "create": { "s0": { "name": "Unknown Property Test" } } }),
    )
    .await
    .expect("create");
    let space_id = create_resp["created"]["s0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "name": "OK", "totallyUnknownProperty": 42 } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "invalidProperties",
        "unknown property must be rejected as invalidProperties: {:?}",
        resp["notUpdated"][&space_id]
    );
    let props: Vec<&str> = resp["notUpdated"][&space_id]["properties"]
        .as_array()
        .expect("properties array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        props.contains(&"totallyUnknownProperty"),
        "properties must name the unknown key: {props:?}"
    );
}

// ---------------------------------------------------------------------------
// Space/set Category-family mutations (bd:JMAP-g7wu.2.4.5)
//
// These cover the three Category variants of SpacePatchOp:
// AddCategory, RemoveCategory, UpdateCategory. Channel-reassignment
// cascade semantics (channels move to uncategorizedChannelIds on
// category removal; channel.category_id stays consistent with the
// Space-side categories[].channel_ids on add/update) are exercised by
// driving Chat/set channels alongside the category ops.
// ---------------------------------------------------------------------------

/// Helper: create a Space and return its server-assigned id.
async fn make_space(backend: &MemoryBackend, name: &str) -> String {
    let (resp, _) = handle_space_set(
        backend,
        &(),
        json!({ "accountId": "a1", "create": { "s0": { "name": name } } }),
    )
    .await
    .expect("create space");
    resp["created"]["s0"]["id"]
        .as_str()
        .expect("space id")
        .to_owned()
}

/// Helper: directly seed a channel-kind Chat in the in-memory backend so
/// Category cascade tests have channels to reassign without needing a
/// Chat/set channel-create path (which is part of bd:JMAP-g7wu.2.4.4 and
/// not yet implemented).
fn seed_channel(
    backend: &MemoryBackend,
    account_id: &str,
    chat_id: &str,
    space_id: &str,
    category_id: Option<&str>,
) {
    use jmap_chat_server::JmapObject;
    let mut chat = json!({
        "id": chat_id,
        "kind": "channel",
        "muted": false,
        "receiveTypingIndicators": true,
        "spaceId": space_id,
    });
    if let Some(cid) = category_id {
        chat["categoryId"] = json!(cid);
    }
    // Reach into the public type-name constant to match what the backend
    // uses internally; if the Chat type-name ever changes, this test will
    // refuse to compile.
    let type_name = <jmap_chat_types::Chat as JmapObject>::TYPE_NAME;
    backend.insert_object_for_test(type_name, account_id, chat_id, chat);
}

/// Oracle: Space/set addCategories assigns a fresh CategoryId and pushes
/// the category into Space.categories.
#[tokio::test]
async fn space_set_category_add_assigns_id_and_pushes() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Cat Add Test").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "General",
                        "position": 0,
                        "channelIds": [],
                    }]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"].is_null(),
        "addCategories should succeed: {:?}",
        resp["notUpdated"]
    );

    // Verify the category landed in space.categories with a server-assigned id.
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("handle_space_get");
    let cats = get_resp["list"][0]["categories"]
        .as_array()
        .expect("categories array");
    assert_eq!(cats.len(), 1, "exactly one category present");
    let new_id = cats[0]["id"].as_str().expect("category id");
    assert_ne!(
        new_id, "placeholder",
        "server must replace placeholder with a real id"
    );
    assert_eq!(cats[0]["name"], "General");
    assert_eq!(cats[0]["position"], 0);
}

/// Oracle: Space/set addCategories with a channelIds list referencing
/// existing channels updates those channels' categoryId and pulls them
/// out of uncategorizedChannelIds.
#[tokio::test]
async fn space_set_category_add_reassigns_existing_channels() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Cat Reassign Test").await;

    // Seed two channels into the Space; they start uncategorized.
    seed_channel(&backend, "a1", "ch-1", &space_id, None);
    seed_channel(&backend, "a1", "ch-2", &space_id, None);

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "Voice",
                        "position": 0,
                        "channelIds": ["ch-1", "ch-2"],
                    }]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"].is_null(),
        "addCategories with channels should succeed: {:?}",
        resp["notUpdated"]
    );

    // Channel categoryId fields should point at the new category.
    let new_cat_id = backend.first_category_id(&Id::from(space_id.as_str()));
    let chat1 = backend.peek_chat(&Id::from("ch-1"));
    let chat2 = backend.peek_chat(&Id::from("ch-2"));
    assert_eq!(
        chat1["categoryId"].as_str(),
        Some(new_cat_id.as_ref()),
        "ch-1.categoryId must point at the new category"
    );
    assert_eq!(
        chat2["categoryId"].as_str(),
        Some(new_cat_id.as_ref()),
        "ch-2.categoryId must point at the new category"
    );
}

/// Oracle: addCategories rejects channelIds that don't reference
/// channels of this Space (invalidProperties on `channelIds`).
#[tokio::test]
async fn space_set_category_add_unknown_channel_rejected() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Cat Unknown Channel Test").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "Voice",
                        "position": 0,
                        "channelIds": ["nonexistent-channel"],
                    }]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "invalidProperties",
        "unknown channelId must be invalidProperties: {:?}",
        resp["notUpdated"][&space_id]
    );
    let props: Vec<&str> = resp["notUpdated"][&space_id]["properties"]
        .as_array()
        .expect("properties array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        props.contains(&"channelIds"),
        "properties must name channelIds: {props:?}"
    );
}

/// Oracle: Space/set removeCategories with an unknown id surfaces
/// NotFound (the bead-id description is not preserved here; it just
/// names which category was not found).
#[tokio::test]
async fn space_set_category_remove_unknown_yields_not_found() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Cat Remove Unknown").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "removeCategories": ["does-not-exist"] } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "notFound",
        "removeCategory for unknown id must surface notFound: {:?}",
        resp["notUpdated"][&space_id]
    );
}

/// Oracle: Space/set removeCategories cascades channels into
/// uncategorizedChannelIds (draft §Space/set line 1126).
#[tokio::test]
async fn space_set_category_remove_cascades_channels_to_uncategorized() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Cat Cascade Test").await;

    // Create a category with two channels via addCategories.
    seed_channel(&backend, "a1", "ch-a", &space_id, None);
    seed_channel(&backend, "a1", "ch-b", &space_id, None);
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "TempCat",
                        "position": 0,
                        "channelIds": ["ch-a", "ch-b"],
                    }]
                }
            }
        }),
    )
    .await
    .expect("add cat");
    assert!(resp["notUpdated"].is_null(), "add failed: {:?}", resp);

    let cat_id = backend.first_category_id(&Id::from(space_id.as_str()));

    // Now remove the category. Both channels should land in uncategorized.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "removeCategories": [cat_id.as_ref()] } }
        }),
    )
    .await
    .expect("remove cat");

    assert!(
        resp["notUpdated"].is_null(),
        "removeCategories should succeed: {:?}",
        resp["notUpdated"]
    );

    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("get");
    assert_eq!(
        get_resp["list"][0]["categories"]
            .as_array()
            .expect("categories")
            .len(),
        0,
        "category must be removed from Space"
    );
    let unc: Vec<&str> = get_resp["list"][0]["uncategorizedChannelIds"]
        .as_array()
        .expect("uncategorized array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        unc.contains(&"ch-a") && unc.contains(&"ch-b"),
        "both channels must cascade into uncategorized: {unc:?}"
    );

    // Channel-side category_id must be cleared.
    assert!(
        backend
            .peek_chat(&Id::from("ch-a"))
            .get("categoryId")
            .is_none(),
        "ch-a categoryId must be cleared after cascade"
    );
    assert!(
        backend
            .peek_chat(&Id::from("ch-b"))
            .get("categoryId")
            .is_none(),
        "ch-b categoryId must be cleared after cascade"
    );
}

/// Oracle: Space/set updateCategories patches the category in place and,
/// when channelIds is replaced, keeps channel.categoryId consistent.
#[tokio::test]
async fn space_set_category_update_rename_and_reassign_channels() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Cat Update Test").await;

    seed_channel(&backend, "a1", "ch-x", &space_id, None);
    seed_channel(&backend, "a1", "ch-y", &space_id, None);
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "Original",
                        "position": 0,
                        "channelIds": ["ch-x"],
                    }]
                }
            }
        }),
    )
    .await
    .expect("seed cat");
    assert!(resp["notUpdated"].is_null());
    let cat_id = backend.first_category_id(&Id::from(space_id.as_str()));

    // Rename the category AND swap its channel membership: ch-x out,
    // ch-y in. Expected: ch-x lands in uncategorized, ch-y leaves
    // uncategorized; both channels' categoryId fields flip.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "updateCategories": [{
                        "id": cat_id.as_ref(),
                        "name": "Renamed",
                        "channelIds": ["ch-y"],
                    }]
                }
            }
        }),
    )
    .await
    .expect("update cat");
    assert!(
        resp["notUpdated"].is_null(),
        "updateCategories must succeed: {:?}",
        resp["notUpdated"]
    );

    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("get");
    let cats = get_resp["list"][0]["categories"]
        .as_array()
        .expect("categories");
    assert_eq!(cats[0]["name"], "Renamed", "name must update");
    let channel_ids: Vec<&str> = cats[0]["channelIds"]
        .as_array()
        .expect("channelIds")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(channel_ids, vec!["ch-y"], "channelIds replaced wholesale");

    let unc: Vec<&str> = get_resp["list"][0]["uncategorizedChannelIds"]
        .as_array()
        .expect("uncategorized")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        unc.contains(&"ch-x"),
        "ch-x must land in uncategorized: {unc:?}"
    );
    assert!(
        !unc.contains(&"ch-y"),
        "ch-y must have left uncategorized: {unc:?}"
    );

    assert!(
        backend
            .peek_chat(&Id::from("ch-x"))
            .get("categoryId")
            .is_none(),
        "ch-x.categoryId must be cleared"
    );
    assert_eq!(
        backend.peek_chat(&Id::from("ch-y"))["categoryId"].as_str(),
        Some(cat_id.as_ref()),
        "ch-y.categoryId must point at the (renamed) category"
    );
}

/// Oracle: when a Space/set Category op cascades and mutates one or
/// more Chat records' `categoryId` field, those Chat ids must surface
/// in the next `Chat/changes` delta as `updated`. This regression test
/// pins the fix from bd:JMAP-g7wu.2.4.9 — before that bead, the
/// `set_channel_category` cascade silently mutated Chat records
/// without producing a `Chat/changes` entry, leaving subscribers
/// unaware of the categoryId change.
#[tokio::test]
async fn space_set_category_cascade_emits_chat_changes() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Cat Cascade Chat-changes Test").await;

    // Seed two channels using direct injection so the Chat state stays
    // at "0" until the cascade bumps it. (Going through addChannels
    // would also work but would advance Chat state, requiring an extra
    // sinceState baseline read.)
    seed_channel(&backend, "a1", "ch-p", &space_id, None);
    seed_channel(&backend, "a1", "ch-q", &space_id, None);

    let (_, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "Cascade",
                        "position": 0,
                        "channelIds": ["ch-p", "ch-q"],
                    }]
                }
            }
        }),
    )
    .await
    .expect("addCategories");

    // Chat/changes since "0" must show both channels in `updated`
    // (the cascade set their categoryId).
    let (changes, _) = handle_chat_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": "0" }),
    )
    .await
    .expect("chat_changes");
    let mut updated: Vec<&str> = changes["updated"]
        .as_array()
        .expect("updated array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    updated.sort();
    assert_eq!(
        updated,
        vec!["ch-p", "ch-q"],
        "Chat/changes must list both cascaded channels as updated: {updated:?}"
    );
}

// ---------------------------------------------------------------------------
// Space/set Channel variants (bd:JMAP-g7wu.2.4.4)
//
// Oracle source: draft-atwood-jmap-chat-00 §Space/set lines 1114-1120.
// Hand-written JSON fixtures derived from the spec; no oracle is computed
// by code-under-test (test-integrity rule from workspace AGENTS.md).
// ---------------------------------------------------------------------------

/// Oracle: addChannels with no categoryId creates a channel-kind Chat
/// (server-assigned id) and lands the new id in
/// `space.uncategorizedChannelIds`. Required Chat fields (`createdAt`,
/// `unreadCount`, `pinnedMessageIds`, `muted`, `receiveTypingIndicators`,
/// `spaceId`, `name`, `kind: "channel"`) are all set.
#[tokio::test]
async fn space_set_channel_add_assigns_id_and_lands_uncategorized() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Add Test").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: { "addChannels": [{ "name": "general" }] }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"].is_null(),
        "addChannels should succeed: {:?}",
        resp["notUpdated"]
    );

    // The new channel id lands in uncategorizedChannelIds.
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("handle_space_get");
    let unc: Vec<&str> = get_resp["list"][0]["uncategorizedChannelIds"]
        .as_array()
        .expect("uncategorizedChannelIds")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        unc.len(),
        1,
        "exactly one channel should be uncategorized: {unc:?}"
    );
    let new_id = unc[0];

    // The Chat record exists with the expected fields.
    let chat = backend.peek_chat(&Id::from(new_id));
    assert_eq!(chat["kind"], "channel");
    assert_eq!(chat["spaceId"].as_str(), Some(space_id.as_str()));
    assert_eq!(chat["name"], "general");
    assert_eq!(chat["unreadCount"], 0);
    assert_eq!(chat["muted"], false);
    assert_eq!(chat["receiveTypingIndicators"], true);
    assert!(chat["pinnedMessageIds"].is_array());
    assert!(chat["pinnedMessageIds"].as_array().unwrap().is_empty());
    assert!(
        chat["createdAt"].is_string(),
        "createdAt must be set: {chat:?}"
    );
    // Channel-specific optional fields with no client input should be absent.
    assert!(chat.get("categoryId").is_none());
    assert!(chat.get("topic").is_none());
    assert!(chat.get("position").is_none());
}

/// Oracle: addChannels with a categoryId appends the new channel id
/// to that category's channelIds array (and NOT to
/// uncategorizedChannelIds). The Chat record carries the same
/// categoryId.
#[tokio::test]
async fn space_set_channel_add_into_existing_category() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Add Into Cat").await;

    // First, create an empty category to add the new channel into.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "Voice",
                        "position": 0,
                        "channelIds": [],
                    }]
                }
            }
        }),
    )
    .await
    .expect("addCategories");
    assert!(resp["notUpdated"].is_null(), "addCategories: {resp:?}");
    let cat_id = backend.first_category_id(&Id::from(space_id.as_str()));

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addChannels": [{
                        "name": "lounge",
                        "categoryId": cat_id.as_ref(),
                        "position": 1,
                        "topic": "general chit-chat",
                    }]
                }
            }
        }),
    )
    .await
    .expect("addChannels");
    assert!(
        resp["notUpdated"].is_null(),
        "addChannels should succeed: {:?}",
        resp["notUpdated"]
    );

    // The category's channelIds array now carries the new channel id;
    // uncategorizedChannelIds is empty.
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("handle_space_get");
    let cats = get_resp["list"][0]["categories"]
        .as_array()
        .expect("categories");
    let ch_ids: Vec<&str> = cats[0]["channelIds"]
        .as_array()
        .expect("channelIds")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        ch_ids.len(),
        1,
        "category should hold exactly one channel: {ch_ids:?}"
    );
    assert!(
        get_resp["list"][0]["uncategorizedChannelIds"]
            .as_array()
            .expect("uncategorizedChannelIds")
            .is_empty(),
        "uncategorizedChannelIds should be empty"
    );

    // The Chat record carries the categoryId and the optional fields.
    let chat = backend.peek_chat(&Id::from(ch_ids[0]));
    assert_eq!(chat["categoryId"].as_str(), Some(cat_id.as_ref()));
    assert_eq!(chat["position"], 1);
    assert_eq!(chat["topic"], "general chit-chat");
}

/// Oracle: addChannels with a categoryId that is not a category of
/// this Space is rejected with `invalidProperties` naming `categoryId`,
/// and no Chat record is created.
#[tokio::test]
async fn space_set_channel_add_unknown_category_rejected() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Add Unknown Cat").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addChannels": [{
                        "name": "ghost",
                        "categoryId": "nonexistent-cat",
                    }]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "invalidProperties",
        "unknown categoryId must surface invalidProperties: {:?}",
        resp["notUpdated"][&space_id]
    );
    let props: Vec<&str> = resp["notUpdated"][&space_id]["properties"]
        .as_array()
        .expect("properties array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        props.contains(&"categoryId"),
        "properties must name categoryId: {props:?}"
    );

    // No Chat record should have been created.
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("handle_space_get");
    assert!(
        get_resp["list"][0]["uncategorizedChannelIds"]
            .as_array()
            .expect("uncategorizedChannelIds")
            .is_empty(),
        "no channel should be created on failed addChannels"
    );
}

/// Oracle: removeChannels with an unknown id returns `notFound` and
/// does not destroy any other channel or Message.
#[tokio::test]
async fn space_set_channel_remove_unknown_yields_not_found() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Remove Unknown").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "removeChannels": ["does-not-exist"] } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "notFound",
        "removeChannels for unknown id must surface notFound: {:?}",
        resp["notUpdated"][&space_id]
    );
}

/// Oracle: removeChannels destroys the Chat record and cascades to
/// every Message in that channel (draft §Space/set line 1117). The
/// Space-side cross-reference (uncategorized or per-category) is
/// pruned. Other channels in the Space are untouched.
#[tokio::test]
async fn space_set_channel_remove_cascades_messages() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Remove Cascade").await;

    // Add two channels via Space/set so we exercise the real path.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: { "addChannels": [
                    { "name": "general" },
                    { "name": "keep-me" },
                ] }
            }
        }),
    )
    .await
    .expect("addChannels");
    assert!(resp["notUpdated"].is_null(), "addChannels: {resp:?}");

    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("handle_space_get");
    let unc: Vec<String> = get_resp["list"][0]["uncategorizedChannelIds"]
        .as_array()
        .expect("uncategorizedChannelIds")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    assert_eq!(unc.len(), 2, "two channels uncategorized: {unc:?}");

    // Identify which channel id is which by reading the stored Chat
    // records. Iteration order over a HashMap is non-deterministic; we
    // distinguish by Chat.name (which is what callers always have).
    let (general_id, keep_id) = {
        let mut general = None;
        let mut keep = None;
        for cid in &unc {
            let chat = backend.peek_chat(&Id::from(cid.as_str()));
            match chat["name"].as_str() {
                Some("general") => general = Some(cid.clone()),
                Some("keep-me") => keep = Some(cid.clone()),
                other => panic!("unexpected channel name: {other:?}"),
            }
        }
        (
            general.expect("general channel created"),
            keep.expect("keep-me channel created"),
        )
    };

    // Seed three Messages in general and one in keep-me using the
    // test-only direct-injection helper. The cascade only reads each
    // Message's `chatId` so we don't need every required Message field
    // — `insert_object_for_test` bypasses serde validation. We exercise
    // the cascade and observe the `destroyed` ids via `Message/changes`
    // (which returns just id lists, no Message deserialization).
    backend.insert_object_for_test(
        <jmap_chat_types::Message as jmap_chat_server::JmapObject>::TYPE_NAME,
        "a1",
        "m1",
        json!({ "id": "m1", "chatId": general_id, "body": "one" }),
    );
    backend.insert_object_for_test(
        <jmap_chat_types::Message as jmap_chat_server::JmapObject>::TYPE_NAME,
        "a1",
        "m2",
        json!({ "id": "m2", "chatId": general_id, "body": "two" }),
    );
    backend.insert_object_for_test(
        <jmap_chat_types::Message as jmap_chat_server::JmapObject>::TYPE_NAME,
        "a1",
        "m3",
        json!({ "id": "m3", "chatId": general_id, "body": "three" }),
    );
    backend.insert_object_for_test(
        <jmap_chat_types::Message as jmap_chat_server::JmapObject>::TYPE_NAME,
        "a1",
        "k1",
        json!({ "id": "k1", "chatId": keep_id, "body": "keep" }),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "removeChannels": [&general_id] } }
        }),
    )
    .await
    .expect("removeChannels");
    assert!(
        resp["notUpdated"].is_null(),
        "removeChannels should succeed: {:?}",
        resp["notUpdated"]
    );

    // Verify the cascade via Message/changes (id-only, no Message
    // deserialization). Since insert_object_for_test does not bump
    // Message state, the only Message state-bump in this run is the
    // one our removeChannels cascade emits. So a Message/changes since
    // "0" must list exactly m1, m2, m3 in `destroyed`.
    let (changes_resp, _) = handle_message_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": "0" }),
    )
    .await
    .expect("message_changes");
    let mut destroyed: Vec<String> = changes_resp["destroyed"]
        .as_array()
        .expect("destroyed array")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    destroyed.sort();
    assert_eq!(
        destroyed,
        vec!["m1".to_owned(), "m2".to_owned(), "m3".to_owned()],
        "cascade must destroy exactly the three messages in the removed channel: {destroyed:?}"
    );

    // Surviving channel still listed in uncategorizedChannelIds.
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("space_get");
    let unc: Vec<&str> = get_resp["list"][0]["uncategorizedChannelIds"]
        .as_array()
        .expect("uncategorizedChannelIds")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        unc,
        vec![keep_id.as_str()],
        "only keep-me must remain uncategorized: {unc:?}"
    );
}

/// Oracle: updateChannels patches a channel's `name` and `position`
/// fields in place. Required fields and other optionals are preserved.
#[tokio::test]
async fn space_set_channel_update_name_and_position() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Update Basic").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: { "addChannels": [{ "name": "old-name", "topic": "old topic" }] }
            }
        }),
    )
    .await
    .expect("addChannels");
    assert!(resp["notUpdated"].is_null(), "addChannels: {resp:?}");
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("space_get");
    let ch_id = get_resp["list"][0]["uncategorizedChannelIds"][0]
        .as_str()
        .expect("channel id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "updateChannels": [{
                        "id": &ch_id,
                        "name": "renamed",
                        "position": 7,
                    }]
                }
            }
        }),
    )
    .await
    .expect("updateChannels");
    assert!(
        resp["notUpdated"].is_null(),
        "updateChannels should succeed: {:?}",
        resp["notUpdated"]
    );

    let chat = backend.peek_chat(&Id::from(ch_id.as_str()));
    assert_eq!(chat["name"], "renamed");
    assert_eq!(chat["position"], 7);
    // Topic was not in the patch — preserve as-is.
    assert_eq!(chat["topic"], "old topic");
}

/// Oracle: updateChannels with `"topic": null` clears the topic
/// (Clearable::Clear semantics) without touching other fields.
#[tokio::test]
async fn space_set_channel_update_topic_cleared_by_null() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Update Clearable").await;

    let (_, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: { "addChannels": [{ "name": "ch", "topic": "to-clear" }] }
            }
        }),
    )
    .await
    .expect("addChannels");
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("space_get");
    let ch_id = get_resp["list"][0]["uncategorizedChannelIds"][0]
        .as_str()
        .expect("channel id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: { "updateChannels": [{ "id": &ch_id, "topic": null }] }
            }
        }),
    )
    .await
    .expect("updateChannels");
    assert!(
        resp["notUpdated"].is_null(),
        "updateChannels should succeed: {:?}",
        resp["notUpdated"]
    );

    let chat = backend.peek_chat(&Id::from(ch_id.as_str()));
    assert!(
        chat.get("topic").is_none(),
        "topic must be cleared (absent): {chat:?}"
    );
    // Name is untouched.
    assert_eq!(chat["name"], "ch");
}

/// Oracle: updateChannels with a new `categoryId` value moves the
/// channel from `uncategorizedChannelIds` (or its prior category) into
/// the new category's `channelIds`, keeping Chat.categoryId and the
/// Space-side arrays consistent.
#[tokio::test]
async fn space_set_channel_update_category_move() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Update Move Cat").await;

    let (_, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "Voice",
                        "position": 0,
                        "channelIds": [],
                    }],
                    "addChannels": [{ "name": "wandering" }],
                }
            }
        }),
    )
    .await
    .expect("seed");
    let cat_id = backend.first_category_id(&Id::from(space_id.as_str()));
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("space_get");
    let ch_id = get_resp["list"][0]["uncategorizedChannelIds"][0]
        .as_str()
        .expect("channel id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "updateChannels": [{ "id": &ch_id, "categoryId": cat_id.as_ref() }]
                }
            }
        }),
    )
    .await
    .expect("updateChannels");
    assert!(
        resp["notUpdated"].is_null(),
        "updateChannels should succeed: {:?}",
        resp["notUpdated"]
    );

    let chat = backend.peek_chat(&Id::from(ch_id.as_str()));
    assert_eq!(chat["categoryId"].as_str(), Some(cat_id.as_ref()));

    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("space_get");
    assert!(
        get_resp["list"][0]["uncategorizedChannelIds"]
            .as_array()
            .expect("uncategorizedChannelIds")
            .is_empty(),
        "uncategorized must be empty after move"
    );
    let new_ch_ids: Vec<&str> = get_resp["list"][0]["categories"][0]["channelIds"]
        .as_array()
        .expect("channelIds")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        new_ch_ids,
        vec![ch_id.as_str()],
        "category channelIds must now hold the channel: {new_ch_ids:?}"
    );
}

/// Oracle: updateChannels with `"categoryId": null` (Clearable::Clear)
/// moves the channel back to `uncategorizedChannelIds` and drops the
/// channel id from its prior category's `channelIds`.
#[tokio::test]
async fn space_set_channel_update_category_cleared_by_null() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Update Clear Cat").await;

    // Seed: create a category with one channel inside it.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "Voice",
                        "position": 0,
                        "channelIds": [],
                    }]
                }
            }
        }),
    )
    .await
    .expect("addCategories");
    assert!(resp["notUpdated"].is_null());
    let cat_id = backend.first_category_id(&Id::from(space_id.as_str()));

    let (_, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addChannels": [{ "name": "ch", "categoryId": cat_id.as_ref() }]
                }
            }
        }),
    )
    .await
    .expect("addChannels");

    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("space_get");
    let ch_id = get_resp["list"][0]["categories"][0]["channelIds"][0]
        .as_str()
        .expect("channel id")
        .to_owned();

    // Clear the categoryId.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "updateChannels": [{ "id": &ch_id, "categoryId": null }]
                }
            }
        }),
    )
    .await
    .expect("updateChannels");
    assert!(resp["notUpdated"].is_null(), "{resp:?}");

    let chat = backend.peek_chat(&Id::from(ch_id.as_str()));
    assert!(
        chat.get("categoryId").is_none(),
        "Chat.categoryId must be absent after clear: {chat:?}"
    );
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("space_get");
    let unc: Vec<&str> = get_resp["list"][0]["uncategorizedChannelIds"]
        .as_array()
        .expect("uncategorizedChannelIds")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        unc,
        vec![ch_id.as_str()],
        "channel must move back to uncategorized: {unc:?}"
    );
    assert!(
        get_resp["list"][0]["categories"][0]["channelIds"]
            .as_array()
            .expect("channelIds")
            .is_empty(),
        "former category's channelIds must drop the channel"
    );
}

/// Oracle: updateChannels with a categoryId that is not a category of
/// this Space is rejected with `invalidProperties` naming `categoryId`,
/// and the existing assignment is preserved.
#[tokio::test]
async fn space_set_channel_update_unknown_category_rejected() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Update Unknown Cat").await;

    let (_, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "addChannels": [{ "name": "ch" }] } }
        }),
    )
    .await
    .expect("addChannels");
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("space_get");
    let ch_id = get_resp["list"][0]["uncategorizedChannelIds"][0]
        .as_str()
        .expect("channel id")
        .to_owned();

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "updateChannels": [{ "id": &ch_id, "categoryId": "no-such-cat" }]
                }
            }
        }),
    )
    .await
    .expect("updateChannels");
    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "invalidProperties",
        "unknown categoryId on update must surface invalidProperties: {:?}",
        resp["notUpdated"][&space_id]
    );

    // The channel's categoryId remains absent (the prior state).
    let chat = backend.peek_chat(&Id::from(ch_id.as_str()));
    assert!(
        chat.get("categoryId").is_none(),
        "categoryId must remain unset after failed update: {chat:?}"
    );
}

/// Oracle: updateChannels for an id that is not a channel of this
/// Space (does not exist, or has a different spaceId) returns
/// `notFound`.
#[tokio::test]
async fn space_set_channel_update_unknown_channel_yields_not_found() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Update Unknown Channel").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "updateChannels": [{ "id": "no-such-channel", "name": "x" }]
                }
            }
        }),
    )
    .await
    .expect("updateChannels");
    assert_eq!(
        resp["notUpdated"][&space_id]["type"], "notFound",
        "updateChannels for unknown id must surface notFound: {:?}",
        resp["notUpdated"][&space_id]
    );
}

/// Oracle: a Space/set patch that creates and then immediately
/// destroys a channel produces a single consolidated `Chat/changes`
/// entry whose `destroyed` list contains the channel id and whose
/// `created`/`updated` lists are empty. (Same channel in both lists
/// would amplify state-token rotations; the implementation de-dups
/// to the most-impactful list per call.)
#[tokio::test]
async fn space_set_channel_changelog_dedups_create_then_destroy() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Channel Changelog Dedup").await;

    // Establish a baseline state for Chat so we know we are looking at
    // the delta introduced by this single patch.
    let (chat_state_resp, _) = handle_chat_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": "0" }),
    )
    .await
    .expect("chat_changes baseline");
    let baseline_state = chat_state_resp["newState"]
        .as_str()
        .expect("baseline state")
        .to_owned();

    // Add a channel then immediately remove it in the same patch.
    // The handler walks ops in array order, so add-then-remove leaves
    // the channel destroyed at end-of-call. Read the assigned id from
    // the OpResult-driven uncategorized list snapshot mid-patch is not
    // wire-visible; we look at change-log entries instead.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addChannels": [{ "name": "ephemeral" }]
                }
            }
        }),
    )
    .await
    .expect("addChannels");
    assert!(resp["notUpdated"].is_null(), "addChannels: {resp:?}");
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("space_get");
    let ch_id = get_resp["list"][0]["uncategorizedChannelIds"][0]
        .as_str()
        .expect("channel id")
        .to_owned();

    // Re-fetch baseline (we just bumped Chat state with the create).
    let (chat_state_resp, _) = handle_chat_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": &baseline_state }),
    )
    .await
    .expect("chat_changes after create");
    let created_list: Vec<&str> = chat_state_resp["created"]
        .as_array()
        .expect("created array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        created_list,
        vec![ch_id.as_str()],
        "Chat/changes after create must list the new channel: {created_list:?}"
    );
    let after_create_state = chat_state_resp["newState"]
        .as_str()
        .expect("post-create state")
        .to_owned();

    // Now destroy the channel.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { &space_id: { "removeChannels": [&ch_id] } }
        }),
    )
    .await
    .expect("removeChannels");
    assert!(resp["notUpdated"].is_null(), "removeChannels: {resp:?}");

    // Chat/changes since the post-create state must show only the
    // destruction. The dedup logic in apply_space_patch ensures the
    // channel id is in `destroyed`, not `updated` or `created`.
    let (chat_state_resp, _) = handle_chat_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": &after_create_state }),
    )
    .await
    .expect("chat_changes after destroy");
    let destroyed: Vec<&str> = chat_state_resp["destroyed"]
        .as_array()
        .expect("destroyed array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        destroyed,
        vec![ch_id.as_str()],
        "destroy delta must list the channel id: {destroyed:?}"
    );
    assert!(
        chat_state_resp["created"]
            .as_array()
            .expect("created array")
            .is_empty(),
        "created list in destroy delta must be empty: {:?}",
        chat_state_resp["created"]
    );
    assert!(
        chat_state_resp["updated"]
            .as_array()
            .expect("updated array")
            .is_empty(),
        "updated list in destroy delta must be empty: {:?}",
        chat_state_resp["updated"]
    );
}

/// Oracle: Space/set update accepts metadata fields (name, description, isPublic, etc.).
#[tokio::test]
async fn space_set_update_metadata_success() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_space_set(
        &backend,
        &(),
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
        &(),
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
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
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
        &(),
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
        &(),
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

/// Oracle: draft-atwood-jmap-chat-00 §ChatContact/set (line 878):
/// "update supports: blocked, displayName."
///
/// A patch carrying spec-allowed fields lands them on the object.
/// This is the positive control for the allowlist projection added
/// in bd:JMAP-x2gd.8.
#[tokio::test]
async fn contact_set_update_allowed_fields_apply() {
    let backend = MemoryBackend::new();
    backend.register_account(&jmap_types::Id::from("a1"));
    backend.insert_object_for_test(
        "ChatContact",
        "a1",
        "ct1",
        json!({
            "id": "ct1",
            "login": "alice@example.com",
            "firstSeenAt": "2024-01-01T00:00:00Z",
            "lastSeenAt": "2024-01-02T00:00:00Z",
            "blocked": false
        }),
    );

    let (resp, _) = handle_contact_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { "ct1": { "blocked": true, "displayName": "Alice" } }
        }),
    )
    .await
    .expect("handle_contact_set");

    assert!(
        resp["notUpdated"].is_null(),
        "spec-allowed patch must update: {:?}",
        resp["notUpdated"]
    );
    let (get_resp, _) =
        handle_contact_get(&backend, &(), json!({ "accountId": "a1", "ids": ["ct1"] }))
            .await
            .expect("handle_contact_get");
    assert_eq!(get_resp["list"][0]["blocked"], json!(true));
    assert_eq!(get_resp["list"][0]["displayName"], json!("Alice"));
}

/// Oracle: draft-atwood-jmap-chat-00 §ChatContact/set says
/// "update supports: blocked, displayName." A patch carrying ONLY
/// non-allowed fields must not reach the backend. The handler
/// surfaces `invalidPatch` so a client cannot silently overwrite
/// server-derived state such as `presence`. Regression bead
/// bd:JMAP-x2gd.8.
#[tokio::test]
async fn contact_set_update_drops_non_allowed_fields() {
    let backend = MemoryBackend::new();
    backend.register_account(&jmap_types::Id::from("a1"));
    backend.insert_object_for_test(
        "ChatContact",
        "a1",
        "ct1",
        json!({
            "id": "ct1",
            "login": "alice@example.com",
            "firstSeenAt": "2024-01-01T00:00:00Z",
            "lastSeenAt": "2024-01-02T00:00:00Z",
            "blocked": false
        }),
    );

    let (resp, _) = handle_contact_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { "ct1": { "presence": "online", "lastActiveAt": "2030-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("handle_contact_set");

    assert!(
        resp["updated"].is_null(),
        "non-allowed patch must not update"
    );
    let not_updated = resp["notUpdated"]
        .as_object()
        .expect("notUpdated must be populated");
    let entry = &not_updated["ct1"];
    assert_eq!(
        entry["type"], "invalidPatch",
        "patch with no spec-allowed fields must be invalidPatch: {entry:?}"
    );

    // The backend record must be unchanged.
    let (get_resp, _) =
        handle_contact_get(&backend, &(), json!({ "accountId": "a1", "ids": ["ct1"] }))
            .await
            .expect("handle_contact_get");
    assert!(
        get_resp["list"][0].get("presence").is_none(),
        "presence must not have leaked into the stored object"
    );
    assert!(
        get_resp["list"][0].get("lastActiveAt").is_none(),
        "lastActiveAt must not have leaked into the stored object"
    );
}

/// Oracle: a mixed patch carrying one spec-allowed field and one
/// non-allowed field applies only the allowed field. The non-allowed
/// field is silently dropped before reaching the backend, matching
/// the existing SpaceBan/set allowlist convention (ban.rs:250-263).
#[tokio::test]
async fn contact_set_update_mixed_patch_applies_only_allowed() {
    let backend = MemoryBackend::new();
    backend.register_account(&jmap_types::Id::from("a1"));
    backend.insert_object_for_test(
        "ChatContact",
        "a1",
        "ct1",
        json!({
            "id": "ct1",
            "login": "alice@example.com",
            "firstSeenAt": "2024-01-01T00:00:00Z",
            "lastSeenAt": "2024-01-02T00:00:00Z",
            "blocked": false
        }),
    );

    let (resp, _) = handle_contact_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { "ct1": { "blocked": true, "presence": "online" } }
        }),
    )
    .await
    .expect("handle_contact_set");

    assert!(
        resp["notUpdated"].is_null(),
        "patch with at least one allowed field must update"
    );
    let (get_resp, _) =
        handle_contact_get(&backend, &(), json!({ "accountId": "a1", "ids": ["ct1"] }))
            .await
            .expect("handle_contact_get");
    assert_eq!(get_resp["list"][0]["blocked"], json!(true));
    assert!(
        get_resp["list"][0].get("presence").is_none(),
        "non-allowed `presence` field must not have leaked into the stored object"
    );
}

/// Oracle: ChatContact/get on an empty backend returns an empty list.
#[tokio::test]
async fn contact_get_empty() {
    let backend = MemoryBackend::new();
    let (get_resp, _) =
        handle_contact_get(&backend, &(), json!({ "accountId": "a1", "ids": null }))
            .await
            .expect("get");

    assert_eq!(get_resp["list"].as_array().expect("list").len(), 0);
}

/// Oracle: ChatContact/changes on an empty backend returns empty change lists.
#[tokio::test]
async fn contact_changes_empty() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_contact_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": "0" }),
    )
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
        &(),
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
        &(),
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

/// Oracle: Two sequential ReadPosition/set creates for the same chatId
/// reject the second with `alreadyExists`, naming the canonical id of
/// the existing record. The (account, chatId) uniqueness invariant
/// (position.rs module-doc) is enforced by the handler so a retried
/// client call doesn't produce two ReadPosition rows (bd:JMAP-x2gd.13).
#[tokio::test]
async fn position_set_create_duplicate_chat_id_sequential_rejected() {
    let backend = MemoryBackend::new();
    let (first, _) = handle_position_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "rp0": { "chatId": "c1" } }
        }),
    )
    .await
    .expect("first create");
    let canonical_id = first["created"]["rp0"]["id"]
        .as_str()
        .expect("first id")
        .to_owned();

    let (second, _) = handle_position_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "rp1": { "chatId": "c1" } }
        }),
    )
    .await
    .expect("second create");

    assert!(second["notCreated"]["rp1"].is_object());
    assert_eq!(second["notCreated"]["rp1"]["type"], "alreadyExists");
    assert_eq!(
        second["notCreated"]["rp1"]["existingId"], canonical_id,
        "expected the existingId to name the canonical pre-existing ReadPosition"
    );
    assert!(second["created"].as_object().is_none_or(|m| m.is_empty()));
}

/// Oracle: Two creates for the same chatId in a single /set batch reject
/// the second with `alreadyExists`, naming the id of the one that did
/// succeed. Mirrors Chat/set Direct intra-batch dedup at chat.rs.
#[tokio::test]
async fn position_set_create_duplicate_chat_id_intra_batch_rejected() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_position_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "rp0": { "chatId": "c1" },
                "rp1": { "chatId": "c1" }
            }
        }),
    )
    .await
    .expect("handle_position_set");

    // Exactly one of rp0/rp1 should be in `created`; the other should be
    // in `notCreated` with alreadyExists pointing at the winner's id.
    let created_keys: Vec<&str> = resp["created"]
        .as_object()
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let not_created_keys: Vec<&str> = resp["notCreated"]
        .as_object()
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default();
    assert_eq!(created_keys.len(), 1, "exactly one create should succeed");
    assert_eq!(
        not_created_keys.len(),
        1,
        "exactly one create should be rejected"
    );

    let winner = created_keys[0];
    let loser = not_created_keys[0];
    assert_ne!(winner, loser);
    let canonical_id = resp["created"][winner]["id"]
        .as_str()
        .expect("winner id")
        .to_owned();
    assert_eq!(resp["notCreated"][loser]["type"], "alreadyExists");
    assert_eq!(resp["notCreated"][loser]["existingId"], canonical_id);
}

/// Oracle: ReadPosition/set update of chatId is rejected (immutable).
#[tokio::test]
async fn position_set_update_chat_id_rejected() {
    let backend = MemoryBackend::new();

    let (create_resp, _) = handle_position_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "create": { "rp0": { "chatId": "c1" } } }),
    )
    .await
    .expect("create");
    let rp_id = create_resp["created"]["rp0"]["id"].as_str().expect("id");

    let (resp, _) = handle_position_set(
        &backend,
        &(),
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
        &(),
        json!({ "accountId": "a1", "create": { "rp0": { "chatId": "c99" } } }),
    )
    .await
    .expect("create");
    let rp_id = create_resp["created"]["rp0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (get_resp, _) =
        handle_position_get(&backend, &(), json!({ "accountId": "a1", "ids": [rp_id] }))
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
    let err = handle_chat_get(&backend, &(), json!({ "accountId": "a1", "ids": null }))
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
    let a1 = Id::from("a1");
    let a2 = Id::from("a2");
    backend.register_account(&a1);
    backend.register_account(&a2);

    handle_chat_set(
        &backend,
        &(),
        json!({ "accountId": "a1", "create": { "c0": { "kind": "group", "name": "A's group" } } }),
    )
    .await
    .expect("create in a1");

    let (resp, _) = handle_chat_get(&backend, &(), json!({ "accountId": "a2", "ids": null }))
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
    let (resp, invocations) = handle_contact_query(&backend, &(), json!({ "accountId": "a1" }))
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
    let err = handle_contact_query_changes(&backend, &(), json!({ "accountId": "a1" }))
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
        handle_invite_get(&backend, &(), json!({ "accountId": "a1", "ids": null }))
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
        &(),
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

/// Oracle: SpaceInvite/set create with malformed expiresAt (not RFC 8620 §1.4
/// UTCDate form) is rejected with invalidProperties: ["expiresAt"].
#[tokio::test]
async fn invite_set_create_with_malformed_expiry_rejected() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_invite_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "i0": { "spaceId": "s1", "expiresAt": "next-friday" }
            }
        }),
    )
    .await
    .expect("handle_invite_set");

    assert!(resp["notCreated"]["i0"].is_object());
    assert_eq!(resp["notCreated"]["i0"]["type"], "invalidProperties");
    let props = resp["notCreated"]["i0"]["properties"]
        .as_array()
        .expect("properties");
    assert!(
        props.iter().any(|p| p == "expiresAt"),
        "expiresAt must be listed in rejected properties"
    );
}

/// Oracle: SpaceInvite/set create with valid spaceId succeeds and returns a code field.
#[tokio::test]
async fn invite_set_create_success() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_invite_set(
        &backend,
        &(),
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
        &(),
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
        &(),
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
        &(),
        json!({ "accountId": "a1", "create": { "i0": { "spaceId": "s2", "maxUses": 10 } } }),
    )
    .await
    .expect("create");
    let invite_id = create_resp["created"]["i0"]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let (resp, _) = handle_invite_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [invite_id] }),
    )
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
    let (resp, invocations) =
        handle_emoji_get(&backend, &(), json!({ "accountId": "a1", "ids": null }))
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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

    let (get_resp, _) = handle_emoji_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [emoji_id] }),
    )
    .await
    .expect("get");

    assert_eq!(get_resp["list"].as_array().expect("list").len(), 1);
    assert_eq!(get_resp["list"][0]["name"], "rocket");
    assert_eq!(get_resp["list"][0]["blobId"], "b99");
    assert_eq!(get_resp["notFound"], json!([]));
}

// ---------------------------------------------------------------------------
// CustomEmoji authorization gate (draft-atwood-jmap-chat-00 commit 9344aec)
// ---------------------------------------------------------------------------

/// Oracle: the reference `MemoryBackend` returns `Ok(Ok(()))` from
/// `may_set_custom_emoji` unconditionally; `CustomEmoji/set` create
/// without a spaceId (server-global) succeeds. The kit defines the
/// authorization hook; the consumer enforces the policy.
#[tokio::test]
async fn emoji_set_create_server_global_no_authz_block() {
    use jmap_chat_server::handle_emoji_set;

    let backend = MemoryBackend::new();
    let (resp, _) = handle_emoji_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "e0": { "name": "smile", "blobId": "b1" } }
        }),
    )
    .await
    .expect("handle_emoji_set");

    assert!(resp["created"]["e0"].is_object());
    assert_eq!(resp["notCreated"], json!(null));
    assert_eq!(
        resp["created"]["e0"]["spaceId"],
        json!(null),
        "server-global emoji has no spaceId"
    );
}

/// Oracle: `MemoryBackend` permits a Space-scoped `CustomEmoji/set`
/// create too (its `may_set_custom_emoji` is the demo permissive
/// `Ok(Ok(()))`). Spec text: "Authorization for CustomEmoji/set is
/// implementation-defined, for both Space-scoped and server-global
/// emoji" (draft commit 9344aec).
#[tokio::test]
async fn emoji_set_create_space_scoped_no_authz_block() {
    use jmap_chat_server::handle_emoji_set;

    let backend = MemoryBackend::new();
    let (resp, _) = handle_emoji_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "e0": { "name": "wave", "blobId": "b1", "spaceId": "s1" }
            }
        }),
    )
    .await
    .expect("handle_emoji_set");

    assert!(resp["created"]["e0"].is_object());
    assert_eq!(resp["notCreated"], json!(null));
    assert_eq!(resp["created"]["e0"]["spaceId"], "s1");
}

/// Oracle: when `may_set_custom_emoji` returns
/// `Ok(Err(SetError::new(SetErrorType::Forbidden)))` on Create, the
/// handler MUST reject the create with the backend's SetError
/// serialised verbatim into `notCreated`, including its description,
/// per draft commit 9344aec.
#[tokio::test]
async fn emoji_set_create_authz_denied_returns_forbidden() {
    use jmap_chat_server::handle_emoji_set;

    let backend = TrackingBackend::with_emoji_set_denied();
    let (resp, _) = handle_emoji_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "e0": { "name": "blocked", "blobId": "b1" } }
        }),
    )
    .await
    .expect("handle_emoji_set");

    assert_eq!(resp["created"], json!(null));
    let nc = &resp["notCreated"]["e0"];
    assert!(nc.is_object(), "e0 must appear in notCreated");
    assert_eq!(nc["type"], "forbidden");
    assert!(
        nc["description"]
            .as_str()
            .is_some_and(|s| s.contains("emoji authorization")),
        "description must reference the authorization gate (got {nc:?})"
    );
}

/// Oracle: when `may_set_custom_emoji` returns
/// `Ok(Err(SetError::new(SetErrorType::Forbidden)))` on Update, the
/// handler MUST reject the update with the backend's SetError
/// serialised verbatim into `notUpdated`. The update path pre-fetches
/// the existing emoji to learn its spaceId before consulting the
/// gate. We seed the wrapped `MemoryBackend` directly via
/// `insert_object_for_test` because the deny-everything backend
/// cannot seed via its own `create_object` (the gate denies create
/// too).
#[tokio::test]
async fn emoji_set_update_authz_denied_returns_forbidden() {
    use jmap_chat_server::handle_emoji_set;

    let backend = TrackingBackend::with_emoji_set_denied();
    backend.inner().insert_object_for_test(
        "CustomEmoji",
        "a1",
        "emoji1",
        json!({
            "id": "emoji1",
            "name": "preexisting",
            "blobId": "b1",
            "createdBy": "a1",
            "createdAt": "2024-01-01T00:00:00Z"
        }),
    );

    let (resp, _) = handle_emoji_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { "emoji1": { "name": "renamed" } }
        }),
    )
    .await
    .expect("handle_emoji_set");

    assert_eq!(resp["updated"], json!(null));
    let nu = &resp["notUpdated"]["emoji1"];
    assert!(nu.is_object(), "emoji1 must appear in notUpdated");
    assert_eq!(nu["type"], "forbidden");
}

/// Oracle: when `may_set_custom_emoji` returns
/// `Ok(Err(SetError::new(SetErrorType::Forbidden)))` on Destroy, the
/// handler MUST reject the destroy with the backend's SetError
/// serialised verbatim into `notDestroyed`. Destroy path pre-fetches
/// the existing emoji to learn its spaceId before consulting the
/// gate.
#[tokio::test]
async fn emoji_set_destroy_authz_denied_returns_forbidden() {
    use jmap_chat_server::handle_emoji_set;

    let backend = TrackingBackend::with_emoji_set_denied();
    backend.inner().insert_object_for_test(
        "CustomEmoji",
        "a1",
        "emoji1",
        json!({
            "id": "emoji1",
            "name": "doomed",
            "blobId": "b1",
            "createdBy": "a1",
            "createdAt": "2024-01-01T00:00:00Z"
        }),
    );

    let (resp, _) = handle_emoji_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "destroy": ["emoji1"]
        }),
    )
    .await
    .expect("handle_emoji_set");

    assert_eq!(
        resp["destroyed"],
        json!(null),
        "destroyed array must be absent (null) when no entry succeeded"
    );
    let nd = &resp["notDestroyed"]["emoji1"];
    assert!(nd.is_object(), "emoji1 must appear in notDestroyed");
    assert_eq!(nd["type"], "forbidden");
}

/// Oracle: the authorization gate is skipped for non-existent update
/// targets — `update_object` surfaces `notFound` naturally without the
/// pre-fetch consuming an authorization decision.
#[tokio::test]
async fn emoji_set_update_missing_id_does_not_consult_gate() {
    use jmap_chat_server::handle_emoji_set;

    // Backend denies every authorization decision. If the handler
    // were to consult the gate before discovering the target is
    // absent, the wire response would be `forbidden`. Per the
    // documented contract, the gate is skipped and the response is
    // `notFound`.
    let backend = TrackingBackend::with_emoji_set_denied();

    let (resp, _) = handle_emoji_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": { "ghost": { "name": "ghost" } }
        }),
    )
    .await
    .expect("handle_emoji_set");

    let nu = &resp["notUpdated"]["ghost"];
    assert!(nu.is_object());
    assert_eq!(
        nu["type"], "notFound",
        "non-existent target must surface notFound, NOT forbidden"
    );
}

// ---------------------------------------------------------------------------
// SpaceBan
// ---------------------------------------------------------------------------

/// Oracle: SpaceBan/get on empty backend returns empty list.
#[tokio::test]
async fn ban_get_empty() {
    let backend = MemoryBackend::new();
    let (resp, invocations) =
        handle_ban_get(&backend, &(), json!({ "accountId": "a1", "ids": null }))
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
        &(),
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

/// Oracle: SpaceBan/set create with malformed expiresAt (not RFC 8620 §1.4
/// UTCDate form) is rejected with invalidProperties: ["expiresAt"].
#[tokio::test]
async fn ban_set_create_with_malformed_expiry_rejected() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_ban_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": {
                "b0": {
                    "spaceId": "s1",
                    "userId": "u2",
                    "expiresAt": "later"
                }
            }
        }),
    )
    .await
    .expect("handle_ban_set");

    assert!(resp["notCreated"]["b0"].is_object());
    assert_eq!(resp["notCreated"]["b0"]["type"], "invalidProperties");
    let props = resp["notCreated"]["b0"]["properties"]
        .as_array()
        .expect("properties");
    assert!(
        props.iter().any(|p| p == "expiresAt"),
        "expiresAt must be listed in rejected properties"
    );
}

/// Oracle: SpaceBan/set create without userId returns notCreated invalidProperties.
#[tokio::test]
async fn ban_set_create_missing_user_id() {
    let backend = MemoryBackend::new();
    let (resp, _) = handle_ban_set(
        &backend,
        &(),
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
        &(),
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
        &(),
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

    let (get_resp, _) =
        handle_ban_get(&backend, &(), json!({ "accountId": "a1", "ids": [ban_id] }))
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
        handle_presence_get(&backend, &(), json!({ "accountId": "a1", "ids": null }))
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
    let (resp, invocations) = handle_presence_changes(
        &backend,
        &(),
        json!({ "accountId": "a1", "sinceState": "0" }),
    )
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
    let err = handle_presence_changes(&backend, &(), json!({ "accountId": "a1" }))
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
    let err = handle_space_join(&backend, &(), json!({ "accountId": "a1" }))
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
    let (invite_list, _) = handle_invite_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [invite_id] }),
    )
    .await
    .expect("handle_invite_get");
    assert_eq!(
        invite_list["list"][0]["uses"], 1,
        "uses must be incremented"
    );

    // Verify caller was added as a member.
    let (space_list, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [space_id] }),
    )
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
        .create_object::<SpaceInvite>(&(), &account_id, "i0", invite)
        .await
        .expect("create SpaceInvite");

    let err = handle_space_join(
        &backend,
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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
        &(),
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

// ---------------------------------------------------------------------------
// Space/set count-limit enforcement (bd:JMAP-g7wu.2.4.8)
//
// These tests install tight ChatLimits via MemoryBackend::set_limits_for_test
// to exercise the `overQuota` rejection path without seeding hundreds of
// objects. The oracle for the assertion shape is RFC 8620 §5.3 SetError
// (objects in `notUpdated[id]` with a `type` of "overQuota") plus the
// draft-atwood-jmap-chat-00 §Space/set normative requirement that an
// over-cap `add*` MUST return overQuota (spec commit `80d5e11`).
// ---------------------------------------------------------------------------

/// Oracle: a backend that returns the default ChatLimits passes a small
/// addCategories patch (well under the 100-category cap).
#[tokio::test]
async fn space_set_count_limits_default_caps_allow_normal_patch() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "Default Caps").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "Voice",
                        "position": 0,
                        "channelIds": [],
                    }]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"].is_null(),
        "default 100-category cap should not reject 1 add: {:?}",
        resp["notUpdated"]
    );
}

/// Oracle: addCategories beyond the cap rejects the whole update target
/// with an overQuota SetError naming the offending collection.
#[tokio::test]
async fn space_set_count_limits_add_categories_over_cap_rejects() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(
        jmap_chat_server::ChatLimits::default().with_max_categories_per_space(1),
    ));

    let space_id = make_space(&backend, "Tight Categories").await;

    // First add succeeds — fills the 1-category cap.
    let (resp1, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "Voice",
                        "position": 0,
                        "channelIds": [],
                    }]
                }
            }
        }),
    )
    .await
    .expect("first add");
    assert!(resp1["notUpdated"].is_null(), "first add should succeed");

    // Second add (any size) puts us over cap.
    let (resp2, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [{
                        "id": "placeholder",
                        "name": "Text",
                        "position": 1,
                        "channelIds": [],
                    }]
                }
            }
        }),
    )
    .await
    .expect("second add");

    assert!(
        resp2["notUpdated"][&space_id].is_object(),
        "second add must be rejected"
    );
    assert_eq!(resp2["notUpdated"][&space_id]["type"], "overQuota");
    let desc = resp2["notUpdated"][&space_id]["description"]
        .as_str()
        .expect("description");
    assert!(
        desc.contains("categories"),
        "description must name the categories collection: {desc:?}"
    );
    assert!(
        resp2["updated"].is_null(),
        "the rejected target must not also appear in updated"
    );
}

/// Oracle: a single addCategories op containing N entries that collectively
/// cross the cap is rejected atomically — none of the entries are applied.
#[tokio::test]
async fn space_set_count_limits_atomic_reject_whole_target() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(
        jmap_chat_server::ChatLimits::default().with_max_categories_per_space(2),
    ));

    let space_id = make_space(&backend, "Atomic Reject").await;

    // 3 categories in one op against a cap of 2 — entire target rejected.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [
                        { "id": "p1", "name": "A", "position": 0, "channelIds": [] },
                        { "id": "p2", "name": "B", "position": 1, "channelIds": [] },
                        { "id": "p3", "name": "C", "position": 2, "channelIds": [] },
                    ]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(resp["notUpdated"][&space_id]["type"], "overQuota");

    // Verify atomicity: zero categories actually landed on the Space.
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": "a1", "ids": [&space_id] }),
    )
    .await
    .expect("handle_space_get");
    let cats = get_resp["list"][0]["categories"]
        .as_array()
        .expect("categories array");
    assert_eq!(
        cats.len(),
        0,
        "no categories should have landed when the whole target is rejected"
    );
}

/// Oracle: addChannels at-cap is rejected. Channel count = uncategorized + categorized.
#[tokio::test]
async fn space_set_count_limits_add_channels_over_cap_rejects() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(
        jmap_chat_server::ChatLimits::default().with_max_channels_per_space(2),
    ));

    let space_id = make_space(&backend, "Tight Channels").await;

    // Fill the cap with two addChannels.
    let (resp1, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: { "addChannels": [{ "name": "general" }, { "name": "random" }] }
            }
        }),
    )
    .await
    .expect("first batch");
    assert!(
        resp1["notUpdated"].is_null(),
        "first batch should succeed: {:?}",
        resp1["notUpdated"]
    );

    // Third channel — over cap.
    let (resp2, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: { "addChannels": [{ "name": "off-topic" }] }
            }
        }),
    )
    .await
    .expect("over-cap add");

    assert_eq!(resp2["notUpdated"][&space_id]["type"], "overQuota");
    let desc = resp2["notUpdated"][&space_id]["description"]
        .as_str()
        .expect("description");
    assert!(
        desc.contains("channels"),
        "description must name the channels collection: {desc:?}"
    );
}

/// Oracle: a patch with both an over-cap addChannels and an under-cap
/// addCategories rejects the whole target with the first offender. The
/// reference handler surfaces the first cap to trip in struct-field
/// declaration order: roles, members, channels, categories.
#[tokio::test]
async fn space_set_count_limits_first_offender_surfaces() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(
        jmap_chat_server::ChatLimits::default()
            .with_max_channels_per_space(0)
            .with_max_categories_per_space(0),
    ));
    let space_id = make_space(&backend, "First Offender").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addChannels": [{ "name": "general" }],
                    "addCategories": [
                        { "id": "p1", "name": "Voice", "position": 0, "channelIds": [] }
                    ]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(resp["notUpdated"][&space_id]["type"], "overQuota");
    let desc = resp["notUpdated"][&space_id]["description"]
        .as_str()
        .expect("description");
    // The helper checks roles → members → channels → categories in
    // that order, so channels trips first when both are over cap.
    assert!(
        desc.contains("channels"),
        "channels should be reported first (before categories): {desc:?}"
    );
}

/// Oracle: a patch with zero Add* ops (only Remove/Update) is not
/// gated by limits — the handler skips the count-limit check entirely
/// when there are no Adds. The Forbidden return is from the
/// not-yet-implemented backend variants (bd:JMAP-g7wu.2.4.3), proving
/// the patch reached `apply_space_patch` rather than being short-
/// circuited by the cap check.
#[tokio::test]
async fn space_set_count_limits_no_add_ops_bypasses_check() {
    let backend = MemoryBackend::new();
    // Set roles cap to 0; if the check were running unconditionally, a
    // bare removeRoles patch would falsely trip a cap.
    backend.set_limits_for_test(Some(jmap_chat_server::ChatLimits::new(0, 0, 0, 0)));
    let space_id = make_space(&backend, "No-Add Patch").await;

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: { "removeRoles": ["nonexistent-role-id"] }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    // Backend stub returns Forbidden for unimplemented Role variants
    // (bd:JMAP-g7wu.2.4.3). The point of this assertion is *not*
    // overQuota — that's the failure we're guarding against.
    assert!(
        resp["notUpdated"][&space_id].is_object(),
        "Remove* patch should reach the backend stub: {:?}",
        resp["notUpdated"]
    );
    assert_ne!(
        resp["notUpdated"][&space_id]["type"], "overQuota",
        "no-Add* patches must not be cap-rejected"
    );
}

/// Oracle: a non-update Space/set request (pure create or pure
/// destroy) does not invoke the count-limit check. The `create`
/// arm has its own creation logic and does not pre-fetch the Space.
#[tokio::test]
async fn space_set_count_limits_create_only_unaffected() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(jmap_chat_server::ChatLimits::new(0, 0, 0, 0)));

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "create": { "s0": { "name": "Free Standing" } }
        }),
    )
    .await
    .expect("handle_space_set");

    // Space create itself contains no Add* ops; the cap check is not
    // invoked for pure create paths.
    assert!(
        resp["created"]["s0"].is_object(),
        "create should succeed regardless of cap settings: {:?}",
        resp
    );
}

// ---------------------------------------------------------------------------
// Backend-canonical cap enforcement (bd:JMAP-x2gd.44)
// ---------------------------------------------------------------------------
//
// These tests verify that the cap check fires when callers bypass the
// `handle_space_set` handler and call `apply_space_patch` (via the
// test-only `apply_space_patch_with_caller_id` entry point) directly.
// Per bd:JMAP-x2gd.44, cap enforcement is backend-canonical: a direct
// caller (admin tool, federation receiver, batch importer) must still
// see caps enforced.

/// Oracle: a direct call to the backend's structural-mutation API with
/// an Add* op that would push the Space over the roles cap MUST be
/// rejected at the backend, not silently allowed. The handler-level
/// pre-flight check does not run on this path.
#[tokio::test]
async fn apply_space_patch_direct_call_enforces_roles_cap() {
    use jmap_chat_server::SpacePatchOp;
    use jmap_chat_types::SpaceRole;

    let backend = MemoryBackend::new();
    // Tight cap: only 0 roles allowed.
    backend.set_limits_for_test(Some(jmap_chat_server::ChatLimits::new(0, 1024, 1024, 1024)));
    let space_id = make_space(&backend, "Direct Caller Cap Test").await;

    // Construct one AddRole op via JSON since SpaceRole is
    // #[non_exhaustive]. Without the backend-canonical cap enforcement,
    // this op would land successfully.
    let role: SpaceRole = serde_json::from_value(json!({
        "id": "placeholder",
        "name": "admin",
        "permissions": ["manage_space"],
        "position": 10
    }))
    .expect("deserialize SpaceRole");
    let ops = vec![SpacePatchOp::AddRole(role)];

    let result = backend.apply_space_patch_with_caller_id(
        None,
        &Id::from("a1"),
        &Id::from(space_id.as_str()),
        ops,
    );

    match result {
        Err(jmap_chat_server::BackendSetError::SetError(set_err)) => {
            let v = serde_json::to_value(&set_err).expect("set_err serializes");
            assert_eq!(
                v["type"], "overQuota",
                "Direct backend call must surface overQuota when caps would be exceeded: {v:?}"
            );
            assert!(
                v["description"]
                    .as_str()
                    .is_some_and(|s| s.contains("roles")),
                "overQuota description must name the offending collection: {v:?}"
            );
        }
        other => panic!("Direct backend call should be rejected with overQuota, got {other:?}"),
    }
}

/// Oracle: a no-Add* patch (e.g. RemoveRole with a missing id) called
/// directly on the backend with a roles cap of 0 must NOT trip the
/// cap check — only Add* ops are counted against the cap.
#[tokio::test]
async fn apply_space_patch_direct_call_no_add_ops_bypasses_cap_check() {
    use jmap_chat_server::SpacePatchOp;

    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(jmap_chat_server::ChatLimits::new(0, 0, 0, 0)));
    let space_id = make_space(&backend, "No-Add Direct").await;

    let ops = vec![SpacePatchOp::RemoveRole(Id::from("nonexistent-role-id"))];

    let result = backend.apply_space_patch_with_caller_id(
        None,
        &Id::from("a1"),
        &Id::from(space_id.as_str()),
        ops,
    );

    // The result should not be an overQuota — it should reach the
    // per-op apply path. (The actual outcome here depends on whether
    // the RemoveRole stub treats missing-id as Forbidden or NotFound;
    // the assertion is specifically that it is NOT overQuota.)
    match result {
        Ok(_) => {}
        Err(jmap_chat_server::BackendSetError::SetError(set_err)) => {
            let v = serde_json::to_value(&set_err).expect("set_err serializes");
            assert_ne!(
                v["type"], "overQuota",
                "no-Add* patches must NOT be cap-rejected at the backend: {v:?}"
            );
        }
        Err(other) => panic!("unexpected backend error: {other:?}"),
    }
}

/// Regression canary for bd:JMAP-x2gd.107.
///
/// The overQuota SetError emitted on per-Space aggregate cap violations
/// MUST name the offending aggregate but MUST NOT disclose:
///   - the current per-Space count (membership / role count / etc.)
///   - the per-account cap value (deployment policy, possibly tier-derived)
///   - any numeric quantity that lets a caller infer either
///
/// Both the handler-side defense-in-depth pre-flight
/// (`crate::space::check_space_count_limits`) and the backend-canonical
/// (`crate::memory::check_count_caps`) descriptions must satisfy this.
///
/// Oracle: independent of the code under test. We seed a Space with a
/// known small count, configure a known cap, send an over-cap add,
/// then assert structural invariants on the wire description: no ASCII
/// digit appears, and none of the back-channel keywords
/// (`existing`, `cap`, `would have`, `adding`) appear.
#[tokio::test]
async fn space_set_overquota_description_does_not_leak_counts_or_caps() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(
        jmap_chat_server::ChatLimits::default().with_max_categories_per_space(1),
    ));
    let space_id = make_space(&backend, "Leak Canary").await;

    // First add lands at-cap.
    let _ = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [
                        { "id": "p1", "name": "First", "position": 0, "channelIds": [] }
                    ]
                }
            }
        }),
    )
    .await
    .expect("first add");

    // Second add is over-cap and is rejected.
    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": "a1",
            "update": {
                &space_id: {
                    "addCategories": [
                        { "id": "p2", "name": "Second", "position": 1, "channelIds": [] }
                    ]
                }
            }
        }),
    )
    .await
    .expect("second add");

    assert_eq!(resp["notUpdated"][&space_id]["type"], "overQuota");
    let desc = resp["notUpdated"][&space_id]["description"]
        .as_str()
        .expect("description");

    // Structural invariant: the description must NOT contain any ASCII
    // digit. A numeric leak (cap value or current count) would manifest
    // as a digit run in the wire payload.
    assert!(
        !desc.chars().any(|c| c.is_ascii_digit()),
        "overQuota description must not contain ASCII digits (would leak count/cap): {desc:?}"
    );

    // Structural invariant: none of the pre-fix keywords appear. These
    // are the exact tokens the leaky format string contained — a
    // regression to that shape would surface here.
    for forbidden in &["existing", "cap", "would have", "adding"] {
        assert!(
            !desc.contains(forbidden),
            "overQuota description must not contain back-channel token {forbidden:?}: {desc:?}"
        );
    }

    // Positive check: the aggregate name is still surfaced for the
    // client's retry hint.
    assert!(
        desc.contains("categories"),
        "overQuota description must still name the offending aggregate: {desc:?}"
    );
}

/// Sibling regression canary for the backend-canonical path
/// (`apply_space_patch_with_caller_id` direct call). Same invariant as
/// the handler-side test above; both descriptions must be the same
/// shape for response-shape parity per bd:JMAP-x2gd.107.
#[tokio::test]
async fn apply_space_patch_direct_call_overquota_description_does_not_leak_counts_or_caps() {
    use jmap_chat_server::SpacePatchOp;

    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(jmap_chat_server::ChatLimits::new(0, 0, 0, 0)));
    let space_id = make_space(&backend, "Direct Leak Canary").await;

    let role_to_add = serde_json::from_value::<jmap_chat_types::SpaceRole>(json!({
        "id": "role-creation-1",
        "name": "Moderator",
        "permissions": [],
        "position": 1,
    }))
    .expect("SpaceRole deserialize");
    let ops = vec![SpacePatchOp::AddRole(role_to_add)];

    let result = backend.apply_space_patch_with_caller_id(
        None,
        &Id::from("a1"),
        &Id::from(space_id.as_str()),
        ops,
    );

    match result {
        Err(jmap_chat_server::BackendSetError::SetError(set_err)) => {
            let v = serde_json::to_value(&set_err).expect("set_err serializes");
            assert_eq!(v["type"], "overQuota");
            let desc = v["description"].as_str().expect("description");

            assert!(
                !desc.chars().any(|c| c.is_ascii_digit()),
                "overQuota description must not contain ASCII digits: {desc:?}"
            );
            for forbidden in &["existing", "cap", "would have", "adding"] {
                assert!(
                    !desc.contains(forbidden),
                    "overQuota description must not contain back-channel token {forbidden:?}: {desc:?}"
                );
            }
            assert!(
                desc.contains("roles"),
                "overQuota description must still name the offending aggregate: {desc:?}"
            );
        }
        other => panic!("expected overQuota SetError, got {other:?}"),
    }
}
