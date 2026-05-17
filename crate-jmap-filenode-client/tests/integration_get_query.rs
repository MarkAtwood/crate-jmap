//! Wiremock integration tests for FileNode/get, /changes, /query, /queryChanges.
//!
//! Oracle for all response shapes:
//!   - draft-ietf-jmap-filenode-13 §3.2.1 (get), §3.2.2 (changes),
//!     §3.2.5 (query), §3.2.6 (queryChanges).
//!   - RFC 8620 §3.4 JMAP batch response envelope.

#[path = "helpers.rs"]
mod helpers;

use jmap_types::{Id, State};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// FileNode fixture helpers
// ---------------------------------------------------------------------------

/// Minimal valid FileNode JSON for a file node (all required-nullable fields explicit).
///
/// Oracle: draft-ietf-jmap-filenode-13 §3.1 field table — parentId, blobId, target,
/// size, type, shareWith, role are required-and-nullable (must appear as null or value).
fn file_node_fixture(
    id: &str,
    name: &str,
    node_type: &str,
    parent_id: Option<&str>,
) -> serde_json::Value {
    json!({
        "id": id,
        "name": name,
        "nodeType": node_type,
        "parentId": parent_id,
        "blobId": null,
        "target": null,
        "size": null,
        "type": null,
        "shareWith": null,
        "role": null
    })
}

// ---------------------------------------------------------------------------
// Test 1: FileNode/get with fetchParents=true
// ---------------------------------------------------------------------------

/// FileNode/get with fetchParents=Some(true) sends fetchParents in request and
/// returns the fetched node list including parent nodes.
///
/// Oracle: draft-ietf-jmap-filenode-13 §3.2.1 — fetchParents argument causes server
/// to also return ancestor nodes for every requested ID.
/// Response: RFC 8620 §5.1 GetResponse shape.
#[tokio::test]
async fn file_node_get_with_fetch_parents() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "FileNode/get",
            {
                "accountId": "A13824",
                "state": "s5",
                "list": [
                    file_node_fixture("fn1", "report.pdf", "file", Some("dir1")),
                    file_node_fixture("dir1", "docs", "directory", None)
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
    let resp = sc
        .file_node_get(Some(&[Id::from("fn1")]), None, Some(true))
        .await
        .expect("file_node_get_with_fetch_parents: must succeed");

    assert_eq!(resp.list.len(), 2, "list must have 2 nodes (file + parent)");
    assert!(
        resp.list.iter().any(|n| n.id.as_ref() == "fn1"),
        "list must contain fn1"
    );
    assert!(
        resp.list.iter().any(|n| n.id.as_ref() == "dir1"),
        "list must contain parent dir1"
    );

    // Verify fetchParents was sent in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("file_node_get_with_fetch_parents: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("file_node_get_with_fetch_parents: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["fetchParents"],
        json!(true),
        "fetchParents must be true in request: {args}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: FileNode/get without fetchParents omits the key
// ---------------------------------------------------------------------------

/// FileNode/get with fetchParents=None must NOT include fetchParents in the request.
///
/// Oracle: draft-ietf-jmap-filenode-13 §3.2.1 — fetchParents is optional; when
/// absent the server uses its default (no parent fetching). Sending null would be
/// a protocol error; the key must simply be absent.
#[tokio::test]
async fn file_node_get_without_fetch_parents_omits_field() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "FileNode/get",
            {
                "accountId": "A13824",
                "state": "s5",
                "list": [
                    file_node_fixture("fn2", "notes.txt", "file", None)
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
    sc.file_node_get(Some(&[Id::from("fn2")]), None, None)
        .await
        .expect("file_node_get_without_fetch_parents_omits_field: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("file_node_get_without_fetch_parents_omits_field: must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args.get("fetchParents").is_none(),
        "fetchParents must be absent from request when None is passed: {args}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: FileNode/changes returns change lists
// ---------------------------------------------------------------------------

/// FileNode/changes returns correct oldState/newState and change ID lists.
///
/// Oracle: RFC 8620 §5.2 /changes response — oldState, newState, hasMoreChanges,
/// created, updated, destroyed arrays. sinceState in request maps to oldState in response.
#[tokio::test]
async fn file_node_changes_returns_change_lists() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "FileNode/changes",
            {
                "accountId": "A13824",
                "oldState": "s3",
                "newState": "s4",
                "hasMoreChanges": false,
                "created": ["n10", "n11"],
                "updated": ["n5"],
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
    let resp = sc
        .file_node_changes(&State::from("s3"), None)
        .await
        .expect("file_node_changes_returns_change_lists: must succeed");

    assert_eq!(resp.old_state, "s3", "oldState mismatch");
    assert_eq!(resp.new_state, "s4", "newState mismatch");
    assert!(!resp.has_more_changes, "hasMoreChanges must be false");
    assert!(
        resp.created.iter().any(|id| id.as_ref() == "n10"),
        "created must contain n10"
    );
    assert!(
        resp.created.iter().any(|id| id.as_ref() == "n11"),
        "created must contain n11"
    );
    assert!(
        resp.updated.iter().any(|id| id.as_ref() == "n5"),
        "updated must contain n5"
    );
}

// ---------------------------------------------------------------------------
// Test 4: FileNode/query with depth and filter
// ---------------------------------------------------------------------------

/// FileNode/query with depth=Some(2) and a parentId filter sends both in the request
/// and returns the matching ID list.
///
/// Oracle: draft-ietf-jmap-filenode-13 §3.2.5 — depth argument controls recursive
/// descent; filter is a standard RFC 8620 FilterCondition object.
/// Response: RFC 8620 §5.5 QueryResponse shape.
#[tokio::test]
async fn file_node_query_with_depth_and_filter() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "FileNode/query",
            {
                "accountId": "A13824",
                "queryState": "qs3",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["f1", "f2", "f3"],
                "total": 3
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
    let resp = sc
        .file_node_query(
            Some(json!({ "parentId": "dir1" })),
            None,
            None,
            None,
            Some(2u64),
        )
        .await
        .expect("file_node_query_with_depth_and_filter: must succeed");

    assert_eq!(resp.ids.len(), 3, "ids must have 3 items");
    assert_eq!(resp.query_state, "qs3", "queryState mismatch");

    // Verify depth and filter were sent in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("file_node_query_with_depth_and_filter: must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["depth"],
        json!(2),
        "depth must be 2 in request: {args}"
    );
    assert_eq!(
        args["filter"]["parentId"],
        json!("dir1"),
        "filter.parentId must be dir1 in request: {args}"
    );
}

// ---------------------------------------------------------------------------
// Test 5: FileNode/queryChanges returns added and removed
// ---------------------------------------------------------------------------

/// FileNode/queryChanges returns removed IDs and added items with index.
///
/// Oracle: RFC 8620 §5.6 /queryChanges response — removed is an array of IDs,
/// added is an array of AddedItem objects with {id, index}.
#[tokio::test]
async fn file_node_query_changes_returns_added_and_removed() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "FileNode/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "qs1",
                "newQueryState": "qs2",
                "total": 5,
                "removed": ["n8"],
                "added": [
                    { "id": "n12", "index": 2 }
                ]
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
    let resp = sc
        .file_node_query_changes(&State::from("qs1"), None, None, None, None, None)
        .await
        .expect("file_node_query_changes_returns_added_and_removed: must succeed");

    assert_eq!(resp.old_query_state, "qs1", "oldQueryState mismatch");
    assert_eq!(resp.new_query_state, "qs2", "newQueryState mismatch");
    assert!(
        resp.removed.iter().any(|id| id.as_ref() == "n8"),
        "removed must contain n8"
    );
    assert_eq!(resp.added.len(), 1, "added must have 1 item");
    assert_eq!(resp.added[0].id.as_ref(), "n12", "added[0].id must be n12");
    assert_eq!(resp.added[0].index, 2, "added[0].index must be 2");
}
