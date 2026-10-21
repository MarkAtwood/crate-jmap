//! `register_graph_handlers` wires every method of draft-atwood-jmap-graph-00
//! §Methods (L236-330 at jmap-chat-spec 6bcd9d8): the standard `/get`,
//! `/changes`, `/set`, `/query` and `/queryChanges` of Graph, GraphNode and
//! GraphEdge.

mod common;

use std::sync::Arc;

use common::*;
use jmap_graph_server::{register_graph_edit_handlers, register_graph_handlers};
use jmap_graph_types::GraphEditCapability;
use jmap_server::{Dispatcher, JmapRequest, State};
use jmap_types::{Id, Invocation};
use serde_json::json;

#[tokio::test]
async fn registers_all_15_methods() {
    let mut dispatcher: Dispatcher<Id> = Dispatcher::new();
    register_graph_handlers(&mut dispatcher, Arc::new(backend()));

    for ty in ["Graph", "GraphNode", "GraphEdge"] {
        for (method, args) in [
            ("get", json!({ "accountId": ACCOUNT, "ids": [] })),
            (
                "changes",
                json!({ "accountId": ACCOUNT, "sinceState": "0" }),
            ),
            ("set", json!({ "accountId": ACCOUNT })),
            (
                "query",
                json!({ "accountId": ACCOUNT, "filter": { "graphId": "G1" } }),
            ),
        ] {
            let name = format!("{ty}/{method}");
            let mut args = args;
            if ty == "Graph" && method == "query" {
                args = json!({ "accountId": ACCOUNT });
            }
            let req = JmapRequest::new(
                vec!["urn:ietf:params:jmap:graph".into()],
                vec![Invocation::new(&name, args, "c0")],
                None,
            );
            let resp = dispatcher
                .dispatch(req, Id::from(ALICE), State::from("s0"))
                .await;
            let Invocation {
                name: resp_name,
                arguments,
                ..
            } = &resp.method_responses[0];
            assert_eq!(resp_name, &name, "{name}: {arguments}");
        }
        // `/queryChanges` is registered: the memory backend answers
        // `cannotCalculateChanges` (RFC 8620 §5.6), not `unknownMethod`.
        let name = format!("{ty}/queryChanges");
        let mut args = json!({ "accountId": ACCOUNT, "sinceQueryState": "0" });
        if ty != "Graph" {
            args["filter"] = json!({ "graphId": "G1" });
        }
        let req = JmapRequest::new(
            vec!["urn:ietf:params:jmap:graph".into()],
            vec![Invocation::new(&name, args, "c0")],
            None,
        );
        let resp = dispatcher
            .dispatch(req, Id::from(ALICE), State::from("s0"))
            .await;
        let args = &resp.method_responses[0].arguments;
        assert_eq!(args["type"], "cannotCalculateChanges", "{name}: {args}");
    }
}

/// `register_graph_edit_handlers` wires the draft-atwood-jmap-graph-edit-00
/// methods: GraphNodeOp `/get`, `/changes`, `/set`, `/query`,
/// `/queryChanges` (§Methods, L198-234 at jmap-chat-spec 6bcd9d8) and
/// `GraphNode/cursor` (§GraphNode/cursor, L334-347).
#[tokio::test]
async fn registers_the_6_edit_methods() {
    let backend = backend();
    let cap: GraphEditCapability = serde_json::from_value(json!({
        "maxOpSize": 64, "maxCursorSize": 64, "opRetention": null
    }))
    .expect("capability fixture");
    backend.register_edit(&Id::from(ACCOUNT), cap);
    let mut dispatcher: Dispatcher<Id> = Dispatcher::new();
    register_graph_edit_handlers(&mut dispatcher, Arc::new(backend));

    let filter = json!({ "nodeId": "N1" });
    for (name, args, response) in [
        ("GraphNodeOp/get", json!({ "ids": [] }), "GraphNodeOp/get"),
        (
            "GraphNodeOp/changes",
            json!({ "sinceState": "0" }),
            "GraphNodeOp/changes",
        ),
        ("GraphNodeOp/set", json!({}), "GraphNodeOp/set"),
        (
            "GraphNodeOp/query",
            json!({ "filter": filter }),
            "GraphNodeOp/query",
        ),
        // The memory backend answers `cannotCalculateChanges` (RFC 8620
        // §5.6), and an unknown node `invalidArguments` (L347): errors
        // from the handler, not `unknownMethod`.
        (
            "GraphNodeOp/queryChanges",
            json!({ "filter": filter, "sinceQueryState": "0" }),
            "error",
        ),
        (
            "GraphNode/cursor",
            json!({ "nodeId": "N1", "data": "AA" }),
            "error",
        ),
    ] {
        let mut args = args;
        args["accountId"] = json!(ACCOUNT);
        let req = JmapRequest::new(
            vec![
                "urn:ietf:params:jmap:graph".into(),
                "urn:ietf:params:jmap:graph:edit".into(),
            ],
            vec![Invocation::new(name, args, "c0")],
            None,
        );
        let resp = dispatcher
            .dispatch(req, Id::from(ALICE), State::from("s0"))
            .await;
        let got = &resp.method_responses[0];
        assert_eq!(got.name, response, "{name}: {}", got.arguments);
        assert_ne!(got.arguments["type"], "unknownMethod", "{name}");
    }
}
