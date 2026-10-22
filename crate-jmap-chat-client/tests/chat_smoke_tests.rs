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
use jmap_metadata_types::WithMetadata;
use jmap_types::{Filter, FilterOperator, Operator};
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
    assert_eq!(
        members[0].role.as_str(),
        "owner",
        "members[0].role mismatch"
    );
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
    let _ = sc
        .chat_query(
            None::<&jmap_chat_types::ChatFilter>,
            None,
            &jmap_base_client::QueryParams {
                position: Some(0),
                limit: Some(50),
                ..Default::default()
            },
        )
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
    let mut cond = jmap_chat_types::ChatFilterCondition::default();
    cond.muted = Some(true);
    let _ = sc
        .chat_query(
            Some(&Filter::Condition(cond)),
            None,
            &jmap_base_client::QueryParams::default(),
        )
        .await
        .expect("chat_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"],
        json!({ "muted": true }),
        "filter object must contain muted=true"
    );
}

/// RFC 8620 §5.5 /query arguments:
///
///   "anchor: "Id|null" ... If supplied, the "position" argument is
///   ignored."
///   "anchorOffset: "Int" (default: 0) ... This MAY be negative."
///   "calculateTotal: "Boolean" (default: false)"
#[tokio::test]
async fn chat_query_anchor_offset_calculate_total_serialize() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Chat/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": "qs-1",
            "canCalculateChanges": false,
            "position": 3,
            "ids": [],
            "total": 9
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let anchor = Id::from("chat-4");
    let _ = sc
        .chat_query(
            None::<&jmap_chat_types::ChatFilter>,
            None,
            &jmap_base_client::QueryParams {
                anchor: Some(anchor.clone()),
                anchor_offset: Some(-1),
                calculate_total: Some(true),
                ..Default::default()
            },
        )
        .await
        .expect("chat_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["anchor"],
        json!("chat-4"),
        "anchor must thread through"
    );
    assert_eq!(
        args["anchorOffset"],
        json!(-1),
        "negative anchorOffset must thread through"
    );
    assert_eq!(
        args["calculateTotal"],
        json!(true),
        "calculateTotal must thread through"
    );
}

/// `Chat/query` carries a draft-ietf-jmap-metadata-04 §3.6 condition on
/// a leaf.
///
/// Oracle: draft-atwood-jmap-chat-00 §Chat/query — "Filter properties:
/// `kind` (String), `muted` (Boolean)"; draft-ietf-jmap-metadata-04 §3.6 —
/// `privateMetadataExists: String` extends the type's FilterCondition.
/// The expected JSON is hand-written from those definitions.
#[tokio::test]
async fn chat_query_sends_metadata_condition_leaf() {
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

    let mut cond = WithMetadata::<jmap_chat_types::ChatFilterCondition>::default();
    cond.base.muted = Some(false);
    cond.private_metadata_exists = Some("acme.example.com/pinned".to_owned());

    let sc = helpers::make_client(&server);
    sc.chat_query(
        Some(&Filter::Condition(cond)),
        None,
        &jmap_base_client::QueryParams::default(),
    )
    .await
    .expect("chat_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"],
        json!({ "muted": false, "privateMetadataExists": "acme.example.com/pinned" })
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
        .chat_query_changes(
            &since,
            None,
            None::<&jmap_chat_types::ChatFilter>,
            None,
            None,
            None,
        )
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
        .chat_create(&input, None)
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
        .chat_create(&input, None)
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
    let patch = jmap_chat_client::methods::ChatPatch {
        muted: Some(true),
        ..Default::default()
    };
    let _ = sc
        .chat_update(&chat_id, &patch, None)
        .await
        .expect("chat_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["chat-1"],
        json!({ "muted": true }),
        "patch must contain only muted=true"
    );
}

/// A downstream crate builds `ChatPatch` with struct-update syntax
/// (`..Default::default()`), the construction idiom shared with the
/// sibling clients' `*Params` types (bd:JMAP-ops7.22). Oracle:
/// draft-atwood-jmap-chat-00 §Chat/set line 1110 at spec 6bcd9d8 —
/// "`update` supports the following patch keys for all chat kinds:
/// `muted`, `muteUntil`, ..." — so the wire patch is exactly those two
/// keys, with every defaulted field absent.
#[tokio::test]
async fn chat_update_patch_struct_update_syntax_serializes() {
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
    let until = jmap_types::UTCDate::from("2024-06-16T09:00:00Z");
    let patch = jmap_chat_client::methods::ChatPatch {
        muted: Some(true),
        mute_until: jmap_chat_client::methods::Patch::Set(&until),
        ..Default::default()
    };
    let _ = sc
        .chat_update(&chat_id, &patch, None)
        .await
        .expect("chat_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["chat-1"],
        json!({ "muted": true, "muteUntil": "2024-06-16T09:00:00Z" }),
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
    let patch = jmap_chat_client::methods::ChatPatch {
        message_expiry_seconds: jmap_chat_client::methods::Patch::Clear,
        ..Default::default()
    };
    let _ = sc
        .chat_update(&chat_id, &patch, None)
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
    let patch = jmap_chat_client::methods::ChatPatch {
        message_expiry_seconds: jmap_chat_client::methods::Patch::Set(86_400),
        ..Default::default()
    };
    let _ = sc
        .chat_update(&chat_id, &patch, None)
        .await
        .expect("chat_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["chat-1"],
        json!({ "messageExpirySeconds": 86_400 }),
        "Patch::Set(N) must serialise the field as integer N"
    );
}

/// `Chat/set` destroy must thread the `ids` slice through to the
/// `destroy` wire key (chat.rs:387-391). Empty-slice rejection lives
/// in the paired [`chat_destroy_rejects_empty_ids`] test below.
#[tokio::test]
async fn chat_destroy_threads_ids() {
    let server = MockServer::start().await;
    let resp_body = set_destroy_response("Chat/set", CHAT_STATE_OLD, CHAT_STATE_NEW, "chat-doomed");
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("chat-doomed")];
    let resp = sc
        .chat_destroy(&ids, None)
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
}

/// `Chat/set` destroy with an empty ids slice MUST be rejected
/// client-side before any HTTP call, surfacing
/// `ClientError::InvalidArgument`. Paired with
/// [`chat_destroy_threads_ids`] above which exercises the happy path.
#[tokio::test]
async fn chat_destroy_rejects_empty_ids() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);
    let empty: [Id; 0] = [];
    let err = sc
        .chat_destroy(&empty, None)
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

/// `Chat/set` update with `name`, `description: Patch::Set`, and
/// `avatar_blob_id: Patch::Clear` must thread all three through the
/// per-id patch object with camelCase wire keys
/// (chat.rs:236-279, RFC 8620 §5.3).
#[tokio::test]
async fn chat_update_patch_name_description_avatar_serialises() {
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
    let patch = jmap_chat_client::methods::ChatPatch {
        name: Some("Renamed Group"),
        description: jmap_chat_client::methods::Patch::Set("New topic for the group"),
        avatar_blob_id: jmap_chat_client::methods::Patch::Clear,
        ..Default::default()
    };
    let _ = sc
        .chat_update(&chat_id, &patch, None)
        .await
        .expect("chat_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"]["chat-1"];
    assert_eq!(patch_obj["name"], json!("Renamed Group"), "name mismatch");
    assert_eq!(
        patch_obj["description"],
        json!("New topic for the group"),
        "description Patch::Set must thread verbatim"
    );
    assert_eq!(
        patch_obj["avatarBlobId"],
        json!(null),
        "avatarBlobId Patch::Clear must serialise as JSON null"
    );
    // muted left at None must be absent.
    assert!(
        patch_obj.get("muted").is_none(),
        "muted must be absent when None"
    );
}

/// `Chat/set` update with `add_members` (with role) and `remove_members`
/// must serialise both arrays under camelCase keys, with each
/// `AddMemberInput` carrying `id` + optional `role` (chat.rs add_members
/// branch). Confirms the Admin vs Member wire-string serialisation
/// via the typed ChatMemberRole enum.
#[tokio::test]
async fn chat_update_patch_add_and_remove_members_serialises() {
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
    let alice_id = Id::from("u-alice");
    let bob_id = Id::from("u-bob");
    let add_alice = jmap_chat_client::methods::AddMemberInput {
        id: &alice_id,
        role: Some(jmap_chat_client::types::ChatMemberRole::Admin),
    };
    let add_bob = jmap_chat_client::methods::AddMemberInput {
        id: &bob_id,
        role: None,
    };
    let add_members = [add_alice, add_bob];
    let removed_id = Id::from("u-malice");
    let remove_members = [removed_id];

    let patch = jmap_chat_client::methods::ChatPatch {
        add_members: Some(&add_members),
        remove_members: Some(&remove_members),
        ..Default::default()
    };
    let _ = sc
        .chat_update(&chat_id, &patch, None)
        .await
        .expect("chat_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"]["chat-1"];
    assert_eq!(
        patch_obj["addMembers"],
        json!([
            { "id": "u-alice", "role": "admin" },
            { "id": "u-bob" }
        ]),
        "addMembers must thread id + optional role with lowercase wire strings"
    );
    assert_eq!(
        patch_obj["removeMembers"],
        json!(["u-malice"]),
        "removeMembers must thread the id slice"
    );
}

/// `Chat/set` update with `update_member_roles` must serialise an array
/// of `{id, role}` objects. Confirms the `Member` wire string and the
/// `Other(String)` round-trip (vendor-defined wire string is preserved
/// verbatim per workspace `Other(String)` policy).
#[tokio::test]
async fn chat_update_patch_update_member_roles_serialises() {
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
    let alice_id = Id::from("u-alice");
    let bob_id = Id::from("u-bob");
    let role_changes = [
        jmap_chat_client::methods::UpdateMemberRoleInput {
            id: &alice_id,
            role: jmap_chat_client::types::ChatMemberRole::Member,
        },
        // Vendor-defined role survives via Other(String) at the
        // serialize boundary; the wire string is whatever the caller
        // supplied (workspace policy on Other(String) preservation
        // for round-trip).
        jmap_chat_client::methods::UpdateMemberRoleInput {
            id: &bob_id,
            role: jmap_chat_client::types::ChatMemberRole::Other("moderator".into()),
        },
    ];
    let patch = jmap_chat_client::methods::ChatPatch {
        update_member_roles: Some(&role_changes),
        ..Default::default()
    };
    let _ = sc
        .chat_update(&chat_id, &patch, None)
        .await
        .expect("chat_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["chat-1"]["updateMemberRoles"],
        json!([
            { "id": "u-alice", "role": "member" },
            { "id": "u-bob", "role": "moderator" }
        ]),
        "updateMemberRoles must thread well-known + Other(String) wire strings"
    );
}

/// `Chat/queryChanges` with filter, sort, upToId, and calculateTotal must
/// emit all four optional args on the wire (RFC 8620 §5.6).
///
/// Oracle: draft-atwood-jmap-chat-00 — `kind` is a valid Chat filter
/// field; the comparator shape (property + isAscending) follows
/// RFC 8620 §5.5.
#[tokio::test]
async fn chat_query_changes_with_filter_sort_upto_calculatetotal() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Chat/queryChanges",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldQueryState": "qs-old",
            "newQueryState": "qs-new",
            "total": 0,
            "removed": [],
            "added": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("qs-old");
    let up_to = Id::from("CH-100");
    let mut cond = jmap_chat_types::ChatFilterCondition::default();
    cond.kind = Some(jmap_chat_types::ChatKind::Group);
    // draft-atwood-jmap-chat-00 §Chat/query: "Default sort:
    // `lastMessageAt` descending". (`lastSeenAt` is a ChatContact property.)
    let mut cmp = jmap_chat_types::ChatComparator::new(
        jmap_chat_types::ChatComparatorProperty::LastMessageAt,
    );
    cmp.is_ascending = false;
    sc.chat_query_changes(
        &since,
        None,
        Some(&jmap_chat_types::Filter::Condition(cond)),
        Some(&[cmp]),
        Some(&up_to),
        Some(true),
    )
    .await
    .expect("chat_query_changes_with_filter_sort_upto_calculatetotal: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"]["kind"],
        json!("group"),
        "filter.kind must be 'group'"
    );
    assert_eq!(
        args["sort"][0]["property"],
        json!("lastMessageAt"),
        "sort[0].property must be 'lastMessageAt'"
    );
    assert_eq!(
        args["upToId"],
        json!("CH-100"),
        "upToId must be on the wire (RFC 8620 §5.6)"
    );
    assert_eq!(
        args["calculateTotal"],
        json!(true),
        "calculateTotal must be on the wire (RFC 8620 §5.6)"
    );
}

/// `Chat/queryChanges` with all None optional args must NOT emit any of
/// filter/sort/upToId/calculateTotal/maxChanges on the wire.
///
/// Oracle: RFC 8620 §5.6 — all five are optional; the wire shape with
/// `None` for each must be byte-identical to the minimal
/// `sinceQueryState`-only call.
#[tokio::test]
async fn chat_query_changes_all_none_omits_optional_wire_keys() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Chat/queryChanges",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldQueryState": "qs-old",
            "newQueryState": "qs-new",
            "total": 0,
            "removed": [],
            "added": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("qs-old");
    sc.chat_query_changes(
        &since,
        None,
        None::<&jmap_chat_types::ChatFilter>,
        None,
        None,
        None,
    )
    .await
    .expect("chat_query_changes_all_none_omits_optional_wire_keys: must succeed");

    let args = recorded_args(&server).await;
    assert!(args.get("filter").is_none(), "filter must be omitted");
    assert!(args.get("sort").is_none(), "sort must be omitted");
    assert!(args.get("upToId").is_none(), "upToId must be omitted");
    assert!(
        args.get("calculateTotal").is_none(),
        "calculateTotal must be omitted"
    );
    assert!(
        args.get("maxChanges").is_none(),
        "maxChanges must be omitted"
    );
}

/// `Chat/queryChanges` carries a compound filter whose leaf adds a
/// draft-ietf-jmap-metadata-04 §3.6 condition.
///
/// Oracle: draft-atwood-jmap-chat-00 §Chat/query — "Filter properties:
/// `kind` (String), `muted` (Boolean)"; draft-ietf-jmap-metadata-04 §3.6 —
/// `privateMetadataExists: String` extends the type's FilterCondition;
/// RFC 8620 §5.5 — FilterOperator `operator` / `conditions`. The expected
/// JSON is hand-written from those definitions.
#[tokio::test]
async fn chat_query_changes_sends_metadata_condition_in_and() {
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

    let mut muted = WithMetadata::<jmap_chat_types::ChatFilterCondition>::default();
    muted.base.muted = Some(true);
    let mut pinned = WithMetadata::<jmap_chat_types::ChatFilterCondition>::default();
    pinned.private_metadata_exists = Some("acme.example.com/pinned".to_owned());
    let filter = Filter::Operator(FilterOperator::new(
        Operator::And,
        vec![Filter::Condition(muted), Filter::Condition(pinned)],
    ));

    let sc = helpers::make_client(&server);
    sc.chat_query_changes(
        &State::from("qs-old"),
        None,
        Some(&filter),
        None,
        None,
        None,
    )
    .await
    .expect("chat_query_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"],
        json!({
            "operator": "AND",
            "conditions": [
                { "muted": true },
                { "privateMetadataExists": "acme.example.com/pinned" }
            ]
        })
    );
}

/// RFC 8620 §5.3: each `created` value holds only "properties of the
/// created Foo object that were not sent by the client". Oracle: the
/// hand-written response echoes the server-set `id` and `createdAt`;
/// the client-sent `kind` and `name` are absent. The typed
/// `PartialObject` exposes the id directly.
#[tokio::test]
async fn chat_create_created_is_partial_object() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Chat/set",
        CHAT_STATE_OLD,
        CHAT_STATE_NEW,
        json!({ "created": { "g1": { "id": "chat-9", "createdAt": "2026-10-21T10:00:00Z" } } }),
    );
    mock_jmap_post(&server, resp_body).await;
    let sc = helpers::make_client(&server);
    let input = jmap_chat_client::methods::ChatCreateInput::Group {
        client_id: Some("g1"),
        name: "Team",
        member_ids: &[],
        description: None,
        avatar_blob_id: None,
        message_expiry_seconds: None,
    };
    let resp = sc
        .chat_create(&input, None)
        .await
        .expect("a partial created object must parse");
    let g1 = &resp.created.expect("created")["g1"];
    assert_eq!(g1.id, Some(Id::from("chat-9")));
    assert_eq!(g1.extra["createdAt"], "2026-10-21T10:00:00Z");
    assert!(!g1.extra.contains_key("name"));
}
