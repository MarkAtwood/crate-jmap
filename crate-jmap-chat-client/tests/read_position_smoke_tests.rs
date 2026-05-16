//! Wiremock smoke tests for `ReadPosition/*` method paths in
//! jmap-chat-client.
//!
//! Spec oracles:
//!   - RFC 8620 §5.1 /get, §5.2 /changes, §5.3 /set
//!   - draft-atwood-jmap-chat-00 §4.20 (ReadPosition object) and §5
//!     (ReadPosition/* method shapes — only `update` is supported on
//!     `/set`; `create` and `destroy` are forbidden)

#[path = "helpers.rs"]
mod helpers;

use helpers::{
    jmap_response, mock_jmap_post, recorded_args, recorded_body, set_response, TEST_ACCOUNT_ID,
};
use jmap_types::{Id, State};
use serde_json::json;
use wiremock::MockServer;

/// `ReadPosition/get` with `ids: None` must omit the `ids` key on the
/// wire (misc.rs:25-29). ReadPosition/get has NO `properties`
/// parameter (unlike Chat/get etc.) so the wire MUST not contain a
/// properties key in either branch. Pins the USING_CHAT capability set
/// for the entire ReadPosition/* family (one assertion per
/// method-family per workspace convention).
#[tokio::test]
async fn read_position_get_omits_ids_when_none() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ReadPosition/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "rp-state-1",
            "list": [],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let _ = sc
        .read_position_get(None)
        .await
        .expect("read_position_get: must succeed");

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
        "ReadPosition/get must never serialise properties (spec has no such arg)"
    );
    // RFC 8620 §3.3 — ReadPosition/* MUST declare USING_CHAT.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]),
        "ReadPosition/* using must equal USING_CHAT exactly"
    );
}

/// `ReadPosition/get` with non-empty `ids` must thread the slice
/// verbatim. Decode coverage of a populated ReadPosition (covering
/// `last_read_message_id` and `last_read_at` optionals) is folded in.
#[tokio::test]
async fn read_position_get_decodes_populated_record_and_threads_ids() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ReadPosition/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "rp-state-2",
            "list": [
                {
                    "id": "rp-1",
                    "chatId": "chat-1",
                    "lastReadMessageId": "msg-42",
                    "lastReadAt": "2026-01-20T15:00:00Z"
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("rp-1")];
    let resp = sc
        .read_position_get(Some(&ids))
        .await
        .expect("read_position_get: must succeed");

    assert_eq!(resp.list.len(), 1, "list must contain one ReadPosition");
    let rp = &resp.list[0];
    assert_eq!(rp.id.as_ref(), "rp-1", "id mismatch");
    assert_eq!(rp.chat_id.as_ref(), "chat-1", "chat_id mismatch");
    assert_eq!(
        rp.last_read_message_id.as_ref().map(|id| id.as_ref()),
        Some("msg-42"),
        "last_read_message_id mismatch"
    );
    assert_eq!(
        rp.last_read_at.as_ref().map(|d| d.as_ref()),
        Some("2026-01-20T15:00:00Z"),
        "last_read_at mismatch"
    );

    let args = recorded_args(&server).await;
    assert_eq!(args["ids"], json!(["rp-1"]), "ids must thread verbatim");
}

/// `ReadPosition/set` update must emit only `lastReadMessageId` keyed by
/// the position id; `create` and `destroy` are forbidden by the spec so
/// the client only supports update (misc.rs:43-58). The wire request
/// must not include `create` or `destroy` keys (the implementation
/// builds args directly with only `update`).
#[tokio::test]
async fn read_position_update_serialises_last_read_message_id() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "ReadPosition/set",
        "rp-1",
        "rp-2",
        json!({ "updated": { "rp-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let rp_id = Id::from("rp-1");
    let msg_id = Id::from("msg-42");
    let _ = sc
        .read_position_update(&rp_id, &msg_id)
        .await
        .expect("read_position_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["rp-1"],
        json!({ "lastReadMessageId": "msg-42" }),
        "update must contain only lastReadMessageId"
    );
    assert!(
        args.get("create").is_none(),
        "create must be absent (forbidden by spec for ReadPosition)"
    );
    assert!(
        args.get("destroy").is_none(),
        "destroy must be absent (forbidden by spec for ReadPosition)"
    );
}

/// `ReadPosition/changes` must thread `since_state` and `max_changes`
/// and reject empty `since_state` client-side (misc.rs:86-90,
/// RFC 8620 §5.2).
#[tokio::test]
async fn read_position_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ReadPosition/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": "rp-old",
            "newState": "rp-new",
            "hasMoreChanges": false,
            "created": [],
            "updated": ["rp-1"],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("rp-old");
    let _ = sc
        .read_position_changes(&since, Some(20))
        .await
        .expect("read_position_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["sinceState"], json!("rp-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(20), "maxChanges mismatch");

    // Empty-state guard.
    let empty = State::from("");
    let err = sc
        .read_position_changes(&empty, None)
        .await
        .expect_err("read_position_changes must reject empty since_state");
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
