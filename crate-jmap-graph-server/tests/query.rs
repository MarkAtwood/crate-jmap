//! `Graph/query`, `GraphNode/query` and `GraphEdge/query`
//! (draft-atwood-jmap-graph-00 §Graph/query, §GraphNode/query,
//! §GraphEdge/query, §Traversal; L256-330, L396-432, L451 at jmap-chat-spec
//! 6bcd9d8). Each assertion quotes the draft text it checks.

#[macro_use]
mod common;

use common::*;
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::{
    handle_graph_edge_query, handle_graph_edge_query_changes, handle_graph_edge_set,
    handle_graph_node_query, handle_graph_node_set, handle_graph_query, handle_graph_set,
};
use serde_json::{json, Value};

async fn graph(backend: &MemoryBackend, name: &str) -> String {
    let resp = call!(
        handle_graph_set,
        backend,
        ALICE,
        json!({ "create": { "g": { "name": name } } })
    );
    resp["created"]["g"]["id"]
        .as_str()
        .expect("graph")
        .to_owned()
}

async fn node(backend: &MemoryBackend, graph: &str) -> String {
    let resp = call!(
        handle_graph_node_set,
        backend,
        ALICE,
        json!({ "create": { "n": { "graphId": graph } } })
    );
    resp["created"]["n"]["id"]
        .as_str()
        .expect("node")
        .to_owned()
}

async fn edge(backend: &MemoryBackend, graph: &str, from: &str, to: &str, ty: &str) -> String {
    let resp = call!(
        handle_graph_edge_set,
        backend,
        ALICE,
        json!({ "create": { "e": { "graphId": graph, "fromId": from, "toId": to, "type": ty } } })
    );
    resp["created"]["e"]["id"]
        .as_str()
        .expect("edge")
        .to_owned()
}

fn sorted(v: &Value) -> Vec<String> {
    let mut ids: Vec<String> = serde_json::from_value(v.clone()).expect("ids");
    ids.sort();
    ids
}

/// L285: "The `filter` argument MUST be a single FilterCondition, not a
/// FilterOperator, and MUST include `graphId`. A server receiving any other
/// filter, or no filter, rejects the call with an `unsupportedFilter`
/// error"; L314 applies the same restriction to GraphEdge/query.
#[tokio::test]
async fn node_and_edge_filters_need_graph_id() {
    let backend = backend();
    let g = graph(&backend, "g").await;
    for filter in [
        None,
        Some(Value::Null),
        Some(json!({})),
        Some(json!({ "operator": "AND", "conditions": [{ "graphId": g }] })),
    ] {
        let args = match &filter {
            Some(f) => json!({ "filter": f }),
            None => json!({}),
        };
        let t = call_err!(handle_graph_node_query, &backend, ALICE, args.clone());
        assert_eq!(t, "unsupportedFilter", "GraphNode/query filter {filter:?}");
        let t = call_err!(handle_graph_edge_query, &backend, ALICE, args.clone());
        assert_eq!(t, "unsupportedFilter", "GraphEdge/query filter {filter:?}");
        let mut args = args;
        args["sinceQueryState"] = json!("0");
        let t = call_err!(handle_graph_edge_query_changes, &backend, ALICE, args);
        assert_eq!(t, "unsupportedFilter", "GraphEdge/queryChanges {filter:?}");
    }
}

/// For an account without `urn:ietf:params:jmap:graph:search` the
/// FilterCondition properties draft-atwood-jmap-graph-search-00 adds
/// (L149-156 at jmap-chat-spec 6bcd9d8) are RFC 8620 §5.5
/// `unsupportedFilter` ("The "filter" is syntactically valid, but the
/// server cannot process it"), rather than ignored, which would match every
/// node or edge. `tests/search.rs` covers an account with search.
#[tokio::test]
async fn search_filter_properties_are_unsupported() {
    let backend = backend();
    let g = graph(&backend, "g").await;
    node(&backend, &g).await;
    for extra in [
        json!({ "where": {} }),
        json!({ "language": "opencypher", "expression": "MATCH (a) RETURN a" }),
        json!({ "expression": "x" }),
    ] {
        let mut filter = json!({ "graphId": g });
        filter
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let args = json!({ "filter": filter });
        let t = call_err!(handle_graph_node_query, &backend, ALICE, args.clone());
        assert_eq!(t, "unsupportedFilter", "GraphNode/query {filter}");
        let t = call_err!(handle_graph_edge_query, &backend, ALICE, args.clone());
        assert_eq!(t, "unsupportedFilter", "GraphEdge/query {filter}");
        let mut args = args;
        args["sinceQueryState"] = json!("0");
        let t = call_err!(handle_graph_edge_query_changes, &backend, ALICE, args);
        assert_eq!(t, "unsupportedFilter", "GraphEdge/queryChanges {filter}");
    }
}

/// L292, L330: "The following sort properties are supported: `created`,
/// `updated`"; L265 for Graph/query: "`name`, `created`, `updated`".
/// Anything else is RFC 8620 §5.5 `unsupportedSort`.
#[tokio::test]
async fn unsupported_sort() {
    let backend = backend();
    let g = graph(&backend, "g").await;
    let t = call_err!(
        handle_graph_node_query,
        &backend,
        ALICE,
        json!({ "filter": { "graphId": g }, "sort": [{ "property": "name" }] })
    );
    assert_eq!(t, "unsupportedSort");
    let t = call_err!(
        handle_graph_query,
        &backend,
        ALICE,
        json!({ "sort": [{ "property": "type" }] })
    );
    assert_eq!(t, "unsupportedSort");
}

/// The L396-430 traversal example: "This request finds every edge leaving
/// node `N1`"; L321-328: `fromId` "is one of these ids", `toId` likewise,
/// and `type` "equals this value exactly".
#[tokio::test]
async fn edge_filters() {
    let backend = backend();
    let g = graph(&backend, "g").await;
    let (n1, n2, n3) = (
        node(&backend, &g).await,
        node(&backend, &g).await,
        node(&backend, &g).await,
    );
    let e12 = edge(&backend, &g, &n1, &n2, "com.example.child").await;
    let e13 = edge(&backend, &g, &n1, &n3, "com.example.link").await;
    let e23 = edge(&backend, &g, &n2, &n3, "com.example.child").await;

    let query = |filter: Value| {
        let backend = &backend;
        async move {
            let resp = call!(
                handle_graph_edge_query,
                backend,
                ALICE,
                json!({ "filter": filter })
            );
            sorted(&resp["ids"])
        }
    };
    let mut want = vec![e12.clone(), e13.clone()];
    want.sort();
    assert_eq!(query(json!({ "graphId": g, "fromId": [n1] })).await, want);
    let mut want = vec![e13.clone(), e23.clone()];
    want.sort();
    assert_eq!(query(json!({ "graphId": g, "toId": [n3] })).await, want);
    assert_eq!(
        query(json!({ "graphId": g, "fromId": [n1, n2], "type": "com.example.child" })).await,
        {
            let mut w = vec![e12.clone(), e23.clone()];
            w.sort();
            w
        }
    );
    assert_eq!(
        query(json!({ "graphId": g, "type": "com.example" })).await,
        Vec::<String>::new(),
        "type matches exactly"
    );
}

/// L289: GraphNode/query `graphId`: "The node is in this Graph"; L451: a
/// Graph the user may not read is absent "from `/query` results".
#[tokio::test]
async fn node_query_by_graph_and_access() {
    let backend = backend();
    let g = graph(&backend, "g").await;
    let other = graph(&backend, "other").await;
    let (a, b) = (node(&backend, &g).await, node(&backend, &g).await);
    node(&backend, &other).await;

    let resp = call!(
        handle_graph_node_query,
        &backend,
        ALICE,
        json!({ "filter": { "graphId": g } })
    );
    let mut want = vec![a, b];
    want.sort();
    assert_eq!(sorted(&resp["ids"]), want, "{resp}");

    let resp = call!(
        handle_graph_node_query,
        &backend,
        BOB,
        json!({ "filter": { "graphId": g } })
    );
    assert_eq!(resp["ids"], json!([]), "{resp}");
    let resp = call!(handle_graph_query, &backend, BOB, json!({}));
    assert_eq!(resp["ids"], json!([]), "{resp}");
}

/// draft-atwood-jmap-graph-projection-00 L206: `objectUri` is "Always
/// `null` for nodes that are not in a projected Graph", so the L212-213
/// filter ("The node's `objectUri` is one of these values") matches none of
/// an ordinary Graph's nodes.
#[tokio::test]
async fn object_uri_filter_matches_no_unprojected_node() {
    let backend = backend();
    let g = graph(&backend, "g").await;
    node(&backend, &g).await;
    let resp = call!(
        handle_graph_node_query,
        &backend,
        ALICE,
        json!({ "filter": { "graphId": g, "objectUri": ["jmap:email:A1:M42"] } })
    );
    assert_eq!(resp["ids"], json!([]), "{resp}");
}

/// L262: Graph/query `isSubscribed`: "The Graph's `isSubscribed` property
/// equals this value"; L265 sort by `name`.
#[tokio::test]
async fn graph_query_is_subscribed_and_name_sort() {
    let backend = backend();
    let b = graph(&backend, "b").await;
    let a = graph(&backend, "a").await;
    let c = graph(&backend, "c").await;
    let resp = call!(
        handle_graph_set,
        &backend,
        ALICE,
        json!({ "update": { (c.clone()): { "isSubscribed": false } } })
    );
    assert!(resp["updated"].get(&c).is_some(), "{resp}");

    let resp = call!(
        handle_graph_query,
        &backend,
        ALICE,
        json!({ "filter": { "isSubscribed": true }, "sort": [{ "property": "name" }] })
    );
    assert_eq!(resp["ids"], json!([a, b]), "{resp}");
    let resp = call!(
        handle_graph_query,
        &backend,
        ALICE,
        json!({ "sort": [{ "property": "name", "isAscending": false }] })
    );
    assert_eq!(resp["ids"], json!([c, b, a]), "{resp}");
}
