//! Wiremock smoke tests for `Message/*` method paths in jmap-chat-client.
//!
//! Pattern oracle (workspace canonical extension-client): see
//! `crate-jmap-mail-client/tests/thread_smoke_tests.rs` and
//! `crate-jmap-calendars-client/tests/event_smoke_tests.rs`.
//!
//! Spec oracles:
//!   - RFC 8620 §5.1 /get, §5.2 /changes, §5.3 /set, §5.5 /query, §5.6 /queryChanges
//!   - draft-atwood-jmap-chat-00 §4.5 (Message/* method-specific shapes,
//!     reaction patch JSON Pointer keys)

#[path = "helpers.rs"]
mod helpers;

use jmap_types::{Id, State, UTCDate};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// `Message/get` must reject an empty `ids` slice client-side
/// (message.rs:31-35); fetching all messages is impractical per the doc.
#[tokio::test]
async fn message_get_empty_ids_rejected_before_send() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);
    let empty: [Id; 0] = [];
    let err = sc
        .message_get(&empty, None)
        .await
        .expect_err("message_get must reject empty ids");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("ids may not be empty"),
                "error message must mention ids: got {msg:?}"
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
        "no HTTP request must be sent when ids is empty"
    );
}

/// `Message/get` with non-empty ids must thread the slice to the wire
/// `ids` array (message.rs:40-43) and omit `properties` when None.
#[tokio::test]
async fn message_get_threads_ids_and_omits_properties_when_none() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/get",
            {
                "accountId": "A13824",
                "state": "m-state-1",
                "list": [],
                "notFound": ["msg-1", "msg-2"]
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("msg-1"), Id::from("msg-2")];
    let _ = sc
        .message_get(&ids, None)
        .await
        .expect("message_get: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["ids"],
        json!(["msg-1", "msg-2"]),
        "ids must thread through verbatim"
    );
    assert!(
        args.get("properties").is_none(),
        "properties must be omitted when caller passes None"
    );
}

/// `Message/get` decode coverage: populated wire object must round-trip
/// through the [`jmap_chat_types::Message`] `Deserialize` impl with
/// every required field plus representative optionals (`replyTo`,
/// `editedAt`) and each nested collection (`attachments`, `mentions`,
/// `reactions`) populated with at least one entry. Without this test a
/// regression that broke `Message` deserialize would still pass every
/// other `Message/get` smoke test (they all return `"list": []`).
///
/// Also exercises the manual `SenderId` `Deserialize` (message.rs:74-92)
/// — the sentinel string `"self"` must map to `SenderId::Owner`.
///
/// Mirrors the canonical extension-client shape
/// `crate-jmap-calendars-client/tests/calendar_smoke_tests.rs::calendar_get_smoke`.
///
/// Oracles:
///   - draft-atwood-jmap-chat-00 §Message — Message object field set
///   - RFC 8620 §5.1 — /get response envelope
#[tokio::test]
async fn message_get_decodes_populated_message() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/get",
            {
                "accountId": "A13824",
                "state": "m-state-2",
                "list": [
                    {
                        "id": "msg-1",
                        "senderMsgId": "client-msg-1",
                        "senderId": "self",
                        "chatId": "chat-1",
                        "body": "Hello, world!",
                        "bodyType": "text/plain",
                        "attachments": [
                            {
                                "blobId": "blob-1",
                                "filename": "doc.pdf",
                                "contentType": "application/pdf",
                                "size": 12345,
                                "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                            }
                        ],
                        "mentions": [
                            {
                                "id": "u2",
                                "offset": 7,
                                "length": 5
                            }
                        ],
                        "actions": [],
                        "reactions": {
                            "self-r1": {
                                "emoji": "👍",
                                "senderId": "self",
                                "sentAt": "2026-01-20T14:30:00Z"
                            }
                        },
                        "sentAt": "2026-01-20T14:30:00Z",
                        "receivedAt": "2026-01-20T14:30:01Z",
                        "deliveryState": "delivered",
                        "replyTo": "msg-0",
                        "editedAt": "2026-01-20T14:35:00Z"
                    }
                ],
                "notFound": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("msg-1")];
    let resp = sc
        .message_get(&ids, None)
        .await
        .expect("message_get: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert_eq!(resp.state, "m-state-2", "state mismatch");
    assert_eq!(resp.list.len(), 1, "list must contain exactly one Message");

    let msg = &resp.list[0];
    assert_eq!(msg.id.as_ref(), "msg-1", "id mismatch");
    assert_eq!(
        msg.sender_msg_id.as_ref(),
        "client-msg-1",
        "senderMsgId mismatch"
    );
    assert!(
        matches!(msg.sender_id, jmap_chat_types::SenderId::Owner),
        "senderId 'self' must deserialise to SenderId::Owner, got {:?}",
        msg.sender_id
    );
    assert_eq!(msg.chat_id.as_ref(), "chat-1", "chatId mismatch");
    assert_eq!(msg.body, "Hello, world!", "body mismatch");
    assert_eq!(msg.body_type, "text/plain", "bodyType mismatch");
    assert!(
        matches!(
            msg.delivery_state,
            jmap_chat_types::DeliveryState::Delivered
        ),
        "deliveryState 'delivered' must deserialise to DeliveryState::Delivered, got {:?}",
        msg.delivery_state
    );
    assert_eq!(
        msg.sent_at.as_ref(),
        "2026-01-20T14:30:00Z",
        "sentAt mismatch"
    );
    assert_eq!(
        msg.received_at.as_ref(),
        "2026-01-20T14:30:01Z",
        "receivedAt mismatch"
    );

    assert_eq!(msg.attachments.len(), 1, "attachments must have 1 entry");
    assert_eq!(
        msg.attachments[0].blob_id.as_ref(),
        "blob-1",
        "attachments[0].blobId mismatch"
    );
    assert_eq!(
        msg.attachments[0].filename, "doc.pdf",
        "attachments[0].filename mismatch"
    );
    assert_eq!(
        msg.attachments[0].content_type, "application/pdf",
        "attachments[0].contentType mismatch"
    );
    assert_eq!(
        msg.attachments[0].size, 12345,
        "attachments[0].size mismatch"
    );

    assert_eq!(msg.mentions.len(), 1, "mentions must have 1 entry");
    assert_eq!(msg.mentions[0].id.as_ref(), "u2", "mentions[0].id mismatch");
    assert_eq!(msg.mentions[0].offset, 7, "mentions[0].offset mismatch");
    assert_eq!(msg.mentions[0].length, 5, "mentions[0].length mismatch");

    assert!(msg.actions.is_empty(), "actions must round-trip as empty");

    assert_eq!(msg.reactions.len(), 1, "reactions must have 1 entry");
    let reaction = msg
        .reactions
        .get("self-r1")
        .expect("reactions['self-r1'] must be present");
    assert_eq!(reaction.emoji, "👍", "reaction emoji mismatch");
    assert!(
        matches!(reaction.sender_id, jmap_chat_types::SenderId::Owner),
        "reaction.senderId 'self' must deserialise to SenderId::Owner"
    );

    assert_eq!(
        msg.reply_to.as_ref().map(|id| id.as_ref()),
        Some("msg-0"),
        "replyTo optional must round-trip"
    );
    assert_eq!(
        msg.edited_at.as_ref().map(|d| d.as_ref()),
        Some("2026-01-20T14:35:00Z"),
        "editedAt optional must round-trip"
    );
}

/// `Message/query` must require either `chat_id` or `has_mention=true`
/// (message.rs:67-71) — both omitted should short-circuit before send.
#[tokio::test]
async fn message_query_requires_chat_id_or_has_mention() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);

    let input = jmap_chat_client::methods::MessageQueryInput::default();
    let err = sc
        .message_query(&input)
        .await
        .expect_err("message_query must reject empty filter");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("chat_id or has_mention=true"),
                "error message must mention chat_id/has_mention: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
    let reqs = server
        .received_requests()
        .await
        .expect("recorded_requests must succeed");
    assert!(reqs.is_empty(), "no HTTP request must be sent");
}

/// `Message/query` with `chat_id` set must serialize a filter object
/// containing `chatId` and emit the default descending sort by `sentAt`
/// (message.rs:73-104).
#[tokio::test]
async fn message_query_chat_id_and_default_sort() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/query",
            {
                "accountId": "A13824",
                "queryState": "mq-1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["msg-1"]
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from("chat-1");
    let mut input = jmap_chat_client::methods::MessageQueryInput::default();
    input.chat_id = Some(&chat_id);
    let _ = sc
        .message_query(&input)
        .await
        .expect("message_query: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"],
        json!({ "chatId": "chat-1" }),
        "filter must contain chatId only"
    );
    // Default sort: sentAt descending (isAscending=false).
    assert_eq!(
        args["sort"],
        json!([{ "property": "sentAt", "isAscending": false }]),
        "default sort must be sentAt descending"
    );
}

/// `Message/query` with `sort_ascending=true` must flip the `isAscending`
/// field to `true` (message.rs:103, MessageQueryInput.sort_ascending).
#[tokio::test]
async fn message_query_sort_ascending() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/query",
            {
                "accountId": "A13824",
                "queryState": "mq-1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from("chat-1");
    let mut input = jmap_chat_client::methods::MessageQueryInput::default();
    input.chat_id = Some(&chat_id);
    input.sort_ascending = true;
    let _ = sc
        .message_query(&input)
        .await
        .expect("message_query: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["sort"],
        json!([{ "property": "sentAt", "isAscending": true }]),
        "sort must be sentAt ascending"
    );
}

/// `Message/changes` must thread `since_state` to `sinceState` and emit
/// `maxChanges` when provided (RFC 8620 §5.2).
#[tokio::test]
async fn message_changes_since_state_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/changes",
            {
                "accountId": "A13824",
                "oldState": "mc-old",
                "newState": "mc-new",
                "hasMoreChanges": false,
                "created": [],
                "updated": ["msg-1"],
                "destroyed": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let since = State::from("mc-old");
    let _ = sc
        .message_changes(&since, Some(100))
        .await
        .expect("message_changes: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["sinceState"], json!("mc-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(100), "maxChanges mismatch");
}

/// `Message/set` create with a happy-path response must serialize the
/// create object with `chatId`, `body`, `bodyType`, `sentAt` and the
/// caller-supplied `client_id` key (message.rs:172-185).
#[tokio::test]
async fn message_create_serializes_create_object() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/set",
            {
                "accountId": "A13824",
                "oldState": "ms-1",
                "newState": "ms-2",
                "created": {
                    "client-msg-1": { "id": "server-msg-1" }
                },
                "updated": null,
                "destroyed": null,
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from("chat-1");
    // 20-character UTCDate (RFC 8620 §1.4): "YYYY-MM-DDTHH:MM:SSZ".
    let sent_at = UTCDate::from("2024-06-15T09:00:00Z");
    let input = jmap_chat_client::methods::MessageCreateInput::new(
        &chat_id,
        "hello world",
        jmap_chat_client::types::BodyType::Plain,
        &sent_at,
    )
    .with_client_id("client-msg-1");
    let resp = sc
        .message_create(&input)
        .await
        .expect("message_create: must succeed");
    let created = resp.created.expect("created must be present");
    assert!(created.contains_key("client-msg-1"));

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    let create = &args["create"]["client-msg-1"];
    assert_eq!(create["chatId"], json!("chat-1"), "chatId mismatch");
    assert_eq!(create["body"], json!("hello world"), "body mismatch");
    assert_eq!(
        create["bodyType"],
        json!("text/plain"),
        "bodyType must serialize as text/plain"
    );
    assert_eq!(
        create["sentAt"],
        json!("2024-06-15T09:00:00Z"),
        "sentAt must thread through verbatim"
    );
    // `replyTo` was not set, must be absent.
    assert!(
        create.get("replyTo").is_none(),
        "replyTo must be absent when None"
    );
}

/// `Message/set` create that returns a `rateLimited` SetError on the
/// caller's creation key must surface as
/// `ClientError::RateLimited { retry_after }` (message.rs:188-202).
#[tokio::test]
async fn message_create_rate_limited_surfaces_as_error() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/set",
            {
                "accountId": "A13824",
                "oldState": "ms-1",
                "newState": "ms-1",
                "created": null,
                "updated": null,
                "destroyed": null,
                "notCreated": {
                    "client-msg-1": {
                        "type": "rateLimited",
                        "description": "slow down",
                        "serverRetryAfter": "2024-06-15T09:00:07Z"
                    }
                },
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from("chat-1");
    let sent_at = UTCDate::from("2024-06-15T09:00:00Z");
    let input = jmap_chat_client::methods::MessageCreateInput::new(
        &chat_id,
        "hello",
        jmap_chat_client::types::BodyType::Plain,
        &sent_at,
    )
    .with_client_id("client-msg-1");
    let err = sc
        .message_create(&input)
        .await
        .expect_err("message_create must surface rateLimited as error");
    match err {
        jmap_base_client::ClientError::RateLimited { retry_after } => {
            // `serverRetryAfter` is a *workspace convention* paired with
            // the `rateLimited` SetError type — it is NOT declared by
            // draft-atwood-jmap-chat-00 (the spec defines `slowModeSeconds`
            // on Chat objects but does not name `serverRetryAfter`) and it
            // is NOT declared by RFC 8620.
            //
            // The convention's canonical source is the chat-server
            // emission site at crate-jmap-chat-server/src/message.rs:457
            // (`SetError::with_extra("serverRetryAfter", ...)`), which
            // writes the field as a verbatim UTCDate string. The wire
            // payload on line 374 of this test was hand-crafted against
            // that canonical site, not generated by the production
            // parser, so the literal below is the independent oracle.
            //
            // This test pins the workspace wire contract pending IETF
            // stabilisation. If a future spec edit changes the field
            // shape (numeric seconds, RFC 7231 Retry-After, etc.), update
            // chat-server's emission, the client parser, AND this test
            // together.
            assert_eq!(
                retry_after.as_ref(),
                "2024-06-15T09:00:07Z",
                "retry_after must equal the wire serverRetryAfter UTCDate verbatim"
            );
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

/// `Message/set` update with `body` patch must emit `body` inside the
/// per-id patch object (message.rs:222-224, RFC 8620 §5.3).
#[tokio::test]
async fn message_update_body_patch_serializes() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/set",
            {
                "accountId": "A13824",
                "oldState": "ms-1",
                "newState": "ms-2",
                "created": null,
                "updated": { "msg-1": null },
                "destroyed": null,
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from("msg-1");
    let mut patch = jmap_chat_client::methods::MessagePatch::default();
    patch.body = Some("edited body");
    patch.body_type = Some(jmap_chat_client::types::BodyType::Markdown);
    let _ = sc
        .message_update(&msg_id, &patch)
        .await
        .expect("message_update: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    let patch_obj = &args["update"]["msg-1"];
    assert_eq!(patch_obj["body"], json!("edited body"), "body mismatch");
    assert_eq!(
        patch_obj["bodyType"],
        json!("text/markdown"),
        "bodyType must serialize as text/markdown"
    );
}

/// `Message/set` update with a reaction-add must emit a
/// `reactions/<senderReactionId>` JSON Pointer patch key containing
/// `{emoji, sentAt}` (message.rs:259-262, RFC 6901 / RFC 8620 §5.3).
#[tokio::test]
async fn message_update_reaction_add_uses_json_pointer_patch_key() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/set",
            {
                "accountId": "A13824",
                "oldState": "ms-1",
                "newState": "ms-2",
                "created": null,
                "updated": { "msg-1": null },
                "destroyed": null,
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from("msg-1");
    let reaction_sent_at = UTCDate::from("2024-06-15T09:01:00Z");
    let changes = [jmap_chat_client::methods::ReactionChange::Add {
        sender_reaction_id: "react-ulid-1",
        emoji: "👍",
        sent_at: &reaction_sent_at,
    }];
    let mut patch = jmap_chat_client::methods::MessagePatch::default();
    patch.reaction_changes = Some(&changes);
    let _ = sc
        .message_update(&msg_id, &patch)
        .await
        .expect("message_update: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    let patch_obj = &args["update"]["msg-1"];
    assert_eq!(
        patch_obj["reactions/react-ulid-1"],
        json!({ "emoji": "👍", "sentAt": "2024-06-15T09:01:00Z" }),
        "reaction-add must produce JSON Pointer patch key with emoji + sentAt"
    );
}

/// `Message/set` update with a reaction-remove must emit a
/// `reactions/<senderReactionId>` key with value `null`
/// (message.rs:277-280).
#[tokio::test]
async fn message_update_reaction_remove_emits_null_patch_value() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/set",
            {
                "accountId": "A13824",
                "oldState": "ms-1",
                "newState": "ms-2",
                "created": null,
                "updated": { "msg-1": null },
                "destroyed": null,
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from("msg-1");
    let changes = [jmap_chat_client::methods::ReactionChange::Remove {
        sender_reaction_id: "react-ulid-1",
    }];
    let mut patch = jmap_chat_client::methods::MessagePatch::default();
    patch.reaction_changes = Some(&changes);
    let _ = sc
        .message_update(&msg_id, &patch)
        .await
        .expect("message_update: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    let patch_obj = &args["update"]["msg-1"];
    assert_eq!(
        patch_obj["reactions/react-ulid-1"],
        json!(null),
        "reaction-remove must produce null patch value"
    );
}

/// `Message/set` update with a reaction whose `sender_reaction_id`
/// contains a JSON Pointer special character (`/` or `~` per RFC 6901)
/// must short-circuit client-side (message.rs:252-258, 270-276).
#[tokio::test]
async fn message_update_reaction_id_with_json_pointer_chars_rejected() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);

    let msg_id = Id::from("msg-1");
    let sent_at = UTCDate::from("2024-06-15T09:01:00Z");
    let changes = [jmap_chat_client::methods::ReactionChange::Add {
        sender_reaction_id: "bad/id",
        emoji: "👍",
        sent_at: &sent_at,
    }];
    let mut patch = jmap_chat_client::methods::MessagePatch::default();
    patch.reaction_changes = Some(&changes);
    let err = sc
        .message_update(&msg_id, &patch)
        .await
        .expect_err("message_update must reject JSON Pointer special chars");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("RFC 6901"),
                "error message must cite RFC 6901: got {msg:?}"
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
        "no HTTP request must be sent when reaction id contains forbidden chars"
    );
}

/// `Message/set` destroy must thread non-empty `ids` to the wire
/// `destroy` key and reject the empty slice client-side
/// (message.rs:301-318, RFC 8620 §5.3).
#[tokio::test]
async fn message_destroy_threads_ids_and_rejects_empty() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/set",
            {
                "accountId": "A13824",
                "oldState": "ms-1",
                "newState": "ms-2",
                "created": null,
                "updated": null,
                "destroyed": ["msg-1"],
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("msg-1")];
    let _ = sc
        .message_destroy(&ids)
        .await
        .expect("message_destroy: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["destroy"], json!(["msg-1"]), "destroy must thread");

    // Empty-slice guard.
    let empty: [Id; 0] = [];
    let err = sc
        .message_destroy(&empty)
        .await
        .expect_err("message_destroy must reject empty ids");
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

// ---------------------------------------------------------------------------
// readDisposition plumbing — draft-atwood-jmap-chat-00 §Message/set update
// (line 1012) + §ReadDisposition (lines 305-318).
//
// Each test sets `readAt` together with a different `readDisposition` value
// and asserts the per-id patch object on the wire contains both fields with
// the expected serialization. The `_omits_field` variant sets `readAt` alone
// and asserts the `readDisposition` key is absent so that the server's
// "default to displayed" path (§Message line 540) is reachable from the
// client side by omitting the field.
// ---------------------------------------------------------------------------

/// Helper: stock `Message/set` update response used by the readDisposition
/// plumbing tests. Bound to `msg-1`, account `A13824`.
fn message_set_update_resp() -> serde_json::Value {
    json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Message/set",
            {
                "accountId": "A13824",
                "oldState": "ms-1",
                "newState": "ms-2",
                "created": null,
                "updated": { "msg-1": null },
                "destroyed": null,
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    })
}

/// `MessagePatch.read_disposition = Some(Displayed)` together with `read_at`
/// must emit both `readAt` and `readDisposition: "displayed"` inside the
/// per-id patch object (spec §1012 + §ReadDisposition line 309).
#[tokio::test]
async fn message_update_with_read_disposition_displayed_serialises_correctly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(message_set_update_resp()))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from("msg-1");
    let read_at = UTCDate::from("2026-01-05T10:00:00Z");
    let mut patch = jmap_chat_client::methods::MessagePatch::default();
    patch.read_at = Some(&read_at);
    patch.read_disposition = Some(jmap_chat_types::ReadDisposition::Displayed);
    let _ = sc
        .message_update(&msg_id, &patch)
        .await
        .expect("message_update: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let patch_obj = &body["methodCalls"][0][1]["update"]["msg-1"];
    assert_eq!(
        patch_obj["readAt"],
        json!("2026-01-05T10:00:00Z"),
        "readAt must thread to the wire alongside readDisposition"
    );
    assert_eq!(
        patch_obj["readDisposition"],
        json!("displayed"),
        "Displayed must serialize as the wire string \"displayed\""
    );
}

/// `MessagePatch.read_disposition = Some(Deleted)` must emit
/// `readDisposition: "deleted"` (spec §ReadDisposition line 311).
#[tokio::test]
async fn message_update_with_read_disposition_deleted_serialises_correctly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(message_set_update_resp()))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from("msg-1");
    let read_at = UTCDate::from("2026-01-05T10:00:00Z");
    let mut patch = jmap_chat_client::methods::MessagePatch::default();
    patch.read_at = Some(&read_at);
    patch.read_disposition = Some(jmap_chat_types::ReadDisposition::Deleted);
    let _ = sc
        .message_update(&msg_id, &patch)
        .await
        .expect("message_update: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let patch_obj = &body["methodCalls"][0][1]["update"]["msg-1"];
    assert_eq!(patch_obj["readAt"], json!("2026-01-05T10:00:00Z"));
    assert_eq!(
        patch_obj["readDisposition"],
        json!("deleted"),
        "Deleted must serialize as the wire string \"deleted\""
    );
}

/// `MessagePatch.read_disposition = Some(Processed)` must emit
/// `readDisposition: "processed"` (spec §ReadDisposition line 313).
#[tokio::test]
async fn message_update_with_read_disposition_processed_serialises_correctly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(message_set_update_resp()))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from("msg-1");
    let read_at = UTCDate::from("2026-01-05T10:00:00Z");
    let mut patch = jmap_chat_client::methods::MessagePatch::default();
    patch.read_at = Some(&read_at);
    patch.read_disposition = Some(jmap_chat_types::ReadDisposition::Processed);
    let _ = sc
        .message_update(&msg_id, &patch)
        .await
        .expect("message_update: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let patch_obj = &body["methodCalls"][0][1]["update"]["msg-1"];
    assert_eq!(patch_obj["readAt"], json!("2026-01-05T10:00:00Z"));
    assert_eq!(
        patch_obj["readDisposition"],
        json!("processed"),
        "Processed must serialize as the wire string \"processed\""
    );
}

/// `MessagePatch.read_disposition = Some(Other("voice-listened"))` must emit
/// the literal wire string (spec §ReadDisposition line 318: unrecognized
/// values stored as-is; vendor extensions like `"voice-listened"` /
/// `"forwarded"` are explicitly called out).
#[tokio::test]
async fn message_update_with_read_disposition_other_serialises_correctly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(message_set_update_resp()))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from("msg-1");
    let read_at = UTCDate::from("2026-01-05T10:00:00Z");
    let mut patch = jmap_chat_client::methods::MessagePatch::default();
    patch.read_at = Some(&read_at);
    patch.read_disposition = Some(jmap_chat_types::ReadDisposition::Other(
        "voice-listened".into(),
    ));
    let _ = sc
        .message_update(&msg_id, &patch)
        .await
        .expect("message_update: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let patch_obj = &body["methodCalls"][0][1]["update"]["msg-1"];
    assert_eq!(patch_obj["readAt"], json!("2026-01-05T10:00:00Z"));
    assert_eq!(
        patch_obj["readDisposition"],
        json!("voice-listened"),
        "Other(\"voice-listened\") must round-trip to the wire as-is"
    );
}

/// `MessagePatch.read_at = Some(_)` alone (no `read_disposition`) must emit
/// `readAt` but NOT a `readDisposition` key, so the server's
/// "default to displayed" path (spec §Message line 540) is reachable from
/// the client side by omitting the field.
#[tokio::test]
async fn message_update_without_read_disposition_omits_field() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(message_set_update_resp()))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from("msg-1");
    let read_at = UTCDate::from("2026-01-05T10:00:00Z");
    let mut patch = jmap_chat_client::methods::MessagePatch::default();
    patch.read_at = Some(&read_at);
    // patch.read_disposition deliberately left as None.
    let _ = sc
        .message_update(&msg_id, &patch)
        .await
        .expect("message_update: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let patch_obj = &body["methodCalls"][0][1]["update"]["msg-1"];
    assert_eq!(
        patch_obj["readAt"],
        json!("2026-01-05T10:00:00Z"),
        "readAt must thread to the wire"
    );
    assert!(
        patch_obj.get("readDisposition").is_none(),
        "readDisposition key must be absent when read_disposition is None; got {:?}",
        patch_obj.get("readDisposition")
    );
}
