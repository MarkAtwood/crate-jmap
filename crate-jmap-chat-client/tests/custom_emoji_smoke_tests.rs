//! Wiremock smoke tests for `CustomEmoji/*` method paths in
//! jmap-chat-client.
//!
//! Spec oracles:
//!   - RFC 8620 §5.1 /get, §5.2 /changes, §5.3 /set, §5.5 /query,
//!     §5.6 /queryChanges
//!   - draft-atwood-jmap-chat-00 §4.16 (CustomEmoji/*) and §4.17
//!     (CustomEmoji object field set)

#[path = "helpers.rs"]
mod helpers;

use helpers::{
    jmap_response, mock_jmap_post, recorded_args, recorded_body, set_response, TEST_ACCOUNT_ID,
};
use jmap_types::{Id, State};
use serde_json::json;
use wiremock::MockServer;

/// `CustomEmoji/get` with `ids: None, properties: None` must omit both
/// keys on the wire (custom_emoji.rs:26-34). Pins the USING_CHAT
/// capability set for the entire CustomEmoji/* family (one assertion
/// per method-family per workspace convention).
#[tokio::test]
async fn custom_emoji_get_omits_ids_and_properties_when_none() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "CustomEmoji/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "ce-state-1",
            "list": [],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let _ = sc
        .custom_emoji_get(None, None)
        .await
        .expect("custom_emoji_get: must succeed");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["accountId"],
        json!(TEST_ACCOUNT_ID),
        "accountId mismatch"
    );
    assert!(args.get("ids").is_none(), "ids must be omitted when None");
    assert!(
        args.get("properties").is_none(),
        "properties must be omitted when None"
    );
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]),
        "CustomEmoji/* using must equal USING_CHAT exactly"
    );
}

/// `CustomEmoji/get` decode coverage: populated wire object must
/// round-trip through the [`jmap_chat_types::CustomEmoji`]
/// `Deserialize` impl with all required fields plus the `space_id`
/// optional (the only optional on the object — Some indicates a
/// Space-scoped emoji, None indicates server-global per spec §4.17).
#[tokio::test]
async fn custom_emoji_get_decodes_populated_emoji() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "CustomEmoji/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "ce-state-2",
            "list": [
                {
                    "id": "ce-1",
                    "name": "catjam",
                    "blobId": "blob-emoji-1",
                    "createdBy": "u1",
                    "createdAt": "2026-01-01T00:00:00Z",
                    "spaceId": "space-eng"
                },
                {
                    "id": "ce-2",
                    "name": "thumbsup",
                    "blobId": "blob-emoji-2",
                    "createdBy": "u1",
                    "createdAt": "2026-01-02T00:00:00Z"
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .custom_emoji_get(None, None)
        .await
        .expect("custom_emoji_get: must succeed");

    assert_eq!(resp.list.len(), 2, "list must contain two emoji");
    let space_scoped = &resp.list[0];
    assert_eq!(space_scoped.name, "catjam", "name mismatch");
    assert_eq!(
        space_scoped.space_id.as_ref().map(|id| id.as_ref()),
        Some("space-eng"),
        "Space-scoped emoji must carry space_id"
    );

    let server_global = &resp.list[1];
    assert_eq!(server_global.name, "thumbsup", "name mismatch");
    assert!(
        server_global.space_id.is_none(),
        "server-global emoji must have space_id == None"
    );
}

/// `CustomEmoji/changes` must thread `since_state` and `max_changes`
/// and reject empty `since_state` (custom_emoji.rs:48-52, RFC 8620
/// §5.2).
#[tokio::test]
async fn custom_emoji_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "CustomEmoji/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": "ce-old",
            "newState": "ce-new",
            "hasMoreChanges": false,
            "created": ["ce-new-1"],
            "updated": [],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("ce-old");
    let _ = sc
        .custom_emoji_changes(&since, Some(15))
        .await
        .expect("custom_emoji_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["sinceState"], json!("ce-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(15), "maxChanges mismatch");

    let empty = State::from("");
    let err = sc
        .custom_emoji_changes(&empty, None)
        .await
        .expect_err("must reject empty since_state");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("since_state may not be empty"),
                "error message must explain validation: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `CustomEmoji/set` create must serialise `name` + `blobId` and
/// optional `spaceId` inside the `create` map keyed by the
/// caller-supplied client id (custom_emoji.rs:79-91). When `space_id`
/// is `None` the wire create object must omit the key (server-global
/// emoji per spec §4.17).
#[tokio::test]
async fn custom_emoji_create_serialises_with_and_without_space_id() {
    // Space-scoped variant.
    let server = MockServer::start().await;
    let resp_body = set_response(
        "CustomEmoji/set",
        "ce-1",
        "ce-2",
        json!({ "created": { "my-emoji-1": { "id": "ce-server-1" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let space_id = Id::from("space-eng");
    let blob_id = Id::from("blob-emoji-1");
    let mut input = jmap_chat_client::methods::CustomEmojiCreateInput::new("catjam", &blob_id)
        .with_client_id("my-emoji-1");
    input.space_id = Some(&space_id);
    let _ = sc
        .custom_emoji_create(&input)
        .await
        .expect("custom_emoji_create: must succeed");

    let args = recorded_args(&server).await;
    let create = &args["create"]["my-emoji-1"];
    assert_eq!(create["name"], json!("catjam"), "name mismatch");
    assert_eq!(create["blobId"], json!("blob-emoji-1"), "blobId mismatch");
    assert_eq!(create["spaceId"], json!("space-eng"), "spaceId mismatch");
}

/// `CustomEmoji/set` create with empty `name` must short-circuit before
/// any HTTP request (custom_emoji.rs:73-77).
#[tokio::test]
async fn custom_emoji_create_empty_name_rejected_before_send() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);

    let blob_id = Id::from("blob-emoji-1");
    let input = jmap_chat_client::methods::CustomEmojiCreateInput::new("", &blob_id);
    let err = sc
        .custom_emoji_create(&input)
        .await
        .expect_err("must reject empty name");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("name may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
    let reqs = server
        .received_requests()
        .await
        .expect("recorded_requests must succeed");
    assert!(reqs.is_empty(), "no HTTP request must be sent");
}

/// `CustomEmoji/set` destroy must thread non-empty `ids` to the wire
/// `destroy` key and reject the empty slice client-side
/// (custom_emoji.rs:99-115).
#[tokio::test]
async fn custom_emoji_destroy_threads_ids_and_rejects_empty() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "CustomEmoji/set",
        "ce-1",
        "ce-2",
        json!({ "destroyed": ["ce-doomed"] }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("ce-doomed")];
    let _ = sc
        .custom_emoji_destroy(&ids)
        .await
        .expect("custom_emoji_destroy: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["destroy"], json!(["ce-doomed"]), "destroy must thread");

    // Empty-slice guard.
    let empty: [Id; 0] = [];
    let err = sc
        .custom_emoji_destroy(&empty)
        .await
        .expect_err("must reject empty ids");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("ids may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `CustomEmoji/query` with `filter_space_id` set must serialise a
/// filter object containing `{"spaceId": "<id>"}` and pass through
/// position/limit when provided (custom_emoji.rs:122-138).
#[tokio::test]
async fn custom_emoji_query_filter_space_id_serialises() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "CustomEmoji/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": "ceq-1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let space_id = Id::from("space-eng");
    let mut input = jmap_chat_client::methods::CustomEmojiQueryInput::default();
    input.filter_space_id = Some(&space_id);
    input.position = Some(0);
    input.limit = Some(100);
    let _ = sc
        .custom_emoji_query(&input)
        .await
        .expect("custom_emoji_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"],
        json!({ "spaceId": "space-eng" }),
        "filter must contain spaceId"
    );
    assert_eq!(args["position"], json!(0), "position must thread");
    assert_eq!(args["limit"], json!(100), "limit must thread");
}

/// `CustomEmoji/queryChanges` must thread `since_query_state` to
/// `sinceQueryState` and reject the empty token client-side
/// (custom_emoji.rs:149-153, RFC 8620 §5.6).
#[tokio::test]
async fn custom_emoji_query_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "CustomEmoji/queryChanges",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldQueryState": "ceqc-old",
            "newQueryState": "ceqc-new",
            "total": null,
            "removed": [],
            "added": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("ceqc-old");
    let _ = sc
        .custom_emoji_query_changes(&since, Some(5))
        .await
        .expect("custom_emoji_query_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["sinceQueryState"],
        json!("ceqc-old"),
        "sinceQueryState mismatch"
    );
    assert_eq!(args["maxChanges"], json!(5), "maxChanges mismatch");

    let empty = State::from("");
    let err = sc
        .custom_emoji_query_changes(&empty, None)
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
