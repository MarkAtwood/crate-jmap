//! `GraphNode/set`, `GraphEdge/set` and their `/get` and `/changes`
//! (draft-atwood-jmap-graph-00 §GraphNode, §GraphEdge, §GraphNode/set,
//! §GraphEdge/set; L175-234, L273-308 at jmap-chat-spec 6bcd9d8). Each
//! assertion quotes the draft text it checks.

#[macro_use]
mod common;

use common::*;
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::{
    handle_graph_edge_changes, handle_graph_edge_get, handle_graph_edge_set, handle_graph_node_get,
    handle_graph_node_set, handle_graph_set,
};
use serde_json::{Value, json};

/// A Graph of [`ALICE`]'s shared with `share_with`.
async fn graph(backend: &MemoryBackend, share_with: Value) -> String {
    let resp = call!(
        handle_graph_set,
        backend,
        ALICE,
        json!({ "create": { "g": { "shareWith": share_with } } })
    );
    resp["created"]["g"]["id"]
        .as_str()
        .expect("graph")
        .to_owned()
}

async fn nodes(backend: &MemoryBackend, caller: &str, create: Value) -> Value {
    call!(
        handle_graph_node_set,
        backend,
        caller,
        json!({ "create": create })
    )
}

async fn node(backend: &MemoryBackend, graph: &str) -> String {
    let resp = nodes(backend, ALICE, json!({ "n": { "graphId": graph } })).await;
    resp["created"]["n"]["id"]
        .as_str()
        .expect("node")
        .to_owned()
}

async fn edges(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(handle_graph_edge_set, backend, caller, args)
}

/// The L367-391 example's node `b` and edge `e`, with the Graph and node
/// ids the earlier calls returned in place of `#g`, `#a` and `#b`.
#[tokio::test]
async fn building_a_small_graph() {
    let backend = backend();
    let g = graph(&backend, Value::Null).await;
    let resp = nodes(
        &backend,
        ALICE,
        json!({
            "a": { "graphId": g, "annotations": { "com.example.label": "Root" } },
            "b": { "graphId": g, "blobId": "B7f3", "mediaType": "text/markdown" }
        }),
    )
    .await;
    let a = resp["created"]["a"]["id"].as_str().expect("a").to_owned();
    let b = resp["created"]["b"]["id"].as_str().expect("b").to_owned();
    // L188-207: the defaults and the server-set properties; `createdBy` is
    // "The id of the Principal ... that created the node".
    assert_eq!(resp["created"]["a"]["blobId"], Value::Null, "{resp}");
    assert_eq!(resp["created"]["a"]["uri"], Value::Null, "{resp}");
    assert_eq!(resp["created"]["b"]["annotations"], json!({}), "{resp}");
    assert_eq!(resp["created"]["b"]["createdBy"], ALICE, "{resp}");
    assert_eq!(resp["created"]["b"]["updatedBy"], ALICE, "{resp}");

    let resp = edges(
        &backend,
        ALICE,
        json!({ "create": { "e": {
            "graphId": g, "fromId": a, "toId": b, "type": "com.example.child"
        } } }),
    )
    .await;
    let e = resp["created"]["e"]["id"].as_str().expect("e").to_owned();
    let got = call!(
        handle_graph_edge_get,
        &backend,
        ALICE,
        json!({ "ids": [e] })
    );
    let edge = &got["list"][0];
    assert_eq!(edge["fromId"], json!(a), "{got}");
    assert_eq!(edge["toId"], json!(b), "{got}");
    assert_eq!(edge["type"], "com.example.child", "{got}");
}

/// L277: "On create, `graphId` is required. If it does not identify a
/// Graph the user may read, the server rejects the create with an
/// `invalidProperties` SetError naming `graphId`"; L451 "the
/// `invalidProperties` errors described above rather than `forbidden`".
#[tokio::test]
async fn node_create_graph_id() {
    let backend = backend();
    let g = graph(&backend, Value::Null).await;
    let resp = nodes(&backend, ALICE, json!({ "n": {} })).await;
    assert_eq!(resp["notCreated"]["n"]["properties"], json!(["graphId"]));
    let resp = nodes(&backend, ALICE, json!({ "n": { "graphId": "nope" } })).await;
    assert_eq!(
        resp["notCreated"]["n"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notCreated"]["n"]["properties"], json!(["graphId"]));
    let resp = nodes(&backend, BOB, json!({ "n": { "graphId": g } })).await;
    assert_eq!(
        resp["notCreated"]["n"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notCreated"]["n"]["properties"], json!(["graphId"]));
}

/// L275: "Creating, updating, or destroying a node requires `mayWrite` on
/// its Graph"; with `mayRead` only, the needed right is missing
/// (`forbidden`, as L250 gives for Graph/set).
#[tokio::test]
async fn node_set_needs_may_write() {
    let backend = backend();
    let g = graph(
        &backend,
        json!({ BOB: rights(true, false, false), CAROL: rights(true, true, false) }),
    )
    .await;
    let n = node(&backend, &g).await;

    let resp = nodes(&backend, BOB, json!({ "x": { "graphId": g } })).await;
    assert_eq!(resp["notCreated"]["x"]["type"], "forbidden", "{resp}");
    let resp = call!(
        handle_graph_node_set,
        &backend,
        BOB,
        json!({ "update": { (n.clone()): { "mediaType": "text/plain" } }, "destroy": [n] })
    );
    assert_eq!(resp["notUpdated"][&n]["type"], "forbidden", "{resp}");
    assert_eq!(resp["notDestroyed"][&n]["type"], "forbidden", "{resp}");

    let resp = nodes(&backend, CAROL, json!({ "x": { "graphId": g } })).await;
    assert!(resp["created"]["x"].is_object(), "{resp}");
    // L206: `updatedBy` is "The id of the Principal that last changed the
    // node".
    let resp = call!(
        handle_graph_node_set,
        &backend,
        CAROL,
        json!({ "update": { (n.clone()): { "mediaType": "text/plain" } } })
    );
    assert_eq!(resp["updated"][&n]["updatedBy"], CAROL, "{resp}");
    let got = call!(
        handle_graph_node_get,
        &backend,
        ALICE,
        json!({ "ids": [n] })
    );
    assert_eq!(got["list"][0]["createdBy"], ALICE, "{got}");
    assert_eq!(got["list"][0]["updatedBy"], CAROL, "{got}");
}

/// L451: the nodes of a Graph the user may not read are `notFound` on
/// `/get` and on `/set` update or destroy.
#[tokio::test]
async fn nodes_of_unreadable_graph_are_not_found() {
    let backend = backend();
    let g = graph(&backend, Value::Null).await;
    let n = node(&backend, &g).await;
    let got = call!(handle_graph_node_get, &backend, BOB, json!({ "ids": [n] }));
    assert_eq!(got["list"], json!([]), "{got}");
    assert_eq!(got["notFound"], json!([n]), "{got}");
    let got = call!(handle_graph_node_get, &backend, BOB, json!({ "ids": null }));
    assert_eq!(got["list"], json!([]), "{got}");
    let resp = call!(
        handle_graph_node_set,
        &backend,
        BOB,
        json!({ "update": { (n.clone()): { "mediaType": null } }, "destroy": [n] })
    );
    assert_eq!(resp["notUpdated"][&n]["type"], "notFound", "{resp}");
    assert_eq!(resp["notDestroyed"][&n]["type"], "notFound", "{resp}");
}

/// L195: "The server rejects a value that is not a URI with an
/// `invalidProperties` SetError naming `uri`"; L342 gives
/// `jmap:graphnode:A2:N9` as a usable URI.
#[tokio::test]
async fn node_uri() {
    let backend = backend();
    let g = graph(&backend, Value::Null).await;
    let resp = nodes(
        &backend,
        ALICE,
        json!({
            "bad": { "graphId": g, "uri": "not a uri" },
            "jmap": { "graphId": g, "uri": "jmap:graphnode:A2:N9" },
            "web": { "graphId": g, "uri": "https://example.com/a?b=c#d" }
        }),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["bad"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notCreated"]["bad"]["properties"], json!(["uri"]));
    assert!(resp["created"]["jmap"].is_object(), "{resp}");
    assert!(resp["created"]["web"].is_object(), "{resp}");
}

/// L182: `graphId` is "(Id, immutable)".
#[tokio::test]
async fn graph_id_is_immutable() {
    let backend = backend();
    let g = graph(&backend, Value::Null).await;
    let other = graph(&backend, Value::Null).await;
    let n = node(&backend, &g).await;
    let resp = call!(
        handle_graph_node_set,
        &backend,
        ALICE,
        json!({ "update": { (n.clone()): { "graphId": other } } })
    );
    assert_eq!(
        resp["notUpdated"][&n]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notUpdated"][&n]["properties"], json!(["graphId"]));
}

/// L304: "On create, and on any update that changes `fromId` or `toId`, the
/// server MUST verify that each of `fromId` and `toId` identifies an
/// existing GraphNode in the edge's Graph. If either does not, the server
/// MUST reject the operation with an `invalidProperties` SetError naming
/// the failing property."
#[tokio::test]
async fn edge_endpoints_in_same_graph() {
    let backend = backend();
    let g = graph(&backend, Value::Null).await;
    let other = graph(&backend, Value::Null).await;
    let (a, b) = (node(&backend, &g).await, node(&backend, &g).await);
    let foreign = node(&backend, &other).await;

    let resp = edges(
        &backend,
        ALICE,
        json!({ "create": {
            "x": { "graphId": g, "fromId": foreign, "toId": b },
            "y": { "graphId": g, "fromId": a, "toId": "missing" },
            "z": { "graphId": g, "fromId": a }
        } }),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["x"]["properties"],
        json!(["fromId"]),
        "{resp}"
    );
    assert_eq!(
        resp["notCreated"]["y"]["properties"],
        json!(["toId"]),
        "{resp}"
    );
    // L304: "`graphId`, `fromId`, and `toId` are required".
    assert_eq!(
        resp["notCreated"]["z"]["properties"],
        json!(["toId"]),
        "{resp}"
    );

    // L223: `toId` "may equal `fromId`".
    let resp = edges(
        &backend,
        ALICE,
        json!({ "create": { "loop": { "graphId": g, "fromId": a, "toId": a } } }),
    )
    .await;
    let e = resp["created"]["loop"]["id"]
        .as_str()
        .expect("loop")
        .to_owned();

    let resp = edges(
        &backend,
        ALICE,
        json!({ "update": { (e.clone()): { "toId": foreign } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&e]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"][&e]["properties"],
        json!(["toId"]),
        "{resp}"
    );
    // L234: "An edge's `fromId` and `toId` may be changed by an update".
    let resp = edges(
        &backend,
        ALICE,
        json!({ "update": { (e.clone()): { "toId": b } } }),
    )
    .await;
    assert!(resp["updated"].get(&e).is_some(), "{resp}");
}

/// L279: "When a node is destroyed, the server MUST also destroy every
/// GraphEdge whose `fromId` or `toId` is that node. These edges are
/// reported as destroyed by subsequent `GraphEdge/changes` calls." L308:
/// "Destroying an edge does not affect its nodes."
#[tokio::test]
async fn node_destroy_cascades_to_its_edges() {
    let backend = backend();
    let g = graph(&backend, Value::Null).await;
    let (a, b, c) = (
        node(&backend, &g).await,
        node(&backend, &g).await,
        node(&backend, &g).await,
    );
    let resp = edges(
        &backend,
        ALICE,
        json!({ "create": {
            "out": { "graphId": g, "fromId": a, "toId": b },
            "in": { "graphId": g, "fromId": c, "toId": a },
            "keep": { "graphId": g, "fromId": b, "toId": c }
        } }),
    )
    .await;
    let id = |k: &str| resp["created"][k]["id"].as_str().expect("edge").to_owned();
    let (out, inn, keep) = (id("out"), id("in"), id("keep"));

    let resp = edges(&backend, ALICE, json!({ "destroy": [keep] })).await;
    assert_eq!(resp["destroyed"], json!([keep]), "{resp}");
    let got = call!(
        handle_graph_node_get,
        &backend,
        ALICE,
        json!({ "ids": [b, c] })
    );
    assert_eq!(got["notFound"], json!([]), "{got}");

    let since = call!(
        handle_graph_edge_changes,
        &backend,
        ALICE,
        json!({ "sinceState": "0" })
    )["newState"]
        .clone();
    let resp = call!(
        handle_graph_node_set,
        &backend,
        ALICE,
        json!({ "destroy": [a] })
    );
    assert_eq!(resp["destroyed"], json!([a]), "{resp}");
    let changes = call!(
        handle_graph_edge_changes,
        &backend,
        ALICE,
        json!({ "sinceState": since })
    );
    let mut destroyed: Vec<String> =
        serde_json::from_value(changes["destroyed"].clone()).expect("ids");
    destroyed.sort();
    let mut want = vec![out, inn];
    want.sort();
    assert_eq!(destroyed, want, "{changes}");
}
