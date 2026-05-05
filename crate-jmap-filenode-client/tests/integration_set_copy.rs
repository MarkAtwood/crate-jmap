//! Wiremock integration tests for FileNode/set and FileNode/copy.
//!
//! Oracle for all response shapes:
//!   - draft-ietf-jmap-filenode-13 §3.2.3 (set), §3.2.4 (copy).
//!   - RFC 8620 §5.3 SetResponse envelope.

#[path = "helpers.rs"]
mod helpers;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use jmap_filenode_client::{FileNodeOnExists, FileNodeSetParams};

// ---------------------------------------------------------------------------
// Test 1: FileNode/set with params — create with onDestroyRemoveChildren
// ---------------------------------------------------------------------------

/// FileNode/set with FileNodeSetParams sends onDestroyRemoveChildren and onExists
/// as top-level arguments in the request.
///
/// Oracle: draft-ietf-jmap-filenode-13 §3.2.3 — onDestroyRemoveChildren (Boolean),
/// onExists ("replace"|"rename"), compareCaseInsensitively (Boolean) are top-level
/// arguments on the FileNode/set method call, not nested inside a create object.
#[tokio::test]
async fn file_node_set_create_with_params() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "FileNode/set",
            {
                "accountId": "A13824",
                "oldState": "s10",
                "newState": "s11",
                "created": {
                    "c1": {
                        "id": "new-node-1",
                        "parentId": null,
                        "blobId": null,
                        "target": null,
                        "size": null,
                        "name": "NewDir",
                        "type": null,
                        "shareWith": null,
                        "role": null
                    }
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

    let sc = helpers::make_client(&server).await;
    let params = FileNodeSetParams {
        on_destroy_remove_children: Some(true),
        on_exists: Some(FileNodeOnExists::Replace),
        compare_case_insensitively: Some(false),
    };
    let resp = sc
        .file_node_set(
            Some(json!({
                "c1": { "name": "NewDir", "parentId": null }
            })),
            None,
            None,
            Some(params),
        )
        .await
        .expect("file_node_set_create_with_params: must succeed");

    assert_eq!(resp.new_state, "s11", "newState mismatch");
    let created = resp.created.expect("created must be Some");
    assert!(created.contains_key("c1"), "created must contain key c1");
    assert_eq!(
        created["c1"].id.as_ref(),
        "new-node-1",
        "created node id mismatch"
    );

    // Verify onDestroyRemoveChildren and onExists were sent as top-level args.
    let reqs = server
        .received_requests()
        .await
        .expect("file_node_set_create_with_params: must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["onDestroyRemoveChildren"],
        json!(true),
        "onDestroyRemoveChildren must be true: {args}"
    );
    assert_eq!(
        args["onExists"],
        json!("replace"),
        "onExists must be \"replace\": {args}"
    );
    assert_eq!(
        args["compareCaseInsensitively"],
        json!(false),
        "compareCaseInsensitively must be false: {args}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: FileNode/set destroy — server returns nodeHasChildren error
// ---------------------------------------------------------------------------

/// FileNode/set with destroy returns notDestroyed when the server rejects the
/// destroy because the node still has children.
///
/// Oracle: draft-ietf-jmap-filenode-13 §3.2.3 — servers that do not support
/// onDestroyRemoveChildren=true MUST return a "nodeHasChildren" SetError for
/// any destroy attempt on a non-empty directory. RFC 8620 §5.3 SetError shape.
#[tokio::test]
async fn file_node_set_destroy_node_has_children_error() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "FileNode/set",
            {
                "accountId": "A13824",
                "oldState": "s10",
                "newState": "s10",
                "created": null,
                "updated": null,
                "destroyed": null,
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": {
                    "node-abc": {
                        "type": "nodeHasChildren",
                        "description": "The directory is not empty"
                    }
                }
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server).await;
    let resp = sc
        .file_node_set(None, None, Some(vec!["node-abc"]), None)
        .await
        .expect("file_node_set_destroy_node_has_children_error: must succeed");

    // destroyed must be absent (server rejected the operation).
    assert!(
        resp.destroyed.is_none() || resp.destroyed.as_ref().map_or(true, |v| v.is_empty()),
        "destroyed must be empty or None when destroy fails"
    );

    // notDestroyed must be present and contain the nodeHasChildren error.
    let not_destroyed = resp
        .not_destroyed
        .expect("notDestroyed must be Some when destroy fails");
    let err = not_destroyed
        .get("node-abc")
        .expect("notDestroyed must contain node-abc");
    assert_eq!(
        err.error_type, "nodeHasChildren",
        "error type must be nodeHasChildren; got: {}",
        err.error_type
    );
}

// ---------------------------------------------------------------------------
// Test 3: FileNode/copy with onExists=Rename
// ---------------------------------------------------------------------------

/// FileNode/copy with onExists=Some(FileNodeOnExists::Rename) sends fromAccountId
/// and onExists="rename" in the request.
///
/// Oracle: draft-ietf-jmap-filenode-13 §3.2.4 — fromAccountId is required;
/// onExists controls collision handling for copied nodes.
#[tokio::test]
async fn file_node_copy_with_on_exists_rename() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "FileNode/copy",
            {
                "accountId": "A13824",
                "oldState": "s10",
                "newState": "s11",
                "created": {
                    "copy1": {
                        "id": "copied-node-1",
                        "parentId": null,
                        "blobId": null,
                        "target": null,
                        "size": null,
                        "name": "copied-file.txt",
                        "type": null,
                        "shareWith": null,
                        "role": null
                    }
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

    let sc = helpers::make_client(&server).await;
    let resp = sc
        .file_node_copy(
            "source-account",
            json!({
                "copy1": { "id": "original-node-x", "parentId": null }
            }),
            None, // on_destroy_remove_children
            Some(FileNodeOnExists::Rename),
            None, // compare_case_insensitively
        )
        .await
        .expect("file_node_copy_with_on_exists_rename: must succeed");

    assert_eq!(resp.new_state, "s11", "newState mismatch");
    let created = resp.created.expect("created must be Some");
    assert!(created.contains_key("copy1"), "created must contain copy1");

    // Verify fromAccountId and onExists were sent correctly.
    let reqs = server
        .received_requests()
        .await
        .expect("file_node_copy_with_on_exists_rename: must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["fromAccountId"],
        json!("source-account"),
        "fromAccountId must be source-account: {args}"
    );
    assert_eq!(
        args["onExists"],
        json!("rename"),
        "onExists must be \"rename\": {args}"
    );
}
