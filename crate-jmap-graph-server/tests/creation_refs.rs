//! `#creationId` references in GraphNode/set, GraphEdge/set and
//! GraphNodeOp/set Id-typed properties, through the full dispatcher
//! (bd:JMAP-1p88).
//!
//! Oracles: draft-atwood-jmap-graph-00 at jmap-chat-spec 29e9a41 L277
//! ("Like any id-typed property, `graphId` may reference a Graph created
//! earlier in the same request by its creation id"), L306 ("`fromId` and
//! `toId` may reference nodes created earlier in the same request by
//! creation id") and the L354-387 example request, copied verbatim; RFC
//! 8620 §5.3 (rfc8620.txt:1983-1996), which has the server "substitute in
//! the correct value if necessary in later method calls".
//!
//! Each assertion compares the referencing property with the id the server
//! reported for the referenced record, the relation the draft defines, or
//! with a hand-written SetError shape.

mod common;

use std::sync::Arc;

use common::*;
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::{register_graph_edit_handlers, register_graph_handlers};
use jmap_graph_types::GraphEditCapability;
use jmap_server::{Dispatcher, JmapRequest, State};
use jmap_types::{Id, JmapResponse};
use serde_json::{Value, json};

/// Dispatch `method_calls` as one request by [`ALICE`] against `backend`.
async fn dispatch(backend: &Arc<MemoryBackend>, method_calls: Value) -> JmapResponse {
    let mut dispatcher: Dispatcher<Id> = Dispatcher::new();
    register_graph_handlers(&mut dispatcher, Arc::clone(backend));
    register_graph_edit_handlers(&mut dispatcher, Arc::clone(backend));
    let request: JmapRequest = serde_json::from_value(json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:graph",
            "urn:ietf:params:jmap:graph:edit"
        ],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher
        .dispatch(request, Id::from(ALICE), State::from("s0"))
        .await
}

fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn created_id(resp: &JmapResponse, i: usize, key: &str) -> String {
    args(resp, i)["created"][key]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created {key}: {}", args(resp, i)))
        .to_owned()
}

/// The L354-387 example verbatim, then `/get` of what it stored.
#[tokio::test]
async fn building_a_small_graph_example_resolves_creation_ids() {
    let backend = Arc::new(backend());
    let resp = dispatch(
        &backend,
        json!([
            ["Graph/set", {
                "accountId": "A1",
                "create": { "g": { "name": "Design notes" } }
            }, "0"],
            ["GraphNode/set", {
                "accountId": "A1",
                "create": {
                    "a": {
                        "graphId": "#g",
                        "annotations": { "com.example.label": "Root" }
                    },
                    "b": {
                        "graphId": "#g",
                        "blobId": "B7f3",
                        "mediaType": "text/markdown"
                    }
                }
            }, "1"],
            ["GraphEdge/set", {
                "accountId": "A1",
                "create": {
                    "e": {
                        "graphId": "#g",
                        "fromId": "#a",
                        "toId": "#b",
                        "type": "com.example.child"
                    }
                }
            }, "2"]
        ]),
    )
    .await;
    let g = created_id(&resp, 0, "g");
    let a = created_id(&resp, 1, "a");
    let b = created_id(&resp, 1, "b");
    let e = created_id(&resp, 2, "e");

    let resp = dispatch(
        &backend,
        json!([
            ["GraphNode/get", { "accountId": "A1", "ids": [a, b] }, "0"],
            ["GraphEdge/get", { "accountId": "A1", "ids": [e] }, "1"]
        ]),
    )
    .await;
    let nodes = args(&resp, 0)["list"].as_array().expect("node list");
    assert_eq!(nodes.len(), 2, "{}", args(&resp, 0));
    for n in nodes {
        assert_eq!(n["graphId"], g, "{n}");
    }
    let edge = &args(&resp, 1)["list"][0];
    assert_eq!(edge["graphId"], g, "{edge}");
    assert_eq!(edge["fromId"], a, "{edge}");
    assert_eq!(edge["toId"], b, "{edge}");
}

/// L304: the check covers "any update that changes `fromId` or `toId`",
/// and RFC 8620 §5.3 lets a reference "be set (via create or update)".
#[tokio::test]
async fn edge_update_resolves_node_creation_id() {
    let backend = Arc::new(backend());
    let resp = dispatch(
        &backend,
        json!([
            ["Graph/set", { "accountId": "A1", "create": { "g": {} } }, "0"],
            ["GraphNode/set", { "accountId": "A1",
                "create": { "a": { "graphId": "#g" }, "b": { "graphId": "#g" } } }, "1"],
            ["GraphEdge/set", { "accountId": "A1",
                "create": { "e": { "graphId": "#g", "fromId": "#a", "toId": "#b" } } }, "2"]
        ]),
    )
    .await;
    let e = created_id(&resp, 2, "e");

    let resp = dispatch(
        &backend,
        json!([
            ["GraphNode/set", { "accountId": "A1",
                "create": { "c": { "graphId": args(&resp, 0)["created"]["g"]["id"] } } }, "0"],
            ["GraphEdge/set", { "accountId": "A1",
                "update": { (e.clone()): { "toId": "#c" } } }, "1"],
            ["GraphEdge/get", { "accountId": "A1", "ids": [e] }, "2"]
        ]),
    )
    .await;
    let c = created_id(&resp, 0, "c");
    assert!(
        args(&resp, 1)["updated"].get(&e).is_some(),
        "{}",
        args(&resp, 1)
    );
    assert_eq!(args(&resp, 2)["list"][0]["toId"], c, "{}", args(&resp, 2));
}

/// L277: a `graphId` that "does not identify a Graph the user may read" is
/// "an `invalidProperties` SetError naming `graphId`"; an unknown creation
/// id identifies nothing. Likewise L304 for `fromId`.
#[tokio::test]
async fn unknown_creation_id_is_invalid_properties() {
    let backend = Arc::new(backend());
    let resp = dispatch(
        &backend,
        json!([
            ["GraphNode/set", { "accountId": "A1",
                "create": { "a": { "graphId": "#nope" } } }, "0"],
            ["Graph/set", { "accountId": "A1", "create": { "g": {} } }, "1"],
            ["GraphNode/set", { "accountId": "A1",
                "create": { "b": { "graphId": "#g" } } }, "2"],
            ["GraphEdge/set", { "accountId": "A1",
                "create": { "e": { "graphId": "#g", "fromId": "#nope", "toId": "#b" } } }, "3"]
        ]),
    )
    .await;
    let err = &args(&resp, 0)["notCreated"]["a"];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 0));
    assert_eq!(err["properties"], json!(["graphId"]), "{err}");
    let err = &args(&resp, 3)["notCreated"]["e"];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 3));
    assert_eq!(err["properties"], json!(["fromId"]), "{err}");
}

/// RFC 8620 §5.3 applies to every foreign key, including the GraphNodeOp
/// `nodeId` of draft-atwood-jmap-graph-edit-00 §GraphNodeOp/set.
#[tokio::test]
async fn op_node_id_resolves_creation_id() {
    let backend = backend();
    let cap: GraphEditCapability = serde_json::from_value(json!({
        "maxOpSize": 8, "maxCursorSize": 4, "opRetention": null
    }))
    .expect("capability fixture");
    backend.register_edit(&Id::from(ACCOUNT), cap);
    let backend = Arc::new(backend);
    let resp = dispatch(
        &backend,
        json!([
            ["Graph/set", { "accountId": "A1", "create": { "g": {} } }, "0"],
            ["GraphNode/set", { "accountId": "A1", "create": {
                "n": { "graphId": "#g", "editFormat": "application/x-example-ops" }
            } }, "1"],
            ["GraphNodeOp/set", { "accountId": "A1", "create": {
                "o": { "nodeId": "#n", "baseSeq": 0, "data": "AAECAwQF" }
            } }, "2"]
        ]),
    )
    .await;
    let n = created_id(&resp, 1, "n");
    let o = created_id(&resp, 2, "o");
    let resp = dispatch(
        &backend,
        json!([["GraphNodeOp/get", { "accountId": "A1", "ids": [o] }, "0"]]),
    )
    .await;
    assert_eq!(args(&resp, 0)["list"][0]["nodeId"], n, "{}", args(&resp, 0));
}
