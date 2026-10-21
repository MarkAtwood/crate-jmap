//! `register_graph_handlers` wires every method of draft-atwood-jmap-graph-00
//! §Methods (L236-330 at jmap-chat-spec 6bcd9d8): the standard `/get`,
//! `/changes`, `/set`, `/query` and `/queryChanges` of Graph, GraphNode and
//! GraphEdge.

mod common;

use std::sync::Arc;

use common::*;
use jmap_graph_server::register_graph_handlers;
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
