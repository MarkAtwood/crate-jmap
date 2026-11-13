//! Wiremock smoke tests for Space root discovery in jmap-chat-client.
//!
//! Spec oracle: draft-atwood-jmap-chat-filenode-00 §Discovering Space
//! Roots — "To locate the Space roots of a given Space, clients issue a
//! `FileNode/query` request filtering on `spaceId: <id>`. The result
//! contains every Space root of that Space, and is empty when the Space
//! has none".

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

/// The request MUST be `FileNode/query` filtering on `spaceId` alone (the
/// draft no longer defines a Space-root role) and declare the filenode
/// capabilities in `using` (RFC 8620 §3.3).
#[tokio::test]
async fn space_roots_sends_space_id_filter_only() {
    let server = MockServer::start().await;
    mock_jmap_post(&server, query_response(json!(["fn-root"]))).await;

    let sc = helpers::make_client(&server);
    let roots = sc
        .space_roots(&Id::from("sp1"))
        .await
        .expect("space_roots must succeed");
    assert_eq!(roots, vec![Id::from("fn-root")]);

    let body = recorded_body(&server).await;
    assert_eq!(body["methodCalls"][0][0], json!("FileNode/query"));
    assert_eq!(
        body["methodCalls"][0][1],
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "filter": { "spaceId": "sp1" }
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

/// "is empty when the Space has none" → an empty `Vec`.
#[tokio::test]
async fn space_roots_absent_is_empty() {
    let server = MockServer::start().await;
    mock_jmap_post(&server, query_response(json!([]))).await;

    let sc = helpers::make_client(&server);
    let roots = sc
        .space_roots(&Id::from("sp1"))
        .await
        .expect("empty result must succeed");
    assert!(roots.is_empty());
}

/// "A Space has zero, one, or more Space roots" (§Space File Tree): every
/// returned id comes back, in the server's order.
#[tokio::test]
async fn space_roots_returns_every_root_in_order() {
    let server = MockServer::start().await;
    mock_jmap_post(&server, query_response(json!(["fn-b", "fn-a"]))).await;

    let sc = helpers::make_client(&server);
    let roots = sc
        .space_roots(&Id::from("sp1"))
        .await
        .expect("several roots must succeed");
    assert_eq!(roots, vec![Id::from("fn-b"), Id::from("fn-a")]);
}
