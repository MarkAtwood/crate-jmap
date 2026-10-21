//! Wiremock integration tests for FileNode/set and FileNode/copy.
//!
//! Oracle for all response shapes:
//!   - draft-ietf-jmap-filenode-16 §3.2.3 (set), §3.2.4 (copy).
//!   - draft-ietf-jmap-filenode-16 §3.1.5 FileNode examples for the
//!     created node objects.
//!   - RFC 8620 §5.3 SetResponse envelope.

#[path = "helpers.rs"]
mod helpers;

use jmap_filenode_types::NodeType;
use jmap_types::Id;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use jmap_filenode_client::{FileNodeCopyParams, FileNodeOnExists, FileNodeSetParams};

// ---------------------------------------------------------------------------
// Test 1: FileNode/set with params — create with onDestroyRemoveChildren
// ---------------------------------------------------------------------------

/// FileNode/set with FileNodeSetParams sends onDestroyRemoveChildren and onExists
/// as top-level arguments in the request.
///
/// Oracle: draft-ietf-jmap-filenode-16 §3.2.3 — onDestroyRemoveChildren (Boolean),
/// onExists (String|null), compareCaseInsensitively (Boolean) are top-level
/// arguments on the FileNode/set method call, not nested inside a create object.
/// The created node follows the §3.1.5 directory example "n1": `@type` is
/// `"Directory"` and file/symlink properties are omitted.
#[tokio::test]
async fn filenode_set_create_with_params() {
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
                        "@type": "Directory",
                        "id": "new-node-1",
                        "parentId": null,
                        "name": "NewDir",
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

    let sc = helpers::make_client(&server);
    let params = FileNodeSetParams {
        on_destroy_remove_children: Some(true),
        on_exists: Some(FileNodeOnExists::Replace),
        compare_case_insensitively: Some(false),
        extra: serde_json::Map::new(),
    };
    let resp = sc
        .filenode_set(
            Some(json!({
                "c1": { "name": "NewDir", "parentId": null }
            })),
            None,
            None,
            Some(params),
        )
        .await
        .expect("filenode_set_create_with_params: must succeed");

    assert_eq!(resp.new_state, "s11", "newState mismatch");
    let created = resp.created.expect("created must be Some");
    assert!(created.contains_key("c1"), "created must contain key c1");
    assert_eq!(
        created["c1"].id.as_ref(),
        "new-node-1",
        "created node id mismatch"
    );
    assert_eq!(
        created["c1"].node_type,
        Some(NodeType::Directory),
        "created node @type must be Directory"
    );
    assert!(
        created["c1"].extra.is_empty(),
        "created node extra must be empty: {:?}",
        created["c1"].extra
    );

    // Verify onDestroyRemoveChildren and onExists were sent as top-level args.
    let reqs = server
        .received_requests()
        .await
        .expect("filenode_set_create_with_params: must have recorded requests");
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
/// Oracle: draft-ietf-jmap-filenode-16 §3.2.3 — "nodeHasChildren" is returned
/// when a destroy was attempted on a FileNode that has children and
/// onDestroyRemoveChildren is false. RFC 8620 §5.3 SetError shape.
#[tokio::test]
async fn filenode_set_destroy_node_has_children_error() {
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

    let sc = helpers::make_client(&server);
    let resp = sc
        .filenode_set(None, None, Some(vec![Id::from("node-abc")]), None)
        .await
        .expect("filenode_set_destroy_node_has_children_error: must succeed");

    // destroyed must be absent (server rejected the operation).
    assert!(
        resp.destroyed.is_none() || resp.destroyed.as_ref().is_none_or(|v| v.is_empty()),
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
/// Oracle: RFC 8620 §5.4 — fromAccountId is required; draft-ietf-jmap-filenode-16
/// §3.2.4 — onExists controls collision handling for copied nodes, as on
/// FileNode/set. The created node follows the §3.1.5 file example "n3":
/// `@type` is `"File"` and directory/symlink properties are omitted.
#[tokio::test]
async fn filenode_copy_with_on_exists_rename() {
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
                        "@type": "File",
                        "id": "copied-node-1",
                        "parentId": null,
                        "name": "report.pdf",
                        "blobId": "Bfa61c0",
                        "size": 48576,
                        "mediaType": "application/pdf",
                        "executable": false
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

    let sc = helpers::make_client(&server);
    let resp = sc
        .filenode_copy(
            FileNodeCopyParams {
                from_account_id: Id::from("source-account"),
                on_destroy_remove_children: None,
                on_exists: Some(FileNodeOnExists::Rename),
                compare_case_insensitively: None,
                if_from_in_state: None,
                if_in_state: None,
                on_success_destroy_original: None,
                destroy_from_if_in_state: None,
                extra: serde_json::Map::new(),
            },
            json!({
                "copy1": { "id": "original-node-x", "parentId": null }
            }),
        )
        .await
        .expect("filenode_copy_with_on_exists_rename: must succeed");

    assert_eq!(resp.new_state, "s11", "newState mismatch");
    let created = resp.created.expect("created must be Some");
    assert!(created.contains_key("copy1"), "created must contain copy1");

    // The -16 wire keys must land in typed fields, not in `extra`.
    let copied = &created["copy1"];
    assert_eq!(
        copied.node_type,
        Some(NodeType::File),
        "copied node @type must be File"
    );
    assert_eq!(
        copied.blob_id,
        Some(Id::from("Bfa61c0")),
        "copied node blobId mismatch"
    );
    assert_eq!(copied.size, Some(48576), "copied node size mismatch");
    assert_eq!(
        copied.media_type.as_deref(),
        Some("application/pdf"),
        "copied node mediaType mismatch"
    );
    assert_eq!(
        copied.executable,
        Some(false),
        "copied node executable mismatch"
    );
    assert!(
        copied.extra.is_empty(),
        "copied node extra must be empty: {:?}",
        copied.extra
    );

    // Verify fromAccountId and onExists were sent correctly.
    let reqs = server
        .received_requests()
        .await
        .expect("filenode_copy_with_on_exists_rename: must have recorded requests");
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

// ---------------------------------------------------------------------------
// FileNode/copy — `destroyed` from onExists replacement
// ---------------------------------------------------------------------------

/// A FileNode/copy response carrying `destroyed` decodes into
/// `SetResponse::destroyed`, and one without it decodes to `None`.
///
/// Oracle: draft-ietf-jmap-filenode-16 L677-679 ("the server MUST include
/// the id of the replaced item in the destroyed response list"), applied to
/// FileNode/copy by L773-775; workspace decision bd JMAP-510h.55 omits the
/// field when empty. Response bodies are hand-written.
#[tokio::test]
async fn filenode_copy_decodes_destroyed() {
    for (destroyed, expected) in [
        (
            Some(json!(["old-dir", "old-child"])),
            Some(vec![Id::from("old-dir"), Id::from("old-child")]),
        ),
        (None, None),
    ] {
        let server = MockServer::start().await;
        let mut args = json!({
            "fromAccountId": "source-account",
            "accountId": "A13824",
            "oldState": "s10",
            "newState": "s11",
            "created": {
                "copy1": {
                    "@type": "Directory",
                    "id": "copied-node-1",
                    "parentId": null,
                    "name": "Documents"
                }
            },
            "notCreated": null
        });
        if let Some(d) = destroyed {
            args["destroyed"] = d;
        }
        let resp_body = json!({
            "sessionState": "s1",
            "methodResponses": [["FileNode/copy", args, "r1"]]
        });
        Mock::given(method("POST"))
            .and(path("/api/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
            .mount(&server)
            .await;

        let sc = helpers::make_client(&server);
        let resp = sc
            .filenode_copy(
                FileNodeCopyParams {
                    from_account_id: Id::from("source-account"),
                    on_destroy_remove_children: Some(true),
                    on_exists: Some(FileNodeOnExists::Replace),
                    compare_case_insensitively: None,
                    if_from_in_state: None,
                    if_in_state: None,
                    on_success_destroy_original: None,
                    destroy_from_if_in_state: None,
                    extra: serde_json::Map::new(),
                },
                json!({ "copy1": { "id": "original-node-x" } }),
            )
            .await
            .expect("filenode_copy_decodes_destroyed: must succeed");

        assert_eq!(resp.destroyed, expected);
        assert!(
            !resp.extra.contains_key("destroyed"),
            "destroyed must land in the typed field: {:?}",
            resp.extra
        );
    }
}

// ---------------------------------------------------------------------------
// FileNode/copy — RFC 8620 §5.4 standard /copy arguments
// ---------------------------------------------------------------------------
//
// Oracle: RFC 8620 §5.4 names the optional /copy arguments
// `ifFromInState: "String|null"`, `ifInState: "String|null"`,
// `onSuccessDestroyOriginal: "Boolean" (default: false)` and
// `destroyFromIfInState: "String|null"`. Expected request arguments are
// hand-written from that text; `create` is checked by other tests.

async fn filenode_copy_sent_args(params: FileNodeCopyParams) -> serde_json::Value {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "FileNode/copy",
            {
                "accountId": "A13824",
                "fromAccountId": "src",
                "oldState": "s1",
                "newState": "s2",
                "created": null,
                "notCreated": null
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
    sc.filenode_copy(params, json!({ "k1": { "id": "node-src" } }))
        .await
        .expect("filenode_copy must succeed");
    let reqs = server.received_requests().await.expect("recorded requests");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).expect("JSON body");
    let mut args = body["methodCalls"][0][1].clone();
    args.as_object_mut()
        .expect("args object")
        .remove("create")
        .expect("create must be sent");
    args
}

/// All four RFC 8620 §5.4 optional arguments reach the wire under their
/// spec names; vendor extras pass through, but a colliding typed key
/// keeps its typed value (workspace extras-preservation policy).
#[tokio::test]
async fn filenode_copy_sends_rfc8620_copy_args() {
    let params = FileNodeCopyParams {
        from_account_id: jmap_types::Id::from("src"),
        on_destroy_remove_children: None,
        on_exists: None,
        compare_case_insensitively: None,
        if_from_in_state: Some("from-s5".into()),
        if_in_state: Some("to-s7".into()),
        on_success_destroy_original: Some(true),
        destroy_from_if_in_state: Some("from-s5".into()),
        extra: serde_json::Map::from_iter([
            ("acmeCorpAudit".to_owned(), json!(true)),
            ("fromAccountId".to_owned(), json!("collides")),
        ]),
    };
    assert_eq!(
        filenode_copy_sent_args(params).await,
        json!({
            "accountId": "A13824",
            "fromAccountId": "src",
            "ifFromInState": "from-s5",
            "ifInState": "to-s7",
            "onSuccessDestroyOriginal": true,
            "destroyFromIfInState": "from-s5",
            "acmeCorpAudit": true
        })
    );
}

/// Unset optional arguments are omitted, leaving the server defaults of
/// RFC 8620 §5.4 in force.
#[tokio::test]
async fn filenode_copy_omits_unset_copy_args() {
    let params = FileNodeCopyParams {
        from_account_id: jmap_types::Id::from("src"),
        on_destroy_remove_children: None,
        on_exists: None,
        compare_case_insensitively: None,
        if_from_in_state: None,
        if_in_state: None,
        on_success_destroy_original: None,
        destroy_from_if_in_state: None,
        extra: serde_json::Map::new(),
    };
    assert_eq!(
        filenode_copy_sent_args(params).await,
        json!({ "accountId": "A13824", "fromAccountId": "src" })
    );
}
