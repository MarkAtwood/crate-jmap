//! A misspelt FilterOperator key or FilterCondition property is
//! `unsupportedFilter`, never an empty, match-all condition (bd:JMAP-6azm).
//!
//! Oracle: RFC 8620 §5.5 — a FilterOperator object has the properties
//! `operator` and `conditions`, and "`unsupportedFilter`: The filter is
//! syntactically valid, but the server cannot process it." Condition
//! property names: draft-atwood-jmap-graph-00 §Graph/query
//! (`isSubscribed`), §GraphNode/query (`graphId`; `objectUri` from
//! draft-atwood-jmap-graph-projection-00) and §GraphEdge/query
//! (`graphId`, `type`). The GraphNode and GraphEdge
//! filters keep `graphId`, which those conditions require: without the
//! refusal each would decode as a condition on `graphId` alone and
//! succeed.

#[macro_use]
mod common;

use common::*;
use jmap_graph_server::{
    handle_graph_edge_query, handle_graph_edge_query_changes, handle_graph_node_query,
    handle_graph_node_query_changes, handle_graph_query, handle_graph_query_changes,
    handle_graph_set,
};
use serde_json::json;

#[tokio::test]
async fn graph_misspelt_operator_or_property_is_unsupported_filter() {
    let backend = backend();
    for filter in [
        json!({ "opperator": "AND", "conditions": [{ "isSubscribed": true }] }),
        json!({ "isSubscribd": true }),
    ] {
        let args = json!({ "filter": filter, "sinceQueryState": "0" });
        let t = call_err!(handle_graph_query, &backend, ALICE, args.clone());
        assert_eq!(t, "unsupportedFilter", "Graph/query {filter}");
        let t = call_err!(handle_graph_query_changes, &backend, ALICE, args);
        assert_eq!(t, "unsupportedFilter", "Graph/queryChanges {filter}");
    }
    // Negative control: the spec condition alone is accepted.
    call!(
        handle_graph_query,
        &backend,
        ALICE,
        json!({ "filter": { "isSubscribed": true } })
    );
}

#[tokio::test]
async fn node_and_edge_misspelt_key_is_unsupported_filter() {
    let backend = backend();
    let resp = call!(
        handle_graph_set,
        &backend,
        ALICE,
        json!({ "create": { "g": { "name": "g" } } })
    );
    let g = resp["created"]["g"]["id"].as_str().expect("graph");
    let node = [
        json!({ "opperator": "AND", "conditions": [], "graphId": g }),
        json!({ "graphId": g, "objectURI": ["jmap:x"] }),
    ];
    let edge = [
        json!({ "opperator": "AND", "conditions": [], "graphId": g }),
        json!({ "graphId": g, "edgeType": "knows" }),
    ];
    for filter in node {
        let args = json!({ "filter": filter, "sinceQueryState": "0" });
        let t = call_err!(handle_graph_node_query, &backend, ALICE, args.clone());
        assert_eq!(t, "unsupportedFilter", "GraphNode/query {filter}");
        let t = call_err!(handle_graph_node_query_changes, &backend, ALICE, args);
        assert_eq!(t, "unsupportedFilter", "GraphNode/queryChanges {filter}");
    }
    for filter in edge {
        let args = json!({ "filter": filter, "sinceQueryState": "0" });
        let t = call_err!(handle_graph_edge_query, &backend, ALICE, args.clone());
        assert_eq!(t, "unsupportedFilter", "GraphEdge/query {filter}");
        let t = call_err!(handle_graph_edge_query_changes, &backend, ALICE, args);
        assert_eq!(t, "unsupportedFilter", "GraphEdge/queryChanges {filter}");
    }
    // Negative control: `graphId` alone is accepted.
    let args = json!({ "filter": { "graphId": g } });
    call!(handle_graph_node_query, &backend, ALICE, args.clone());
    call!(handle_graph_edge_query, &backend, ALICE, args);
}
