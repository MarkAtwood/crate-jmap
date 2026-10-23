//! Wiremock smoke tests for Space file tree discovery in jmap-chat-client.
//!
//! Spec oracle: draft-atwood-jmap-chat-filenode-00 §Discovering the File
//! Tree Root — "To locate the file tree root for a given Space, clients
//! issue a `FileNode/query` request filtering on
//! `role: "chatspace"` and `spaceId: <id>`."

#[path = "helpers.rs"]
mod helpers;

use helpers::{TEST_ACCOUNT_ID, jmap_response, mock_jmap_post, recorded_body};
use jmap_types::Id;
use serde_json::json;
use wiremock::MockServer;

fn query_response(ids: serde_json::Value) -> serde_json::Value {
    jmap_response(
        "FileNode/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": "fq-1",
            "canCalculateChanges": false,
            "position": 0,
            "ids": ids
        }),
    )
}

/// The request MUST be `FileNode/query` with the role + spaceId filter
/// from the draft, and declare the filenode capabilities in `using`
/// (RFC 8620 §3.3). "When a file tree exists for the Space, exactly one
/// matching FileNode is returned."
#[tokio::test]
async fn space_file_tree_root_sends_role_and_space_filter() {
    let server = MockServer::start().await;
    mock_jmap_post(&server, query_response(json!(["fn-root"]))).await;

    let sc = helpers::make_client(&server);
    let root = sc
        .space_file_tree_root(&Id::from("sp1"))
        .await
        .expect("space_file_tree_root must succeed");
    assert_eq!(root, Some(Id::from("fn-root")));

    let body = recorded_body(&server).await;
    assert_eq!(body["methodCalls"][0][0], json!("FileNode/query"));
    assert_eq!(
        body["methodCalls"][0][1],
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "filter": {
                "role": "chatspace",
                "spaceId": "sp1"
            }
        })
    );
    assert_eq!(
        body["using"],
        json!([
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:filenode",
            "urn:ietf:params:jmap:chat:filenode"
        ])
    );
}

/// "When no file tree exists, the result is empty" → `Ok(None)`.
#[tokio::test]
async fn space_file_tree_root_absent_is_none() {
    let server = MockServer::start().await;
    mock_jmap_post(&server, query_response(json!([]))).await;

    let sc = helpers::make_client(&server);
    let root = sc
        .space_file_tree_root(&Id::from("sp1"))
        .await
        .expect("empty result must succeed");
    assert!(root.is_none());
}

/// "at most one root per Space is permitted" — a server returning two
/// roots is non-compliant and surfaces as `UnexpectedResponse` rather
/// than an arbitrary pick.
#[tokio::test]
async fn space_file_tree_root_multiple_roots_is_error() {
    let server = MockServer::start().await;
    mock_jmap_post(&server, query_response(json!(["fn-a", "fn-b"]))).await;

    let sc = helpers::make_client(&server);
    let err = sc
        .space_file_tree_root(&Id::from("sp1"))
        .await
        .expect_err("two roots must be rejected");
    assert!(
        matches!(err, jmap_base_client::ClientError::UnexpectedResponse(_)),
        "expected UnexpectedResponse, got {err:?}"
    );
}
