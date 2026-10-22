//! Wiremock smoke tests for multi-account targeting
//! ([`jmap_chat_client::SessionClient::for_account`]) and typed batching
//! ([`jmap_chat_client::Batch`]).
//!
//! Spec oracles:
//!   - RFC 8620 §2 (`accounts`, `accountCapabilities`)
//!   - RFC 8620 §3.3 (request object), §3.7 (result references)
//!   - draft-atwood-jmap-chat-00 §Message/query (required filter)

#[path = "helpers.rs"]
mod helpers;

use helpers::{mock_jmap_post, recorded_args, recorded_body, TEST_ACCOUNT_ID, TEST_SESSION_STATE};
use jmap_chat_client::JmapChatExt;
use jmap_chat_types::{ChatFilter, MessageFilter, MessageFilterCondition};
use jmap_types::query::Filter;
use jmap_types::Id;
use serde_json::json;
use wiremock::MockServer;

/// Second account with the chat capability (e.g. a shared account).
const SHARED_CHAT_ACCOUNT: &str = "A2";
/// Account without the chat capability.
const MAIL_ONLY_ACCOUNT: &str = "A3";

/// Session with three accounts: the primary chat account
/// [`TEST_ACCOUNT_ID`], a second chat account, and a mail-only account.
///
/// Oracle: RFC 8620 §2 Session example shape.
fn make_multi_account_client(server: &MockServer) -> jmap_chat_client::SessionClient {
    let session: jmap_base_client::Session = serde_json::from_value(json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:chat": {}
        },
        "accounts": {
            TEST_ACCOUNT_ID: {
                "name": "john@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": { "urn:ietf:params:jmap:chat": {} }
            },
            SHARED_CHAT_ACCOUNT: {
                "name": "team@example.com",
                "isPersonal": false,
                "isReadOnly": false,
                "accountCapabilities": { "urn:ietf:params:jmap:chat": {} }
            },
            MAIL_ONLY_ACCOUNT: {
                "name": "archive@example.com",
                "isPersonal": false,
                "isReadOnly": true,
                "accountCapabilities": { "urn:ietf:params:jmap:mail": {} }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:chat": TEST_ACCOUNT_ID },
        "username": "john@example.com",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}", server.uri()),
        "uploadUrl": format!("{}/ul/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}", server.uri()),
        "state": TEST_SESSION_STATE
    }))
    .expect("multi-account session must deserialize from RFC 8620 §2 shape");
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("JmapClient construction must succeed");
    client.with_chat_session(session)
}

/// A handle from `for_account` sends that account's id, not the primary.
///
/// Oracle: RFC 8620 §2 — `accounts` maps "an account id to an Account
/// object for each account the user has access to", and §5.5 /query
/// `accountId` is "The id of the account to use."
#[tokio::test]
async fn for_account_targets_named_chat_account() {
    let server = MockServer::start().await;
    mock_jmap_post(
        &server,
        json!({
            "sessionState": TEST_SESSION_STATE,
            "methodResponses": [["Chat/query", {
                "accountId": SHARED_CHAT_ACCOUNT,
                "queryState": "qs-1",
                "canCalculateChanges": false,
                "position": 0,
                "ids": []
            }, "r1"]]
        }),
    )
    .await;

    let sc = make_multi_account_client(&server);
    let shared = sc
        .for_account(SHARED_CHAT_ACCOUNT)
        .expect("chat-capable account must be accepted");
    shared
        .chat_query(
            None::<&ChatFilter>,
            None,
            &jmap_base_client::QueryParams::default(),
        )
        .await
        .expect("chat_query on shared account must succeed");

    assert_eq!(
        recorded_args(&server).await["accountId"],
        json!(SHARED_CHAT_ACCOUNT),
        "request must target the for_account account"
    );
    assert_eq!(shared.chat_account_id().ok(), Some(SHARED_CHAT_ACCOUNT));
    assert_eq!(
        sc.chat_account_id().ok(),
        Some(TEST_ACCOUNT_ID),
        "the original handle keeps the primary account"
    );
}

/// Accounts absent from the session, or whose `accountCapabilities` lacks
/// the chat URI, are rejected with `InvalidArgument`.
///
/// Oracle: RFC 8620 §2 — `accountCapabilities` is "The set of capability
/// URIs for the methods supported in this account"; the server "MUST NOT
/// include it in the accountCapabilities object if the user cannot use
/// those methods with this account."
#[tokio::test]
async fn for_account_rejects_non_chat_accounts() {
    let server = MockServer::start().await;
    let sc = make_multi_account_client(&server);
    for id in [MAIL_ONLY_ACCOUNT, "unknown"] {
        match sc.for_account(id) {
            Err(jmap_base_client::ClientError::InvalidArgument(_)) => {}
            other => panic!("for_account({id}): expected InvalidArgument, got {other:?}"),
        }
    }
}

/// `chat_query_get` sends `Chat/query` then `Chat/get` whose `ids` is a
/// result reference to the query's `/ids`.
///
/// Oracle: RFC 8620 §3.7 example, `Email/query` (call "t0") followed by
/// `Email/get` with
/// `"#ids": { "resultOf": "t0", "name": "Email/query", "path": "/ids" }`
/// (call "t1"); hand-encoded below with the Chat names. `using` per
/// RFC 8620 §3.3 and draft-atwood-jmap-chat-00 §3.
#[tokio::test]
async fn batch_chat_query_get_sends_rfc8620_result_reference() {
    let server = MockServer::start().await;
    mock_jmap_post(
        &server,
        json!({
            "sessionState": TEST_SESSION_STATE,
            "methodResponses": [
                ["Chat/query", {
                    "accountId": TEST_ACCOUNT_ID,
                    "queryState": "qs-1",
                    "canCalculateChanges": true,
                    "position": 0,
                    "ids": []
                }, "t0"],
                ["Chat/get", {
                    "accountId": TEST_ACCOUNT_ID,
                    "state": "c-1",
                    "list": [],
                    "notFound": []
                }, "t1"]
            ]
        }),
    )
    .await;

    let sc = helpers::make_client(&server);
    let mut batch = sc.batch().expect("batch must start");
    let (query, get) = batch
        .chat_query_get(
            None::<&ChatFilter>,
            None,
            &jmap_base_client::QueryParams {
                limit: Some(10),
                ..Default::default()
            },
            None,
        )
        .expect("chat_query_get must build");
    let resp = sc.send_batch(batch).await.expect("send_batch must succeed");

    assert_eq!(
        recorded_body(&server).await,
        json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"],
            "methodCalls": [
                ["Chat/query", {
                    "accountId": TEST_ACCOUNT_ID,
                    "filter": null,
                    "limit": 10
                }, "t0"],
                ["Chat/get", {
                    "accountId": TEST_ACCOUNT_ID,
                    "#ids": {
                        "resultOf": "t0",
                        "name": "Chat/query",
                        "path": "/ids"
                    }
                }, "t1"]
            ]
        })
    );
    assert_eq!(
        resp.get(&query).expect("query result").query_state.as_ref(),
        "qs-1"
    );
    assert_eq!(resp.get(&get).expect("get result").state.as_ref(), "c-1");
}

/// Two pairs in one batch get consecutive call ids, each /get references
/// its own /query, and each handle reads its own response.
///
/// Oracle: RFC 8620 §3.7 example (four calls "t0".."t3", each `#ids`
/// naming the prior call's id and method); `sort: None` omits `sort` so
/// the draft-atwood-jmap-chat-00 §Message/query default applies
/// (jmap-chat-spec 29e9a41 L1295).
#[tokio::test]
async fn batch_two_pairs_reference_their_own_queries() {
    let server = MockServer::start().await;
    mock_jmap_post(
        &server,
        json!({
            "sessionState": TEST_SESSION_STATE,
            "methodResponses": [
                ["Chat/query", {
                    "accountId": TEST_ACCOUNT_ID,
                    "queryState": "cq-1",
                    "canCalculateChanges": true,
                    "position": 0,
                    "ids": ["chat-1"]
                }, "t0"],
                ["Chat/get", {
                    "accountId": TEST_ACCOUNT_ID,
                    "state": "c-1",
                    "list": [],
                    "notFound": ["chat-1"]
                }, "t1"],
                ["Message/query", {
                    "accountId": TEST_ACCOUNT_ID,
                    "queryState": "mq-1",
                    "canCalculateChanges": true,
                    "position": 0,
                    "ids": []
                }, "t2"],
                ["Message/get", {
                    "accountId": TEST_ACCOUNT_ID,
                    "state": "ms-1",
                    "list": [],
                    "notFound": []
                }, "t3"]
            ]
        }),
    )
    .await;

    let sc = helpers::make_client(&server);
    let mut cond = MessageFilterCondition::default();
    cond.chat_id = Some(Id::from("chat-1"));
    let msg_filter = Filter::Condition(cond);
    let mut batch = sc.batch().expect("batch must start");
    let (chat_q, chat_g) = batch
        .chat_query_get(
            None::<&ChatFilter>,
            None,
            &jmap_base_client::QueryParams::default(),
            Some(&["id", "name"]),
        )
        .expect("chat pair must build");
    let (msg_q, msg_g) = batch
        .message_query_get(
            Some(&msg_filter),
            None,
            &jmap_base_client::QueryParams {
                limit: Some(50),
                ..Default::default()
            },
            Some(&["id", "body"]),
        )
        .expect("message pair must build");
    let resp = sc.send_batch(batch).await.expect("send_batch must succeed");

    assert_eq!(
        recorded_body(&server).await["methodCalls"],
        json!([
            ["Chat/query", {
                "accountId": TEST_ACCOUNT_ID,
                "filter": null
            }, "t0"],
            ["Chat/get", {
                "accountId": TEST_ACCOUNT_ID,
                "#ids": { "resultOf": "t0", "name": "Chat/query", "path": "/ids" },
                "properties": ["id", "name"]
            }, "t1"],
            ["Message/query", {
                "accountId": TEST_ACCOUNT_ID,
                "filter": { "chatId": "chat-1" },
                "limit": 50
            }, "t2"],
            ["Message/get", {
                "accountId": TEST_ACCOUNT_ID,
                "#ids": { "resultOf": "t2", "name": "Message/query", "path": "/ids" },
                "properties": ["id", "body"]
            }, "t3"]
        ])
    );
    assert_eq!(resp.get(&chat_q).expect("t0").query_state.as_ref(), "cq-1");
    assert_eq!(
        resp.get(&chat_g).expect("t1").not_found,
        vec![Id::from("chat-1")]
    );
    assert_eq!(resp.get(&msg_q).expect("t2").query_state.as_ref(), "mq-1");
    assert_eq!(resp.get(&msg_g).expect("t3").state.as_ref(), "ms-1");
}

/// A batch from a `for_account` handle targets that account in every call.
///
/// Oracle: RFC 8620 §3.7 example shape; `accountId` per RFC 8620 §5.1.
#[tokio::test]
async fn batch_uses_for_account_target() {
    let server = MockServer::start().await;
    mock_jmap_post(
        &server,
        json!({ "sessionState": TEST_SESSION_STATE, "methodResponses": [] }),
    )
    .await;

    let sc = make_multi_account_client(&server)
        .for_account(SHARED_CHAT_ACCOUNT)
        .expect("chat-capable account must be accepted");
    let mut batch = sc.batch().expect("batch must start");
    batch
        .chat_query_get(
            None::<&ChatFilter>,
            None,
            &jmap_base_client::QueryParams::default(),
            None,
        )
        .expect("chat pair must build");
    sc.send_batch(batch).await.expect("send_batch must succeed");

    let body = recorded_body(&server).await;
    assert_eq!(
        body["methodCalls"][0][1]["accountId"],
        json!(SHARED_CHAT_ACCOUNT)
    );
    assert_eq!(
        body["methodCalls"][1][1]["accountId"],
        json!(SHARED_CHAT_ACCOUNT)
    );
}

/// A failed result reference surfaces as a method error on that call's
/// handle only.
///
/// Oracle: RFC 8620 §3.7 — "If any result reference fails to resolve, the
/// whole method MUST be rejected with an 'invalidResultReference' error";
/// error invocation shape `["error", {"type": ...}, callId]` per §3.6.2.
#[tokio::test]
async fn batch_invalid_result_reference_is_per_call() {
    let server = MockServer::start().await;
    mock_jmap_post(
        &server,
        json!({
            "sessionState": TEST_SESSION_STATE,
            "methodResponses": [
                ["error", { "type": "anchorNotFound" }, "t0"],
                ["error", { "type": "invalidResultReference" }, "t1"]
            ]
        }),
    )
    .await;

    let sc = helpers::make_client(&server);
    let mut batch = sc.batch().expect("batch must start");
    let (query, get) = batch
        .chat_query_get(
            None::<&ChatFilter>,
            None,
            &jmap_base_client::QueryParams::default(),
            None,
        )
        .expect("chat pair must build");
    let resp = sc.send_batch(batch).await.expect("send_batch must succeed");

    match resp.get(&get) {
        Err(jmap_base_client::ClientError::MethodError(e)) => {
            assert_eq!(e.error_type.to_string(), "invalidResultReference")
        }
        other => panic!("expected invalidResultReference, got {other:?}"),
    }
    match resp.get(&query) {
        Err(jmap_base_client::ClientError::MethodError(e)) => {
            assert_eq!(e.error_type.to_string(), "anchorNotFound")
        }
        other => panic!("expected anchorNotFound, got {other:?}"),
    }
}

/// `message_query_get` rejects a query with neither `chatId` nor
/// `hasMention: true`, before anything is sent.
///
/// Oracle: draft-atwood-jmap-chat-00 §Message/query — "All requests MUST
/// include a `chatId` filter, unless the request includes a
/// `hasMention: true` filter condition."
#[tokio::test]
async fn batch_message_query_get_rejects_unscoped_query() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);
    let mut batch = sc.batch().expect("batch must start");
    match batch.message_query_get(
        None::<&MessageFilter>,
        None,
        &jmap_base_client::QueryParams::default(),
        None,
    ) {
        Err(jmap_base_client::ClientError::InvalidArgument(_)) => {}
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
