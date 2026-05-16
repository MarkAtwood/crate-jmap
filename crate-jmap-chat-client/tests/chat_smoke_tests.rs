//! Wiremock smoke tests for `Chat/*` method paths in jmap-chat-client.
//!
//! Focus: production-builder wire-shape assertions and basic round-trip
//! response decoding for the methods on [`jmap_chat_client::SessionClient`]
//! exposed by `src/methods/chat.rs`.
//!
//! Pattern oracle (workspace canonical extension-client): see
//! `crate-jmap-mail-client/tests/thread_smoke_tests.rs` and
//! `crate-jmap-calendars-client/tests/calendar_smoke_tests.rs` (the
//! per-method-family file split is the canonical-template shape).
//!
//! Spec oracles:
//!   - RFC 8620 §3.3 (`using`), §5.1 /get, §5.2 /changes, §5.3 /set,
//!     §5.5 /query, §5.6 /queryChanges
//!   - draft-atwood-jmap-chat-00 §3 (chat capability URI),
//!     §Chat/* (method-specific argument shapes)

#[path = "helpers.rs"]
mod helpers;

use helpers::{
    jmap_response, mock_jmap_post, recorded_args, recorded_body, set_destroy_response,
    set_response, CHAT_STATE_NEW, CHAT_STATE_OLD, TEST_ACCOUNT_ID,
};
use jmap_types::{Id, State};
use serde_json::json;
use wiremock::MockServer;

/// `Chat/get` with `ids: None, properties: None` must omit both keys on the
/// wire (chat.rs:35-43) — the conditional-add idiom prevents "present-but-null
/// vs absent" interop quirks documented in chat.rs:30-34.
#[tokio::test]
async fn chat_get_omits_ids_and_properties_when_none() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Chat/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "c-state-1",
            "list": [],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .chat_get(None, None)
        .await
        .expect("chat_get: must succeed");

    assert_eq!(
        resp.account_id.as_ref(),
        TEST_ACCOUNT_ID,
        "accountId mismatch"
    );
    assert_eq!(resp.state, "c-state-1", "state mismatch");
    assert!(resp.list.is_empty(), "list must be empty");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["accountId"],
        json!(TEST_ACCOUNT_ID),
        "accountId mismatch"
    );
    assert!(
        args.get("ids").is_none(),
        "ids must be omitted when caller passes None"
    );
    assert!(
        args.get("properties").is_none(),
        "properties must be omitted when caller passes None"
    );

    // RFC 8620 §3.3 — `using` MUST equal the exact USING_CHAT constant
    // (`core` + `chat`). assert_eq! on the full array (not the legacy
    // any() membership checks) so a regression that swapped in
    // USING_CHAT_PUSH or accidentally added an extra capability is
    // also caught.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]),
        "using must equal USING_CHAT exactly"
    );
}

/// `Chat/get` with explicit ids and properties must thread both arrays
/// through to the wire (chat.rs:36-42).
#[tokio::test]
async fn chat_get_threads_ids_and_properties_when_some() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Chat/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "c-state-1",
            "list": [],
            "notFound": ["chat-missing"]
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("chat-1"), Id::from("chat-missing")];
    let props = ["id", "name"];
    let _ = sc
        .chat_get(Some(&ids), Some(&props))
        .await
        .expect("chat_get: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["ids"],
        json!(["chat-1", "chat-missing"]),
        "ids must thread through verbatim"
    );
    assert_eq!(
        args["properties"],
        json!(["id", "name"]),
        "properties must thread through verbatim"
    );
}

/// `Chat/get` decode coverage: populated wire object must round-trip
/// through the [`jmap_chat_types::Chat`] `Deserialize` impl with every
/// required field plus representative optionals (`name`, `members`,
/// `lastMessageAt`) populated. Without this test a regression that
/// broke `Chat` deserialize would still pass every other `Chat/get`
/// smoke test in this file (they all return `"list": []`).
///
/// Mirrors the canonical extension-client shape
/// `crate-jmap-calendars-client/tests/calendar_smoke_tests.rs::calendar_get_smoke`.
///
/// Oracles:
///   - draft-atwood-jmap-chat-00 §Chat — Chat object field set
///   - RFC 8620 §5.1 — /get response envelope (`accountId`, `state`,
///     `list`, `notFound`)
#[tokio::test]
async fn chat_get_decodes_populated_chat() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Chat/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "c-state-2",
            "list": [
                {
                    "id": "chat-1",
                    "kind": "group",
                    "name": "Team Standup",
                    "createdAt": "2026-01-15T09:00:00Z",
                    "unreadCount": 3,
                    "pinnedMessageIds": ["msg-100"],
                    "muted": false,
                    "receiveTypingIndicators": true,
                    "members": [
                        {
                            "id": "u1",
                            "role": "owner",
                            "joinedAt": "2026-01-10T08:00:00Z"
                        },
                        {
                            "id": "u2",
                            "role": "member",
                            "joinedAt": "2026-01-12T10:30:00Z",
                            "invitedBy": "u1"
                        }
                    ],
                    "lastMessageAt": "2026-01-20T14:30:00Z"
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .chat_get(None, None)
        .await
        .expect("chat_get: must succeed");

    assert_eq!(
        resp.account_id.as_ref(),
        TEST_ACCOUNT_ID,
        "accountId mismatch"
    );
    assert_eq!(resp.state, "c-state-2", "state mismatch");
    assert_eq!(resp.list.len(), 1, "list must contain exactly one Chat");

    let chat = &resp.list[0];
    assert_eq!(chat.id.as_ref(), "chat-1", "id mismatch");
    assert!(
        matches!(chat.kind, jmap_chat_types::ChatKind::Group),
        "kind 'group' must deserialise to ChatKind::Group, got {:?}",
        chat.kind
    );
    assert_eq!(
        chat.created_at.as_ref(),
        "2026-01-15T09:00:00Z",
        "createdAt mismatch"
    );
    assert_eq!(chat.unread_count, 3, "unreadCount mismatch");
    assert!(!chat.muted, "muted must be false");
    assert!(
        chat.receive_typing_indicators,
        "receiveTypingIndicators must be true"
    );
    assert_eq!(
        chat.name.as_deref(),
        Some("Team Standup"),
        "name optional must round-trip"
    );
    assert_eq!(
        chat.pinned_message_ids.len(),
        1,
        "pinnedMessageIds must have 1 entry"
    );
    assert_eq!(
        chat.pinned_message_ids[0].as_ref(),
        "msg-100",
        "pinnedMessageIds[0] mismatch"
    );
    let members = chat
        .members
        .as_deref()
        .expect("members optional must deserialise to Some");
    assert_eq!(members.len(), 2, "members must have 2 entries");
    assert_eq!(members[0].id.as_ref(), "u1", "members[0].id mismatch");
    assert_eq!(members[0].role, "owner", "members[0].role mismatch");
    assert!(
        members[0].invited_by.is_none(),
        "members[0].invitedBy must be None"
    );
    assert_eq!(
        members[1].invited_by.as_ref().map(|id| id.as_ref()),
        Some("u1"),
        "members[1].invitedBy must round-trip to Some(\"u1\")"
    );
    assert_eq!(
        chat.last_message_at.as_ref().map(|d| d.as_ref()),
        Some("2026-01-20T14:30:00Z"),
        "lastMessageAt mismatch"
    );
}

/// `Chat/query` with no filter set must send `filter: null` (chat.rs:66-70)
/// while still threading position/limit.
#[tokio::test]
async fn chat_query_empty_filter_sends_null() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Chat/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": "qs-1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": ["chat-1", "chat-2"],
            "total": null,
            "limit": null
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let mut input = jmap_chat_client::methods::ChatQueryInput::default();
    input.position = Some(0);
    input.limit = Some(50);
    let _ = sc
        .chat_query(&input)
        .await
        .expect("chat_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"],
        json!(null),
        "filter must be JSON null when no fields are set"
    );
    assert_eq!(args["position"], json!(0), "position must thread through");
    assert_eq!(args["limit"], json!(50), "limit must thread through");
}

/// `Chat/query` with `filter_muted: Some(true)` must serialize a filter
/// object containing `{ "muted": true }` (chat.rs:63-65).
#[tokio::test]
async fn chat_query_filter_muted_serializes() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Chat/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": "qs-1",
            "canCalculateChanges": false,
            "position": 0,
            "ids": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let mut input = jmap_chat_client::methods::ChatQueryInput::default();
    input.filter_muted = Some(true);
    let _ = sc
        .chat_query(&input)
        .await
        .expect("chat_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"],
        json!({ "muted": true }),
        "filter object must contain muted=true"
    );
}

/// `Chat/changes` must thread `since_state` and `max_changes` through to
/// `sinceState` / `maxChanges` (RFC 8620 §5.2).
#[tokio::test]
async fn chat_changes_since_state_and_max_changes_passthrough() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Chat/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": "c-old",
            "newState": "c-new",
            "hasMoreChanges": false,
            "created": ["chat-new"],
            "updated": [],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("c-old");
    let resp = sc
        .chat_changes(&since, Some(50))
        .await
        .expect("chat_changes: must succeed");
    assert_eq!(resp.old_state, "c-old", "oldState mismatch");
    assert_eq!(resp.new_state, "c-new", "newState mismatch");

    let args = recorded_args(&server).await;
    assert_eq!(args["sinceState"], json!("c-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(50), "maxChanges mismatch");
}

/// `Chat/changes` with empty `since_state` must short-circuit client-side
/// before any HTTP request (chat.rs:99-103 defence-in-depth guard).
#[tokio::test]
async fn chat_changes_empty_since_state_rejected_before_send() {
    let server = MockServer::start().await;
    // No mock mounted: any HTTP request would result in a wiremock 404 that
    // would surface as a different error than InvalidArgument.

    let sc = helpers::make_client(&server);
    let empty = State::from("");
    let err = sc
        .chat_changes(&empty, None)
        .await
        .expect_err("chat_changes must reject empty since_state");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("since_state may not be empty"),
                "error message must explain the validation: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    let reqs = server
        .received_requests()
        .await
        .expect("recorded_requests must succeed");
    assert!(
        reqs.is_empty(),
        "no HTTP request must be sent when since_state is empty"
    );
}

/// `Chat/queryChanges` must thread `since_query_state` to `sinceQueryState`
/// (RFC 8620 §5.6).
#[tokio::test]
async fn chat_query_changes_since_query_state_passthrough() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Chat/queryChanges",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldQueryState": "qs-old",
            "newQueryState": "qs-new",
            "total": null,
            "removed": [],
            "added": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("qs-old");
    let _ = sc
        .chat_query_changes(&since, None)
        .await
        .expect("chat_query_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["sinceQueryState"],
        json!("qs-old"),
        "sinceQueryState mismatch"
    );
    assert!(
        args.get("maxChanges").is_none(),
        "maxChanges must be omitted when caller passes None"
    );
}

/// `Chat/typing` must emit `chatId` and `typing` on the wire alongside
/// `accountId` (chat.rs:131-136).
#[tokio::test]
async fn chat_typing_emits_chat_id_and_typing_flag() {
    let server = MockServer::start().await;
    let resp_body = jmap_response("Chat/typing", json!({ "accountId": TEST_ACCOUNT_ID }));
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from("chat-1");
    let _ = sc
        .chat_typing(&chat_id, true)
        .await
        .expect("chat_typing: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["chatId"], json!("chat-1"), "chatId mismatch");
    assert_eq!(args["typing"], json!(true), "typing flag mismatch");
}

/// `Chat/set` create (Direct) must serialize `kind:"direct"` and
/// `contactId` inside the `create` map, keyed by the caller-supplied
/// client id (chat.rs:186-196,250-254).
#[tokio::test]
async fn chat_create_direct_serializes_kind_and_contact_id() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Chat/set",
        CHAT_STATE_OLD,
        CHAT_STATE_NEW,
        json!({ "created": { "myKey": { "id": "chat-new-1" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let contact_id = Id::from("contact-bob");
    let input = jmap_chat_client::methods::ChatCreateInput::Direct {
        client_id: Some("myKey"),
        contact_id: &contact_id,
    };
    let resp = sc
        .chat_create(&input)
        .await
        .expect("chat_create: must succeed");
    let created = resp.created.expect("created must be present");
    assert!(created.contains_key("myKey"), "created must contain myKey");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["create"]["myKey"]["kind"],
        json!("direct"),
        "kind must be 'direct'"
    );
    assert_eq!(
        args["create"]["myKey"]["contactId"],
        json!("contact-bob"),
        "contactId must thread through"
    );
}

/// `Chat/set` create (Group) with empty `name` must short-circuit before
/// any HTTP request (chat.rs:205-209).
#[tokio::test]
async fn chat_create_group_empty_name_rejected_before_send() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);

    let input = jmap_chat_client::methods::ChatCreateInput::Group {
        client_id: None,
        name: "",
        member_ids: &[],
        description: None,
        avatar_blob_id: None,
        message_expiry_seconds: None,
    };
    let err = sc
        .chat_create(&input)
        .await
        .expect_err("chat_create with empty group name must error");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("name may not be empty"),
                "error message must mention name: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    let reqs = server
        .received_requests()
        .await
        .expect("recorded_requests must succeed");
    assert!(
        reqs.is_empty(),
        "no HTTP request must be sent when name is empty"
    );
}

/// `Chat/set` update with `muted: Some(true)` must produce a patch object
/// containing `{"muted": true}` keyed by the chat id, and emit
/// `Patch::Keep` fields as absent (chat.rs:276-279, RFC 8620 §5.3).
#[tokio::test]
async fn chat_update_patch_muted_serializes() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Chat/set",
        CHAT_STATE_OLD,
        CHAT_STATE_NEW,
        json!({ "updated": { "chat-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from("chat-1");
    let mut patch = jmap_chat_client::methods::ChatPatch::default();
    patch.muted = Some(true);
    let _ = sc
        .chat_update(&chat_id, &patch)
        .await
        .expect("chat_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["chat-1"],
        json!({ "muted": true }),
        "patch must contain only muted=true"
    );
}

/// `Chat/set` update with `message_expiry_seconds: Patch::Clear` must
/// emit JSON `null` (RFC 8620 §5.3 patch semantics) so the server
/// removes the local expiry policy. Pins the bd:JMAP-26di.8 fix that
/// changed the field type from `Option<u64>` (no null-clear capability)
/// to `Patch<u64>`; without `Patch::Clear`, the spec-defined "optional"
/// nullability (draft-atwood-jmap-chat-00 §Chat line 505) was
/// unreachable for callers.
#[tokio::test]
async fn chat_update_patch_message_expiry_seconds_clear_emits_null() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Chat/set",
        CHAT_STATE_OLD,
        CHAT_STATE_NEW,
        json!({ "updated": { "chat-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from("chat-1");
    let mut patch = jmap_chat_client::methods::ChatPatch::default();
    patch.message_expiry_seconds = jmap_chat_client::methods::Patch::Clear;
    let _ = sc
        .chat_update(&chat_id, &patch)
        .await
        .expect("chat_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["chat-1"],
        json!({ "messageExpirySeconds": null }),
        "Patch::Clear must serialise the field as JSON null"
    );
}

/// `Chat/set` update with `message_expiry_seconds: Patch::Set(N)` must
/// emit `N` verbatim on the wire (sibling of the Clear test above).
/// Together with the Clear test this exercises both halves of the
/// three-way Patch<u64> shape; the implicit third case (Patch::Keep)
/// is already covered by `chat_update_patch_muted_serializes`.
#[tokio::test]
async fn chat_update_patch_message_expiry_seconds_set_emits_value() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Chat/set",
        CHAT_STATE_OLD,
        CHAT_STATE_NEW,
        json!({ "updated": { "chat-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from("chat-1");
    let mut patch = jmap_chat_client::methods::ChatPatch::default();
    patch.message_expiry_seconds = jmap_chat_client::methods::Patch::Set(86_400);
    let _ = sc
        .chat_update(&chat_id, &patch)
        .await
        .expect("chat_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["chat-1"],
        json!({ "messageExpirySeconds": 86_400 }),
        "Patch::Set(N) must serialise the field as integer N"
    );
}

/// `Chat/set` destroy must thread the `ids` slice through to the `destroy`
/// wire key (chat.rs:387-391) and reject an empty slice client-side
/// (chat.rs:382-386, RFC 8620 §5.3).
#[tokio::test]
async fn chat_destroy_threads_ids_and_rejects_empty() {
    // Success path with a non-empty slice.
    let server = MockServer::start().await;
    let resp_body = set_destroy_response("Chat/set", CHAT_STATE_OLD, CHAT_STATE_NEW, "chat-doomed");
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("chat-doomed")];
    let resp = sc
        .chat_destroy(&ids)
        .await
        .expect("chat_destroy: must succeed");
    assert_eq!(
        resp.destroyed.as_deref(),
        Some(&[Id::from("chat-doomed")][..]),
        "destroyed must contain the chat id"
    );

    let args = recorded_args(&server).await;
    assert_eq!(
        args["destroy"],
        json!(["chat-doomed"]),
        "destroy ids must thread through"
    );
    assert!(
        args.get("create").is_none(),
        "create must be absent on destroy-only call"
    );

    // Empty-slice guard: no separate mock server is needed because the
    // guard fires before any HTTP request. Build a fresh sc against the
    // same server (mock pre-registered but irrelevant).
    let empty: [Id; 0] = [];
    let err = sc
        .chat_destroy(&empty)
        .await
        .expect_err("chat_destroy must reject empty ids");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("ids may not be empty"),
                "error message must mention ids: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
