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

use helpers::{
    MESSAGE_STATE_NEW, MESSAGE_STATE_OLD, QUERY_STATE_NEW, QUERY_STATE_OLD, TEST_ACCOUNT_ID,
    TEST_CHAT_ID, TEST_MESSAGE_ID, TEST_UTC_DATE, TEST_UTC_DATE_LATER, jmap_response,
    mock_jmap_post, recorded_args, recorded_body, set_response, set_update_response,
};
use jmap_types::{Filter, FilterOperator, Operator};
use jmap_types::{Id, State, UTCDate};
use serde_json::json;
use wiremock::MockServer;

/// `Message/get` with `ids: None` must send the request with `ids` omitted.
/// RFC 8620 §5.1: "ids: Id[]|null ... If null, then all records of the data
/// type are returned, if this is supported for that data type and the number
/// of records does not exceed the maxObjectsInGet limit."
/// draft-atwood-jmap-chat-00 §Message/get: "Standard JMAP `/get`." — so the
/// limit is the server's to enforce, not the client's.
#[tokio::test]
async fn message_get_none_ids_omits_ids_on_wire() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": MESSAGE_STATE_OLD,
            "list": [],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .message_get(None, None)
        .await
        .expect("message_get(None): must succeed");
    assert_eq!(resp.state, MESSAGE_STATE_OLD, "state mismatch");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(args, &json!({ "accountId": TEST_ACCOUNT_ID }));
}

/// `Message/get` with non-empty ids must thread the slice to the wire
/// `ids` array (message.rs:40-43) and omit `properties` when None.
#[tokio::test]
async fn message_get_threads_ids_and_omits_properties_when_none() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": MESSAGE_STATE_OLD,
            "list": [],
            "notFound": [TEST_MESSAGE_ID, "msg-2"]
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from(TEST_MESSAGE_ID), Id::from("msg-2")];
    let _ = sc
        .message_get(Some(&ids), None)
        .await
        .expect("message_get: must succeed");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["ids"],
        json!([TEST_MESSAGE_ID, "msg-2"]),
        "ids must thread through verbatim"
    );
    assert!(
        args.get("properties").is_none(),
        "properties must be omitted when caller passes None"
    );
    // RFC 8620 §3.3 — Message/* methods MUST declare USING_CHAT
    // (`core` + `chat`). assert_eq! on the full array so a regression
    // that swapped to USING_CHAT_PUSH or added an extra capability is
    // also caught. One assertion per method-family per bd:JMAP-26di.10
    // — every other Message/* smoke test in this file goes through the
    // same build_request site and inherits the constant.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]),
        "Message/* using must equal USING_CHAT exactly"
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
/// The owner's `senderId` is the accountId (draft-atwood-jmap-chat-00
/// §Message `senderId`: "The accountId for owner-composed messages").
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
    let resp_body = jmap_response(
        "Message/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": MESSAGE_STATE_NEW,
            "list": [
                {
                    "id": TEST_MESSAGE_ID,
                    "senderMsgId": "client-msg-1",
                    "senderId": TEST_ACCOUNT_ID,
                    "chatId": TEST_CHAT_ID,
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
                            "id": "u2"
                        }
                    ],
                    "broadcastMentions": [],
                    "actions": [],
                    "reactions": {
                        "self-r1": {
                            "emoji": "👍",
                            "senderId": TEST_ACCOUNT_ID,
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
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from(TEST_MESSAGE_ID)];
    let resp = sc
        .message_get(Some(&ids), None)
        .await
        .expect("message_get: must succeed");

    assert_eq!(
        resp.account_id.as_ref(),
        TEST_ACCOUNT_ID,
        "accountId mismatch"
    );
    assert_eq!(resp.state, MESSAGE_STATE_NEW, "state mismatch");
    assert_eq!(resp.list.len(), 1, "list must contain exactly one Message");

    let msg = &resp.list[0];
    assert_eq!(msg.id.as_ref(), TEST_MESSAGE_ID, "id mismatch");
    assert_eq!(
        msg.sender_msg_id.as_ref(),
        "client-msg-1",
        "senderMsgId mismatch"
    );
    assert_eq!(msg.sender_id.as_ref(), TEST_ACCOUNT_ID, "senderId mismatch");
    assert_eq!(msg.chat_id.as_ref(), TEST_CHAT_ID, "chatId mismatch");
    assert_eq!(msg.body, "Hello, world!", "body mismatch");
    assert_eq!(
        msg.body_type,
        jmap_chat_client::BodyType::Plain,
        "bodyType mismatch"
    );
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
    // §Mention: "A Mention carries no position in `body`."
    assert_eq!(msg.mentions[0].id.as_ref(), "u2", "mentions[0].id mismatch");
    assert!(
        msg.mentions[0].extra.is_empty(),
        "mentions[0] has no other fields"
    );

    assert!(msg.actions.is_empty(), "actions must round-trip as empty");

    assert_eq!(msg.reactions.len(), 1, "reactions must have 1 entry");
    let reaction = msg
        .reactions
        .get("self-r1")
        .expect("reactions['self-r1'] must be present");
    assert_eq!(reaction.emoji, "👍", "reaction emoji mismatch");
    assert_eq!(
        reaction.sender_id.as_ref(),
        TEST_ACCOUNT_ID,
        "reaction senderId mismatch"
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

/// `Message/query` must require either `chat_id` or `has_mention=true`;
/// with no filter at all both are omitted, so it short-circuits before
/// send.
#[tokio::test]
async fn message_query_requires_chat_id_or_has_mention() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);

    let err = sc
        .message_query(
            None::<&jmap_chat_types::MessageFilter>,
            None,
            &jmap_base_client::QueryParams::default(),
        )
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
/// containing `chatId` and, with `sort: None`, omit `sort` so the server
/// default applies. Oracle: draft-atwood-jmap-chat-00 §Message/query
/// (jmap-chat-spec 29e9a41 L1295): "Default sort: `receivedAt` ascending
/// when `chatId` is present; `receivedAt` descending when `chatId` is
/// absent."; RFC 8620 §5.5 lists `sort` as optional.
#[tokio::test]
async fn message_query_chat_id_and_default_sort() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": QUERY_STATE_OLD,
            "canCalculateChanges": true,
            "position": 0,
            "ids": [TEST_MESSAGE_ID]
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let _ = sc
        .message_query(
            Some(&chat_filter()),
            None,
            &jmap_base_client::QueryParams::default(),
        )
        .await
        .expect("message_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"],
        json!({ "chatId": TEST_CHAT_ID }),
        "filter must contain chatId only"
    );
    assert!(
        args.get("sort").is_none(),
        "sort: None must omit sort so the server default applies, got {args}"
    );
}

/// `Message/query` with an ascending `receivedAt` comparator must send
/// `isAscending: true` (RFC 8620 §5.5 Comparator).
#[tokio::test]
async fn message_query_sort_ascending() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": QUERY_STATE_OLD,
            "canCalculateChanges": true,
            "position": 0,
            "ids": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let sort = [jmap_chat_types::MessageComparator::new(
        jmap_chat_types::MessageComparatorProperty::ReceivedAt,
    )];
    let _ = sc
        .message_query(
            Some(&chat_filter()),
            Some(&sort),
            &jmap_base_client::QueryParams::default(),
        )
        .await
        .expect("message_query: must succeed");

    let args = recorded_args(&server).await;
    // RFC 8620 §5.5 isAscending "(default: true)": omitted when ascending.
    assert_eq!(
        args["sort"],
        json!([{ "property": "receivedAt" }]),
        "sort must be receivedAt ascending"
    );
}

/// draft-atwood-jmap-chat-00 §Message/query filter properties:
///
///   "`topicId` (String, optional): Return only messages whose `topicId`
///   equals this value."
#[tokio::test]
async fn message_query_topic_id_filter_serializes() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": QUERY_STATE_OLD,
            "canCalculateChanges": true,
            "position": 0,
            "ids": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let mut cond = jmap_chat_types::MessageFilterCondition::default();
    cond.chat_id = Some(Id::from(TEST_CHAT_ID));
    cond.topic_id = Some("topic-1".to_owned());
    let _ = sc
        .message_query(
            Some(&Filter::Condition(cond)),
            None,
            &jmap_base_client::QueryParams::default(),
        )
        .await
        .expect("message_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"],
        json!({ "chatId": TEST_CHAT_ID, "topicId": "topic-1" }),
        "filter must carry topicId verbatim"
    );
}

/// RFC 8620 §5.5 /query arguments:
///
///   "anchor: "Id|null" ... If supplied, the "position" argument is
///   ignored."
///   "anchorOffset: "Int" (default: 0) ... This MAY be negative."
///   "calculateTotal: "Boolean" (default: false)"
#[tokio::test]
async fn message_query_anchor_offset_calculate_total_serialize() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": QUERY_STATE_OLD,
            "canCalculateChanges": true,
            "position": 10,
            "ids": [],
            "total": 42
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let anchor = Id::from("msg-7");
    let _ = sc
        .message_query(
            Some(&chat_filter()),
            None,
            &jmap_base_client::QueryParams {
                anchor: Some(anchor.clone()),
                anchor_offset: Some(-5),
                calculate_total: Some(true),
                ..Default::default()
            },
        )
        .await
        .expect("message_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["anchor"], json!("msg-7"), "anchor must thread through");
    assert_eq!(
        args["anchorOffset"],
        json!(-5),
        "negative anchorOffset must thread through"
    );
    assert_eq!(
        args["calculateTotal"],
        json!(true),
        "calculateTotal must thread through"
    );
}

/// `Message/query` carries a compound filter with a `NOT` operator.
///
/// Oracle: RFC 8620 §5.5 — a FilterOperator has `operator` ("AND" |
/// "OR" | "NOT"; "NOT: None of the conditions must match") and
/// `conditions`; draft-atwood-jmap-chat-00 §Message/query — "All requests
/// MUST include a `chatId` filter" and the `hasAttachment` (Boolean)
/// filter property. The expected JSON is hand-written from those
/// definitions.
#[tokio::test]
async fn message_query_sends_not_operator() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": QUERY_STATE_OLD,
            "canCalculateChanges": true,
            "position": 0,
            "ids": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let mut with_attachment = jmap_chat_types::MessageFilterCondition::default();
    with_attachment.has_attachment = Some(true);
    let filter = Filter::Operator(FilterOperator::new(
        Operator::And,
        vec![
            chat_filter(),
            Filter::Operator(FilterOperator::new(
                Operator::Not,
                vec![Filter::Condition(with_attachment)],
            )),
        ],
    ));

    let sc = helpers::make_client(&server);
    sc.message_query(
        Some(&filter),
        None,
        &jmap_base_client::QueryParams::default(),
    )
    .await
    .expect("message_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"],
        json!({
            "operator": "AND",
            "conditions": [
                { "chatId": TEST_CHAT_ID },
                { "operator": "NOT", "conditions": [{ "hasAttachment": true }] }
            ]
        })
    );
}

/// A `Message/query` filter with only `chatId: TEST_CHAT_ID`.
fn chat_filter() -> jmap_chat_types::MessageFilter {
    let mut cond = jmap_chat_types::MessageFilterCondition::default();
    cond.chat_id = Some(Id::from(TEST_CHAT_ID));
    Filter::Condition(cond)
}

/// `Message/changes` must thread `since_state` to `sinceState` and emit
/// `maxChanges` when provided (RFC 8620 §5.2).
#[tokio::test]
async fn message_changes_since_state_passthrough() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": MESSAGE_STATE_OLD,
            "newState": MESSAGE_STATE_NEW,
            "hasMoreChanges": false,
            "created": [],
            "updated": [TEST_MESSAGE_ID],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from(MESSAGE_STATE_OLD);
    let _ = sc
        .message_changes(&since, Some(100))
        .await
        .expect("message_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["sinceState"],
        json!(MESSAGE_STATE_OLD),
        "sinceState mismatch"
    );
    assert_eq!(args["maxChanges"], json!(100), "maxChanges mismatch");
}

/// `Message/set` create with a happy-path response must serialize the
/// create object with `chatId`, `body`, `bodyType`, `sentAt` and the
/// caller-supplied `client_id` key (message.rs:172-185).
#[tokio::test]
async fn message_create_serializes_create_object() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        json!({ "created": { "client-msg-1": { "id": "server-msg-1" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from(TEST_CHAT_ID);
    // 20-character UTCDate (RFC 8620 §1.4): "YYYY-MM-DDTHH:MM:SSZ".
    let sent_at = UTCDate::from(TEST_UTC_DATE);
    let input = jmap_chat_client::MessageCreateInput {
        client_id: Some("client-msg-1"),
        chat_id: &chat_id,
        body: "hello world",
        body_type: jmap_chat_client::BodyType::Plain,
        sent_at: &sent_at,
        reply_to: None,
        attachments: None,
        mentions: None,
        broadcast_mentions: None,
        actions: None,
        thread_root_id: None,
        topic_id: None,
        subject: None,
        sender_expires_at: None,
        burn_on_read: None,
        expires_after_read_seconds: None,
    };
    let resp = sc
        .message_create(&input, None)
        .await
        .expect("message_create: must succeed");
    let created = resp.created;
    assert!(created.contains_key("client-msg-1"));

    let args = recorded_args(&server).await;
    let create = &args["create"]["client-msg-1"];
    assert_eq!(create["chatId"], json!(TEST_CHAT_ID), "chatId mismatch");
    assert_eq!(create["body"], json!("hello world"), "body mismatch");
    assert_eq!(
        create["bodyType"],
        json!("text/plain"),
        "bodyType must serialize as text/plain"
    );
    assert_eq!(
        create["sentAt"],
        json!(TEST_UTC_DATE),
        "sentAt must thread through verbatim"
    );
    // `replyTo` was not set, must be absent.
    assert!(
        create.get("replyTo").is_none(),
        "replyTo must be absent when None"
    );
}

/// `Message/set` create that returns a `rateLimit` SetError on the
/// caller's creation key must surface as
/// `ClientError::RateLimited { retry_after }`. draft-atwood-jmap-chat-00
/// (jmap-chat-spec 6bcd9d8) L617: slow mode rejects "with a `rateLimit`
/// SetError ({{RFC8620, Section 5.3}}) on the offending `Message/set`
/// create".
#[tokio::test]
async fn message_create_rate_limited_surfaces_as_error() {
    let server = MockServer::start().await;
    // Rate-limit response keeps old==new state (no state advance because
    // no message was actually created).
    let resp_body = set_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_OLD,
        json!({
            "notCreated": {
                "client-msg-1": {
                    "type": "rateLimit",
                    "description": "slow down",
                    "serverRetryAfter": "2024-06-15T09:00:07Z"
                }
            }
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from(TEST_CHAT_ID);
    let sent_at = UTCDate::from(TEST_UTC_DATE);
    let input = jmap_chat_client::MessageCreateInput {
        client_id: Some("client-msg-1"),
        chat_id: &chat_id,
        body: "hello",
        body_type: jmap_chat_client::BodyType::Plain,
        sent_at: &sent_at,
        reply_to: None,
        attachments: None,
        mentions: None,
        broadcast_mentions: None,
        actions: None,
        thread_root_id: None,
        topic_id: None,
        subject: None,
        sender_expires_at: None,
        burn_on_read: None,
        expires_after_read_seconds: None,
    };
    let err = sc
        .message_create(&input, None)
        .await
        .expect_err("message_create must surface rateLimit as error");
    match err {
        jmap_base_client::ClientError::RateLimited { retry_after } => {
            // `serverRetryAfter` is a *workspace convention* paired with
            // the `rateLimit` SetError type — it is NOT declared by
            // draft-atwood-jmap-chat-00 (the spec defines `slowModeSeconds`
            // on Chat objects but does not name `serverRetryAfter`) and it
            // is NOT declared by RFC 8620.
            //
            // The convention's canonical source is the chat-server
            // emission site at crate-jmap-chat-server/src/message.rs:457
            // (`SetError::with_extra("serverRetryAfter", ...)`), which
            // writes the field as a verbatim UTCDate string. The wire
            // payload above was hand-crafted against that canonical site,
            // not generated by the production parser, so the literal
            // below is the independent oracle.
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

/// A `rateLimit` SetError carrying only the RFC 8620 §5.3 fields (no
/// `serverRetryAfter`) is still slow mode: draft-atwood-jmap-chat-00
/// (jmap-chat-spec 6bcd9d8) L617, "Clients MAY simply attempt the send and
/// react to `rateLimit`". It must surface as `ClientError::RateLimited`
/// with `retry_after` set to the receive time, the same fallback the base
/// client uses for an HTTP 429 without `Retry-After`. The oracle is the
/// wall clock read before and after the call.
#[tokio::test]
async fn message_create_rate_limit_without_hint_retries_now() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_OLD,
        json!({
            "notCreated": {
                "client-msg-1": { "type": "rateLimit" }
            }
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from(TEST_CHAT_ID);
    let sent_at = UTCDate::from(TEST_UTC_DATE);
    let input = jmap_chat_client::MessageCreateInput {
        client_id: Some("client-msg-1"),
        chat_id: &chat_id,
        body: "hello",
        body_type: jmap_chat_client::BodyType::Plain,
        sent_at: &sent_at,
        reply_to: None,
        attachments: None,
        mentions: None,
        broadcast_mentions: None,
        actions: None,
        thread_root_id: None,
        topic_id: None,
        subject: None,
        sender_expires_at: None,
        burn_on_read: None,
        expires_after_read_seconds: None,
    };
    let now = || chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let before = now();
    let err = sc
        .message_create(&input, None)
        .await
        .expect_err("message_create must surface rateLimit without a hint as error");
    let after = now();
    match err {
        jmap_base_client::ClientError::RateLimited { retry_after } => {
            // Fixed-width `YYYY-MM-DDTHH:MM:SSZ` strings order lexically.
            let at = retry_after.as_ref();
            assert!(
                before.as_str() <= at && at <= after.as_str(),
                "retry_after {at} must lie between {before} and {after}"
            );
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

/// The legacy custom `rateLimited` SetError is not an alias for the RFC
/// 8620 §5.3 `rateLimit` type that draft-atwood-jmap-chat-00 33702a5 L617
/// names for slow mode (CHANGELOG clean break). Like any other per-create
/// rejection it must come back as `Ok` with the error in `notCreated`.
#[tokio::test]
async fn message_create_legacy_rate_limited_is_not_created_not_error() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_OLD,
        json!({
            "notCreated": {
                "client-msg-1": {
                    "type": "rateLimited",
                    "serverRetryAfter": "2024-06-15T09:00:07Z"
                }
            }
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from(TEST_CHAT_ID);
    let sent_at = UTCDate::from(TEST_UTC_DATE);
    let input = jmap_chat_client::MessageCreateInput {
        client_id: Some("client-msg-1"),
        chat_id: &chat_id,
        body: "hello",
        body_type: jmap_chat_client::BodyType::Plain,
        sent_at: &sent_at,
        reply_to: None,
        attachments: None,
        mentions: None,
        broadcast_mentions: None,
        actions: None,
        thread_root_id: None,
        topic_id: None,
        subject: None,
        sender_expires_at: None,
        burn_on_read: None,
        expires_after_read_seconds: None,
    };
    let set_resp = sc
        .message_create(&input, None)
        .await
        .expect("legacy rateLimited must not map to ClientError::RateLimited");
    let err = set_resp
        .not_created
        .get("client-msg-1")
        .expect("creation key must be in notCreated");
    assert_eq!(err.error_type.to_string(), "rateLimited");
}

/// A `rateLimit` SetError whose `serverRetryAfter` is not a UTCDate (RFC
/// 8620 §1.4: an RFC 3339 `date-time` in UTC) cannot yield a retry
/// deadline, so `message_create` reports `UnexpectedResponse` rather than
/// guessing one.
#[tokio::test]
async fn message_create_rate_limit_malformed_retry_after_is_unexpected() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_OLD,
        json!({
            "notCreated": {
                "client-msg-1": {
                    "type": "rateLimit",
                    "serverRetryAfter": "in five seconds"
                }
            }
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from(TEST_CHAT_ID);
    let sent_at = UTCDate::from(TEST_UTC_DATE);
    let input = jmap_chat_client::MessageCreateInput {
        client_id: Some("client-msg-1"),
        chat_id: &chat_id,
        body: "hello",
        body_type: jmap_chat_client::BodyType::Plain,
        sent_at: &sent_at,
        reply_to: None,
        attachments: None,
        mentions: None,
        broadcast_mentions: None,
        actions: None,
        thread_root_id: None,
        topic_id: None,
        subject: None,
        sender_expires_at: None,
        burn_on_read: None,
        expires_after_read_seconds: None,
    };
    let err = sc
        .message_create(&input, None)
        .await
        .expect_err("malformed serverRetryAfter must be an error");
    assert!(
        matches!(err, jmap_base_client::ClientError::UnexpectedResponse(_)),
        "expected UnexpectedResponse, got {err:?}"
    );
}

/// `Message/set` update with `body` patch must emit `body` inside the
/// per-id patch object (message.rs:222-224, RFC 8620 §5.3).
#[tokio::test]
async fn message_update_body_patch_serializes() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let patch = jmap_chat_client::MessagePatch {
        body: Some("edited body"),
        body_type: Some(jmap_chat_client::BodyType::Markdown),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
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
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let reaction_sent_at = UTCDate::from(TEST_UTC_DATE);
    let changes = [jmap_chat_client::ReactionChange::Add {
        sender_reaction_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        emoji: "👍",
        sent_at: &reaction_sent_at,
    }];
    let patch = jmap_chat_client::MessagePatch {
        reaction_changes: Some(&changes),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
    assert_eq!(
        patch_obj["reactions/01ARZ3NDEKTSV4RRFFQ69G5FAV"],
        json!({ "emoji": "👍", "sentAt": TEST_UTC_DATE }),
        "reaction-add must produce JSON Pointer patch key with emoji + sentAt"
    );
}

/// `Message/set` update with a reaction-remove must emit a
/// `reactions/<senderReactionId>` key with value `null`
/// (message.rs:277-280).
#[tokio::test]
async fn message_update_reaction_remove_emits_null_patch_value() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let changes = [jmap_chat_client::ReactionChange::Remove {
        sender_reaction_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
    }];
    let patch = jmap_chat_client::MessagePatch {
        reaction_changes: Some(&changes),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
    assert_eq!(
        patch_obj["reactions/01ARZ3NDEKTSV4RRFFQ69G5FAV"],
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

    let msg_id = Id::from(TEST_MESSAGE_ID);
    let sent_at = UTCDate::from(TEST_UTC_DATE);
    let changes = [jmap_chat_client::ReactionChange::Add {
        sender_reaction_id: "bad/id",
        emoji: "👍",
        sent_at: &sent_at,
    }];
    let patch = jmap_chat_client::MessagePatch {
        reaction_changes: Some(&changes),
        ..Default::default()
    };
    let err = sc
        .message_update(&msg_id, &patch, None)
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
/// `destroy` key (message.rs:301-318, RFC 8620 §5.3). Empty-slice
/// rejection lives in the paired
/// [`message_destroy_rejects_empty_ids`] test below.
#[tokio::test]
async fn message_destroy_threads_ids() {
    let server = MockServer::start().await;
    let resp_body = helpers::set_destroy_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from(TEST_MESSAGE_ID)];
    let _ = sc
        .message_destroy(&ids, None)
        .await
        .expect("message_destroy: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["destroy"],
        json!([TEST_MESSAGE_ID]),
        "destroy must thread"
    );
}

/// `Message/set` destroy must reject an empty `ids` slice client-side
/// before any HTTP call, surfacing `ClientError::InvalidArgument`.
/// Paired with [`message_destroy_threads_ids`] above.
#[tokio::test]
async fn message_destroy_rejects_empty_ids() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);
    let empty: [Id; 0] = [];
    let err = sc
        .message_destroy(&empty, None)
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
//
// The stock `Message/set` update response body for these tests is built
// via `helpers::set_update_response` (bd:JMAP-26di.11 — the previous
// in-file `message_set_update_resp()` helper has been collapsed into the
// shared helper).
// ---------------------------------------------------------------------------

/// `MessagePatch.read_disposition = Some(Displayed)` together with `read_at`
/// must emit both `readAt` and `readDisposition: "displayed"` inside the
/// per-id patch object (spec §1012 + §ReadDisposition line 309).
#[tokio::test]
async fn message_update_with_read_disposition_displayed_serialises_correctly() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let read_at = UTCDate::from(TEST_UTC_DATE);
    let patch = jmap_chat_client::MessagePatch {
        read_at: Some(&read_at),
        read_disposition: Some(jmap_chat_types::ReadDisposition::Displayed),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
    assert_eq!(
        patch_obj["readAt"],
        json!(TEST_UTC_DATE),
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
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let read_at = UTCDate::from(TEST_UTC_DATE);
    let patch = jmap_chat_client::MessagePatch {
        read_at: Some(&read_at),
        read_disposition: Some(jmap_chat_types::ReadDisposition::Deleted),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
    assert_eq!(patch_obj["readAt"], json!(TEST_UTC_DATE));
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
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let read_at = UTCDate::from(TEST_UTC_DATE);
    let patch = jmap_chat_client::MessagePatch {
        read_at: Some(&read_at),
        read_disposition: Some(jmap_chat_types::ReadDisposition::Processed),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
    assert_eq!(patch_obj["readAt"], json!(TEST_UTC_DATE));
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
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let read_at = UTCDate::from(TEST_UTC_DATE);
    let patch = jmap_chat_client::MessagePatch {
        read_at: Some(&read_at),
        read_disposition: Some(jmap_chat_types::ReadDisposition::Other(
            "voice-listened".into(),
        )),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
    assert_eq!(patch_obj["readAt"], json!(TEST_UTC_DATE));
    assert_eq!(
        patch_obj["readDisposition"],
        json!("voice-listened"),
        "Other(\"voice-listened\") must round-trip to the wire as-is"
    );
}

/// The catch-all must cover arbitrary values, not just the spec's examples.
/// Oracle: draft-atwood-jmap-chat-00 §ReadDisposition (6c38e2f line 500):
/// "Unrecognized values MUST be stored as-is". `"acmeCorpPrivateStatus"`
/// appears nowhere in the spec, so a regression that special-cased only
/// `"voice-listened"` / `"forwarded"` fails here.
#[tokio::test]
async fn message_update_with_read_disposition_arbitrary_other_passes_through() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let read_at = UTCDate::from(TEST_UTC_DATE);
    let patch = jmap_chat_client::MessagePatch {
        read_at: Some(&read_at),
        read_disposition: Some(jmap_chat_types::ReadDisposition::Other(
            "acmeCorpPrivateStatus".into(),
        )),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"][TEST_MESSAGE_ID]["readDisposition"],
        json!("acmeCorpPrivateStatus"),
        "an arbitrary Other value must reach the wire unchanged"
    );
}

/// `MessagePatch.read_at = Some(_)` alone (no `read_disposition`) must emit
/// `readAt` but NOT a `readDisposition` key, so the server's
/// "default to displayed" path (spec §Message line 540) is reachable from
/// the client side by omitting the field.
#[tokio::test]
async fn message_update_without_read_disposition_omits_field() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let read_at = UTCDate::from(TEST_UTC_DATE);
    let patch = jmap_chat_client::MessagePatch {
        read_at: Some(&read_at),
        ..Default::default()
    };
    // patch.read_disposition deliberately left as None.
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
    assert_eq!(
        patch_obj["readAt"],
        json!(TEST_UTC_DATE),
        "readAt must thread to the wire"
    );
    assert!(
        patch_obj.get("readDisposition").is_none(),
        "readDisposition key must be absent when read_disposition is None; got {:?}",
        patch_obj.get("readDisposition")
    );
}

/// draft-atwood-jmap-chat-00 §Message `readDisposition`: "A client supplies
/// it only in the same `Message/set` update that sets `readAt`". A lone
/// `read_disposition` must short-circuit client-side.
#[tokio::test]
async fn message_update_read_disposition_without_read_at_rejected() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);

    let patch = jmap_chat_client::MessagePatch {
        read_disposition: Some(jmap_chat_types::ReadDisposition::Deleted),
        ..Default::default()
    };
    let err = sc
        .message_update(&Id::from(TEST_MESSAGE_ID), &patch, None)
        .await
        .expect_err("message_update must reject read_disposition without read_at");
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidArgument(_)),
        "expected InvalidArgument, got {err:?}"
    );

    let reqs = server
        .received_requests()
        .await
        .expect("recorded_requests must succeed");
    assert!(
        reqs.is_empty(),
        "no HTTP request must be sent for a lone read_disposition"
    );
}

/// `Message/queryChanges` must thread `since_query_state` to
/// `sinceQueryState` and reject the empty token client-side
/// (message.rs:341-362, RFC 8620 §5.6).
#[tokio::test]
async fn message_query_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/queryChanges",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldQueryState": QUERY_STATE_OLD,
            "newQueryState": QUERY_STATE_NEW,
            "total": null,
            "removed": [],
            "added": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from(QUERY_STATE_OLD);
    let _ = sc
        .message_query_changes(
            &since,
            Some(25),
            None::<&jmap_chat_types::MessageFilter>,
            None,
            None,
            None,
        )
        .await
        .expect("message_query_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["sinceQueryState"],
        json!(QUERY_STATE_OLD),
        "sinceQueryState mismatch"
    );
    assert_eq!(args["maxChanges"], json!(25), "maxChanges mismatch");

    let empty = State::from("");
    let err = sc
        .message_query_changes(
            &empty,
            None,
            None::<&jmap_chat_types::MessageFilter>,
            None,
            None,
            None,
        )
        .await
        .expect_err("must reject empty since_query_state");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("since_query_state may not be empty"),
                "got: {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `Message/set` create with `BodyType::Rich` must serialise the
/// canonical wire string `"application/jmap-chat-rich"` per
/// `BodyType::as_str` (crate-jmap-chat-client::types::BodyType lines
/// 192-200). The existing message_create_serializes_create_object test
/// covers `BodyType::Plain` ("text/plain"); the
/// message_update_body_patch_serializes test covers
/// `BodyType::Markdown` ("text/markdown"). This test fills the missing
/// third variant.
#[tokio::test]
async fn message_create_with_body_type_rich_serialises_correct_wire_string() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        json!({ "created": { "client-msg-rich": { "id": "server-msg-rich" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from(TEST_CHAT_ID);
    let sent_at = UTCDate::from(TEST_UTC_DATE);
    let input = jmap_chat_client::MessageCreateInput {
        client_id: Some("client-msg-rich"),
        chat_id: &chat_id,
        body: "[{\"type\":\"text\",\"value\":\"hi\"}]",
        body_type: jmap_chat_client::BodyType::Rich,
        sent_at: &sent_at,
        reply_to: None,
        attachments: None,
        mentions: None,
        broadcast_mentions: None,
        actions: None,
        thread_root_id: None,
        topic_id: None,
        subject: None,
        sender_expires_at: None,
        burn_on_read: None,
        expires_after_read_seconds: None,
    };
    let _ = sc
        .message_create(&input, None)
        .await
        .expect("message_create: must succeed");

    let args = recorded_args(&server).await;
    let create = &args["create"]["client-msg-rich"];
    assert_eq!(
        create["bodyType"],
        json!("application/jmap-chat-rich"),
        "BodyType::Rich must serialise as 'application/jmap-chat-rich'"
    );
}

/// `Message/set` update with `body_type: BodyType::Other(s)` must
/// serialise the literal wire string supplied by the caller
/// (workspace `Other(String)` round-trip policy + BodyType::as_str
/// lines 192-200). The vendor-defined wire string here cannot collide
/// with any spec-defined value.
#[tokio::test]
async fn message_update_with_body_type_other_round_trips_wire_string() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let patch = jmap_chat_client::MessagePatch {
        body: Some("vendor-encoded body"),
        body_type: Some(jmap_chat_client::BodyType::Other(
            "application/x-acme-rich".into(),
        )),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"][TEST_MESSAGE_ID]["bodyType"],
        json!("application/x-acme-rich"),
        "BodyType::Other must serialise the caller-supplied wire string verbatim"
    );
}

/// `Message/queryChanges` with filter, sort, upToId, and calculateTotal
/// must emit all four optional args on the wire (RFC 8620 §5.6).
///
/// Oracle: draft-atwood-jmap-chat-00 — `chatId` is a valid Message filter
/// field; the comparator shape (property + isAscending) follows
/// RFC 8620 §5.5.
#[tokio::test]
async fn message_query_changes_with_filter_sort_upto_calculatetotal() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/queryChanges",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldQueryState": QUERY_STATE_OLD,
            "newQueryState": QUERY_STATE_NEW,
            "total": 0,
            "removed": [],
            "added": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from(QUERY_STATE_OLD);
    let up_to = Id::from("MSG-100");
    let mut cond = jmap_chat_types::MessageFilterCondition::default();
    cond.chat_id = Some(Id::from(TEST_CHAT_ID));
    // draft-atwood-jmap-chat-00 §Input Validation: "`sentAt`, `editedAt`:
    // store as-is; never use for ordering or expiry."; §Message/query
    // "Default sort: `receivedAt`".
    let mut cmp = jmap_chat_types::MessageComparator::new(
        jmap_chat_types::MessageComparatorProperty::ReceivedAt,
    );
    cmp.is_ascending = false;
    sc.message_query_changes(
        &since,
        None,
        Some(&jmap_chat_types::Filter::Condition(cond)),
        Some(&[cmp]),
        Some(&up_to),
        Some(true),
    )
    .await
    .expect("message_query_changes_with_filter_sort_upto_calculatetotal: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"]["chatId"],
        json!(TEST_CHAT_ID),
        "filter.chatId must be 'chat-1'"
    );
    assert_eq!(
        args["sort"][0]["property"],
        json!("receivedAt"),
        "sort[0].property must be 'receivedAt'"
    );
    assert_eq!(
        args["upToId"],
        json!("MSG-100"),
        "upToId must be on the wire (RFC 8620 §5.6)"
    );
    assert_eq!(
        args["calculateTotal"],
        json!(true),
        "calculateTotal must be on the wire (RFC 8620 §5.6)"
    );
}

// ---------------------------------------------------------------------------
// broadcastMentions plumbing — draft-atwood-jmap-chat-00 §4.4.1
//
// Three tests confirm the wire behavior of `MessagePatch.broadcast_mentions`:
//   1. `Some(vec![…])` → key present with the serialized array
//   2. `None` → key absent
//   3. `Some(vec![])` → key present as `[]` (clears broadcast mentions)
// ---------------------------------------------------------------------------

/// `MessagePatch.broadcast_mentions = Some(vec![…])` must emit the
/// `broadcastMentions` key inside the per-id patch object with the
/// serialized array (draft-atwood-jmap-chat-00 §4.4.1).
#[tokio::test]
async fn message_update_broadcast_mentions_emits_wire_key() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let patch = jmap_chat_client::MessagePatch {
        broadcast_mentions: Some(vec![
            serde_json::from_value(json!({"scope": "everyone"}))
                .expect("BroadcastMention must deserialize"),
        ]),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
    assert_eq!(
        patch_obj["broadcastMentions"],
        json!([{"scope": "everyone"}]),
        "broadcastMentions must appear on the wire with the serialized array"
    );
}

/// `MessagePatch.broadcast_mentions = None` must NOT emit a
/// `broadcastMentions` key in the per-id patch object — the server
/// leaves the existing value unchanged.
#[tokio::test]
async fn message_update_broadcast_mentions_none_omits_wire_key() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let patch = jmap_chat_client::MessagePatch::default();
    // broadcast_mentions is None by default.
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
    assert!(
        patch_obj.get("broadcastMentions").is_none(),
        "broadcastMentions key must be absent when broadcast_mentions is None; got {:?}",
        patch_obj.get("broadcastMentions")
    );
}

/// `MessagePatch.broadcast_mentions = Some(vec![])` must emit
/// `"broadcastMentions": []` on the wire — this is how a client clears
/// broadcast mentions on an author-only edit.
#[tokio::test]
async fn message_update_broadcast_mentions_empty_vec_emits_empty_array() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let patch = jmap_chat_client::MessagePatch {
        broadcast_mentions: Some(vec![]),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"][TEST_MESSAGE_ID];
    assert_eq!(
        patch_obj["broadcastMentions"],
        json!([]),
        "broadcastMentions must be present as an empty array when Some(vec![])"
    );
}

/// `Message/queryChanges` with all None optional args must NOT emit any
/// of filter/sort/upToId/calculateTotal/maxChanges on the wire.
///
/// Oracle: RFC 8620 §5.6 — all five are optional; the wire shape with
/// `None` for each must be byte-identical to the minimal
/// `sinceQueryState`-only call.
#[tokio::test]
async fn message_query_changes_all_none_omits_optional_wire_keys() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Message/queryChanges",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldQueryState": QUERY_STATE_OLD,
            "newQueryState": QUERY_STATE_NEW,
            "total": 0,
            "removed": [],
            "added": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from(QUERY_STATE_OLD);
    sc.message_query_changes(
        &since,
        None,
        None::<&jmap_chat_types::MessageFilter>,
        None,
        None,
        None,
    )
    .await
    .expect("message_query_changes_all_none_omits_optional_wire_keys: must succeed");

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

// ---------------------------------------------------------------------------
// Message/set create optional properties — draft-atwood-jmap-chat-00
// §Message/set "Creating a Message":
//
//   "Optional: `attachments` (Attachment[]), `mentions` (Mention[]),
//   `broadcastMentions` (BroadcastMention[]), `actions` (MessageAction[]),
//   `replyTo` (String), `threadRootId` (String), `topicId` (String),
//   `subject` (String), `senderExpiresAt` (UTCDate), `burnOnRead`
//   (Boolean), `expiresAfterReadSeconds` (UnsignedInt)."
//
// Element shapes are hand-encoded from the spec's Attachment (`blobId`,
// `filename`, `contentType`, `size`, `sha256`), Mention (`id`, `offset`,
// `length`), BroadcastMention (`scope`, `offset`, `length`) and
// MessageAction (`type`, `uri`, `label`, `expiresAt`, `metadata`)
// property lists.
// ---------------------------------------------------------------------------

/// Every optional create property must appear in the create object under
/// its spec wire name with its spec type.
#[tokio::test]
async fn message_create_serializes_all_optional_properties() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        json!({ "created": { "client-msg-1": { "id": "server-msg-1" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let attachment_json = json!({
        "blobId": "blob-1",
        "filename": "report.pdf",
        "contentType": "application/pdf",
        "size": 1024,
        "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    });
    let mention_json = json!({"id": "c1"});
    let broadcast_json = json!({"scope": "here"});
    let action_json = json!({
        "type": "urn:ietf:params:jmap:chat:cap:vtc",
        "uri": "https://meet.example.com/room-1",
        "label": "Join call",
        "expiresAt": "2024-06-15T10:00:00Z",
        "metadata": {"protocol": "webrtc"}
    });
    let attachments =
        [serde_json::from_value(attachment_json.clone()).expect("Attachment must deserialize")];
    let mentions =
        [serde_json::from_value(mention_json.clone()).expect("Mention must deserialize")];
    let broadcast_mentions =
        [serde_json::from_value(broadcast_json.clone())
            .expect("BroadcastMention must deserialize")];
    let actions =
        [serde_json::from_value(action_json.clone()).expect("MessageAction must deserialize")];

    let sc = helpers::make_client(&server);
    let chat_id = Id::from(TEST_CHAT_ID);
    let sent_at = UTCDate::from(TEST_UTC_DATE);
    let expires = UTCDate::from(TEST_UTC_DATE_LATER);
    let reply_to = Id::from("msg-parent");
    let thread_root = Id::from("msg-root");
    let input = jmap_chat_client::MessageCreateInput {
        client_id: Some("client-msg-1"),
        chat_id: &chat_id,
        body: "@alice @here",
        body_type: jmap_chat_client::BodyType::Plain,
        sent_at: &sent_at,
        reply_to: Some(&reply_to),
        attachments: Some(&attachments),
        mentions: Some(&mentions),
        broadcast_mentions: Some(&broadcast_mentions),
        actions: Some(&actions),
        thread_root_id: Some(&thread_root),
        topic_id: Some("topic-1"),
        subject: Some("Q3 planning"),
        sender_expires_at: Some(&expires),
        burn_on_read: Some(true),
        expires_after_read_seconds: Some(30),
    };
    let _ = sc
        .message_create(&input, None)
        .await
        .expect("message_create: must succeed");

    let args = recorded_args(&server).await;
    let create = &args["create"]["client-msg-1"];
    assert_eq!(create["attachments"], json!([attachment_json]));
    assert_eq!(create["mentions"], json!([mention_json]));
    assert_eq!(create["broadcastMentions"], json!([broadcast_json]));
    assert_eq!(create["actions"], json!([action_json]));
    assert_eq!(create["replyTo"], json!("msg-parent"));
    assert_eq!(create["threadRootId"], json!("msg-root"));
    assert_eq!(create["topicId"], json!("topic-1"));
    assert_eq!(create["subject"], json!("Q3 planning"));
    assert_eq!(create["senderExpiresAt"], json!(TEST_UTC_DATE_LATER));
    assert_eq!(create["burnOnRead"], json!(true));
    assert_eq!(create["expiresAfterReadSeconds"], json!(30));
}

/// Optional create properties left as `None` must be absent from the
/// create object, not sent as `null`: the spec lists them as optional
/// and none of them as nullable.
#[tokio::test]
async fn message_create_omits_unset_optional_properties() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        json!({ "created": { "client-msg-1": { "id": "server-msg-1" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from(TEST_CHAT_ID);
    let sent_at = UTCDate::from(TEST_UTC_DATE);
    let input = jmap_chat_client::MessageCreateInput {
        client_id: Some("client-msg-1"),
        chat_id: &chat_id,
        body: "hello",
        body_type: jmap_chat_client::BodyType::Plain,
        sent_at: &sent_at,
        reply_to: None,
        attachments: None,
        mentions: None,
        broadcast_mentions: None,
        actions: None,
        thread_root_id: None,
        topic_id: None,
        subject: None,
        sender_expires_at: None,
        burn_on_read: None,
        expires_after_read_seconds: None,
    };
    let _ = sc
        .message_create(&input, None)
        .await
        .expect("message_create: must succeed");

    let args = recorded_args(&server).await;
    let create = args["create"]["client-msg-1"]
        .as_object()
        .expect("create entry must be an object");
    let mut keys: Vec<&str> = create.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["body", "bodyType", "chatId", "sentAt"]);
}

/// `MessagePatch.mentions = Some(vec![…])` must emit `mentions` in the
/// per-id patch object. Oracle: draft-atwood-jmap-chat-00 §Message/set
/// "Editing a Message": "`update` with changed `body`, `bodyType`,
/// `mentions`, and/or `broadcastMentions`"; Mention shape is `id` alone
/// per §Mention: "A Mention carries no position in `body`."
#[tokio::test]
async fn message_update_mentions_emits_wire_key() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let patch = jmap_chat_client::MessagePatch {
        mentions: Some(vec![
            serde_json::from_value(json!({"id": "c2"})).expect("Mention must deserialize"),
        ]),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"][TEST_MESSAGE_ID]["mentions"],
        json!([{"id": "c2"}])
    );
}

/// `Message/set` update threads `deletedAt` and `deletedForAll`
/// (bd:JMAP-w8fu.14). Oracle: draft-atwood-jmap-chat-00 at spec 29e9a41
/// line 774 (`deletedAt` (UTCDate, optional)) and line 777
/// (`deletedForAll` (Boolean, optional)).
#[tokio::test]
async fn message_update_deleted_at_and_for_all_serialise() {
    let server = MockServer::start().await;
    let resp_body = set_update_response(
        "Message/set",
        MESSAGE_STATE_OLD,
        MESSAGE_STATE_NEW,
        TEST_MESSAGE_ID,
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let msg_id = Id::from(TEST_MESSAGE_ID);
    let at = UTCDate::from(TEST_UTC_DATE_LATER);
    let patch = jmap_chat_client::MessagePatch {
        deleted_at: Some(&at),
        deleted_for_all: Some(true),
        ..Default::default()
    };
    let _ = sc
        .message_update(&msg_id, &patch, None)
        .await
        .expect("message_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"][TEST_MESSAGE_ID],
        json!({ "deletedAt": TEST_UTC_DATE_LATER, "deletedForAll": true })
    );
}
