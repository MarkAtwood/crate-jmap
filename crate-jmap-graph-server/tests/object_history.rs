//! Object history for Graph, GraphNode and GraphEdge through the
//! dispatcher and the memory backend (bd JMAP-tna1.3.19).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3 (a cascade counts as
//! destroyed), §5.1, §5.2, §8.2; draft-atwood-jmap-graph-00 (a Graph
//! destroy destroys its nodes and edges; a node destroy, its edges; an
//! edge's endpoints are nodes of its Graph); graph-edit-00 §Compaction and
//! docs/design/DECISIONS.md decision 22 (a restored node comes back with an
//! empty op log; GraphNodeOp keeps no history).

mod common;

use std::sync::Arc;

use common::{ACCOUNT, ALICE, BOB, backend};
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::{register_graph_handlers, register_graph_handlers_with_history};
use jmap_object_history_server::HistoryRetention;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const TYPES: [&str; 3] = ["Graph", "GraphNode", "GraphEdge"];

fn history_backend() -> MemoryBackend {
    let b = backend().with_object_history(
        serde_json::from_value(json!({"maxHistoryDuration": null, "supportedTypes": TYPES}))
            .unwrap(),
        HistoryRetention::new(),
    );
    b.set_history_clock_for_test(1_000);
    b
}

struct Server {
    b: MemoryBackend,
    d: Dispatcher<Id>,
}

impl Server {
    fn new() -> Self {
        let b = history_backend();
        let mut d = Dispatcher::new();
        register_graph_handlers_with_history(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    async fn call_as(&self, who: &str, method: &str, mut args: Value) -> Value {
        args["accountId"] = json!(ACCOUNT);
        let req: JmapRequest = serde_json::from_value(json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:graph",
                      "urn:ietf:params:jmap:object-history"],
            "methodCalls": [[method, args, "0"]]
        }))
        .unwrap();
        let resp =
            serde_json::to_value(self.d.dispatch(req, Id::from(who), State::from("s")).await)
                .unwrap();
        let r = resp["methodResponses"][0].clone();
        assert_ne!(r[0], "error", "{r}");
        r[1].clone()
    }

    async fn one(&self, method: &str, args: Value) -> Value {
        self.call_as(ALICE, method, args).await
    }

    async fn create(&self, method: &str, object: Value) -> String {
        let out = self.one(method, json!({"create": {"x": object}})).await;
        out["created"]["x"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{out}"))
            .to_owned()
    }
}

#[tokio::test]
async fn node_destroy_cascade_and_ordered_restores() {
    let s = Server::new();
    let g = s.create("Graph/set", json!({"name": "G"})).await;
    let a = s.create("GraphNode/set", json!({"graphId": &g})).await;
    let b = s.create("GraphNode/set", json!({"graphId": &g})).await;
    let e = s
        .create(
            "GraphEdge/set",
            json!({"graphId": &g, "fromId": &a, "toId": &b, "type": "com.example.link"}),
        )
        .await;
    s.b.set_history_clock_for_test(2_000);
    s.one("GraphNode/set", json!({"destroy": [&a]})).await;

    // The edge went with its node (§3: every route counts).
    let q = s
        .one("GraphEdge/query", json!({"onlyDestroyed": true}))
        .await;
    assert_eq!(q["ids"], json!([&e]), "{q}");

    // The edge cannot come back while its endpoint is destroyed...
    let out = s.one("GraphEdge/set", json!({"restore": {&e: null}})).await;
    assert_eq!(
        out["notRestored"][&e]["properties"],
        json!(["fromId"]),
        "{out}"
    );
    // ...and can once an earlier call restores the node.
    let out = s.one("GraphNode/set", json!({"restore": {&a: null}})).await;
    assert!(out["restored"].get(&a).is_some(), "{out}");
    let out = s.one("GraphEdge/set", json!({"restore": {&e: null}})).await;
    assert!(out["restored"].get(&e).is_some(), "{out}");
}

/// §8.2: another user who cannot read the Graph sees none of its history.
#[tokio::test]
async fn history_respects_current_read_access() {
    let s = Server::new();
    let g = s.create("Graph/set", json!({"name": "Private"})).await;
    s.b.set_history_clock_for_test(2_000);
    s.one("Graph/set", json!({"update": {&g: {"name": "Private 2"}}}))
        .await;
    let mine = s
        .one(
            "Graph/get",
            json!({"ids": [&g], "includeReplaced": true, "properties": ["name"]}),
        )
        .await;
    assert_eq!(mine["list"].as_array().unwrap().len(), 2, "{mine}");
    let bobs = s
        .call_as(
            BOB,
            "Graph/get",
            json!({"ids": [&g], "includeReplaced": true}),
        )
        .await;
    assert_eq!(bobs["list"], json!([]), "{bobs}");
    assert_eq!(bobs["notFound"], json!([&g]));
}

#[test]
fn registration_has_no_gaps() {
    let mut d: Dispatcher<Id> = Dispatcher::new();
    register_graph_handlers_with_history(&mut d, Arc::new(history_backend()));
    assert!(d.object_history_gaps(&TYPES).is_empty());
    let mut d: Dispatcher<Id> = Dispatcher::new();
    register_graph_handlers(&mut d, Arc::new(history_backend()));
    assert_eq!(d.object_history_gaps(&["GraphEdge"]).len(), 4);
}
