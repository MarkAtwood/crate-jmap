//! Conditional graph Foo/set (draft-ietf-jmap-conditional-02) through
//! `register_graph_conditional` and the memory backend's `ScopedState`
//! scopes (bd JMAP-y1yu.16; docs/design/conditional.md §5.1, §7, §9).
//!
//! Oracles: conditional-02 §3.1 and §4.2 ("the server MUST NOT make any of
//! the changes"); draft-atwood-jmap-graph-00 §GraphNode/set ("the server
//! MUST also destroy every GraphEdge whose `fromId` or `toId` is that
//! node") and the edit extension L182 ("The first op of a node has `seq`
//! 1, and each later op of that node has the previous op's `seq` plus one,
//! with no gaps").

mod common;

use std::sync::Arc;

use common::{ACCOUNT, ALICE, backend};
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::{
    register_graph_conditional, register_graph_edit_handlers, register_graph_handlers,
};
use jmap_graph_types::GraphEditCapability;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:graph",
    "urn:ietf:params:jmap:graph:edit",
    "urn:ietf:params:jmap:conditional",
];

/// A backend with the edit capability (`maxOpSize` 8).
fn edit_backend() -> Arc<MemoryBackend> {
    let b = backend();
    let cap: GraphEditCapability = serde_json::from_value(json!({
        "maxOpSize": 8, "maxCursorSize": 4, "opRetention": null
    }))
    .unwrap();
    b.register_edit(&Id::from(ACCOUNT), cap);
    Arc::new(b)
}

fn dispatcher(b: &Arc<MemoryBackend>) -> Dispatcher<Id> {
    let mut d = Dispatcher::new();
    register_graph_handlers(&mut d, Arc::clone(b));
    register_graph_edit_handlers(&mut d, Arc::clone(b));
    register_graph_conditional(&mut d, Arc::clone(b));
    d
}

async fn call(d: &Dispatcher<Id>, method: &str, mut args: Value) -> (String, Value) {
    args["accountId"] = json!(ACCOUNT);
    let req: JmapRequest =
        serde_json::from_value(json!({"using": USING, "methodCalls": [[method, args, "0"]]}))
            .unwrap();
    let resp =
        serde_json::to_value(d.dispatch(req, Id::from(ALICE), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

/// A Graph with nodes `a` and `b` (editFormat set) and an edge a → b; the
/// ids `(graph, a, b, edge)`.
async fn seeded(d: &Dispatcher<Id>) -> (String, String, String, String) {
    let (_, r) = call(
        d,
        "Graph/set",
        json!({"create": {"g": {"shareWith": null}}}),
    )
    .await;
    let g = r["created"]["g"]["id"].as_str().expect("graph").to_owned();
    let (_, r) = call(
        d,
        "GraphNode/set",
        json!({"create": {
            "a": {"graphId": g, "editFormat": "application/x-example-ops"},
            "b": {"graphId": g}
        }}),
    )
    .await;
    let id = |k: &str| r["created"][k]["id"].as_str().expect("node").to_owned();
    let (a, b) = (id("a"), id("b"));
    let (_, r) = call(
        d,
        "GraphEdge/set",
        json!({"create": {"e": {"graphId": g, "fromId": a, "toId": b, "type": "com.example.child"}}}),
    )
    .await;
    let e = r["created"]["e"]["id"].as_str().expect("edge").to_owned();
    (g, a, b, e)
}

async fn get(d: &Dispatcher<Id>, method: &str) -> Value {
    let (name, r) = call(d, method, json!({})).await;
    assert_eq!(name, method, "{r}");
    r
}

#[tokio::test]
async fn precondition_on_a_node() {
    let b = edit_backend();
    let d = dispatcher(&b);
    let (_, a, _, _) = seeded(&d).await;
    let set_if = |format: &str| json!({"ifUnchangedBy": {&a: {"editFormat": format}}, "update": {&a: {"label": "A"}}});
    let (name, r) = call(&d, "GraphNode/set", set_if("text/plain")).await;
    assert_eq!(
        (name.as_str(), r),
        (
            "error",
            json!({"type": "stateMismatch", "failed": [a.clone()]})
        )
    );
    let (name, r) = call(&d, "GraphNode/set", set_if("application/x-example-ops")).await;
    assert_eq!(name, "GraphNode/set", "{r}");
}

/// A node destroy's edge cascade rolls back with an atomic failure; when
/// it commits, the edge goes with the node, so no edge is left dangling.
#[tokio::test]
async fn atomic_node_destroy_cascade() {
    let b = edit_backend();
    let d = dispatcher(&b);
    let (_, a, _, e) = seeded(&d).await;
    let nodes = get(&d, "GraphNode/get").await;
    let edges = get(&d, "GraphEdge/get").await;

    let (name, r) = call(
        &d,
        "GraphNode/set",
        json!({"atomic": true, "destroy": [&a], "update": {"nope": {"label": "x"}}}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert_eq!(get(&d, "GraphNode/get").await, nodes);
    assert_eq!(get(&d, "GraphEdge/get").await, edges);

    let (name, r) = call(
        &d,
        "GraphNode/set",
        json!({"atomic": true, "destroy": [&a]}),
    )
    .await;
    assert_eq!(name, "GraphNode/set", "{r}");
    let (_, r) = call(&d, "GraphEdge/get", json!({"ids": [&e]})).await;
    assert_eq!(r["notFound"], json!([e]), "the edge went with its node");
}

/// A rolled-back GraphNodeOp/set leaves the node's `seq` where it was: the
/// next op is still `seq` 1.
#[tokio::test]
async fn atomic_op_failure_keeps_the_seq() {
    let b = edit_backend();
    let d = dispatcher(&b);
    let (_, a, _, _) = seeded(&d).await;
    let op = |base: u64, data: &str| json!({"nodeId": &a, "baseSeq": base, "data": data});

    let (name, r) = call(
        &d,
        "GraphNodeOp/set",
        json!({"atomic": true, "create": {
            "ok": op(0, "AAECAwQF"),
            "big": op(0, "AAECAwQFBgcICQoLDA0ODw==")
        }}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert!(r["notCreated"].get("big").is_some(), "{r}");

    let (_, r) = call(
        &d,
        "GraphNodeOp/set",
        json!({"create": {"o": op(0, "AAECAwQF")}}),
    )
    .await;
    assert_eq!(r["created"]["o"]["seq"], 1, "{r}");
}

#[test]
fn ready_for_each_registration() {
    let b = edit_backend();

    let mut d: Dispatcher<Id> = Dispatcher::new();
    register_graph_handlers(&mut d, Arc::clone(&b));
    register_graph_conditional(&mut d, Arc::clone(&b));
    assert!(d.conditional_ready(), "{:?}", d.conditional_gaps());

    // The edit handlers add GraphNodeOp/set: a gap until the opt-in runs
    // again.
    register_graph_edit_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps(), ["GraphNodeOp/set"]);
    register_graph_conditional(&mut d, Arc::clone(&b));
    assert!(d.conditional_ready());

    // Edit handlers alone.
    let mut d: Dispatcher<Id> = Dispatcher::new();
    register_graph_edit_handlers(&mut d, Arc::clone(&b));
    register_graph_conditional(&mut d, b);
    assert!(
        d.conditional_gaps().is_empty(),
        "{:?}",
        d.conditional_gaps()
    );
}
