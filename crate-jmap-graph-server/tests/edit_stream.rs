//! draft-atwood-jmap-graph-edit-00 §WebSocket Stream: the
//! `GraphEditStreamEnable` subscription and the delivery check of
//! `GraphNodeOpEvent` and `GraphCursorEvent`. Line numbers cite
//! draft-atwood-jmap-graph-edit-00.md at jmap-chat-spec 6bcd9d8; events are
//! hand-written wire frames.

#[macro_use]
mod common;

use common::*;
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::{
    handle_graph_node_set, handle_graph_set, should_deliver, GraphEditSubscription,
};
use jmap_graph_types::{GraphEditEvent, GraphEditStreamMessage};
use jmap_types::Id;
use serde_json::{json, Value};

fn enable(frame: Value) -> GraphEditStreamMessage {
    serde_json::from_value(frame).expect("stream frame")
}

fn subscribe(frame: Value) -> GraphEditSubscription {
    let mut sub = GraphEditSubscription::default();
    let GraphEditStreamMessage::Enable(e) = enable(frame) else {
        panic!("not an enable frame");
    };
    sub.replace_with(&e).expect("valid enable");
    sub
}

fn event(frame: Value) -> GraphEditEvent {
    serde_json::from_value(frame).expect("event frame")
}

/// [`ALICE`]'s Graph shared with `share_with`, and one node in it.
async fn graph_and_node(backend: &MemoryBackend, share_with: Value) -> (String, String) {
    let resp = call!(
        handle_graph_set,
        backend,
        ALICE,
        json!({ "create": { "g": { "shareWith": share_with } } })
    );
    let g = resp["created"]["g"]["id"]
        .as_str()
        .expect("graph")
        .to_owned();
    let resp = call!(
        handle_graph_node_set,
        backend,
        ALICE,
        json!({ "create": { "n": { "graphId": g } } })
    );
    let n = resp["created"]["n"]["id"]
        .as_str()
        .expect("node")
        .to_owned();
    (g, n)
}

/// L313-324: a `GraphNodeOpEvent` "with all its properties".
fn op_event(graph: &str, node: &str, seq: u64, author: &str) -> GraphEditEvent {
    event(json!({
        "@type": "GraphNodeOpEvent",
        "accountId": ACCOUNT,
        "op": {
            "id": format!("P{seq}"), "graphId": graph, "nodeId": node, "seq": seq,
            "baseSeq": seq - 1, "data": "AAECAwQF",
            "created": "2026-10-20T17:00:00Z", "createdBy": author
        }
    }))
}

/// L351-366: a `GraphCursorEvent`.
fn cursor_event(node: &str, sender: &str) -> GraphEditEvent {
    event(json!({
        "@type": "GraphCursorEvent", "accountId": ACCOUNT, "nodeId": node,
        "senderId": sender, "data": "AAEC"
    }))
}

async fn delivers(
    backend: &MemoryBackend,
    to: &str,
    sub: &GraphEditSubscription,
    e: &GraphEditEvent,
) -> bool {
    should_deliver(backend, &Id::from(to), sub, e)
        .await
        .expect("backend")
}

/// L307: "If `dataTypes` is empty or contains no recognized value, the
/// server responds with a `RequestError` whose `type` is
/// `urn:ietf:params:jmap:error:invalidArguments` and leaves the current
/// subscription unchanged. Unrecognized values alongside recognized ones
/// are ignored."
#[test]
fn enable_data_types_rules() {
    let mut sub = subscribe(json!({
        "@type": "GraphEditStreamEnable", "nodeIds": ["N1"], "dataTypes": ["ops"]
    }));
    let before = sub.clone();
    for types in [json!([]), json!(["presence"])] {
        let GraphEditStreamMessage::Enable(e) = enable(json!({
            "@type": "GraphEditStreamEnable", "nodeIds": ["N2"], "dataTypes": types
        })) else {
            panic!("enable");
        };
        let err = sub.replace_with(&e).expect_err("must fail");
        assert_eq!(err.error_type.to_string(), "invalidArguments", "{types}");
        assert_eq!(sub, before, "{types}");
    }
    let sub = subscribe(json!({
        "@type": "GraphEditStreamEnable", "nodeIds": ["N1"],
        "dataTypes": ["cursors", "presence"]
    }));
    let f = sub.filter().expect("active");
    assert!(f.cursors() && !f.ops());
}

/// L307: "A new `GraphEditStreamEnable` replaces the previous subscription
/// in its entirety."
#[test]
fn enable_replaces_the_whole_subscription() {
    let mut sub = subscribe(json!({
        "@type": "GraphEditStreamEnable", "nodeIds": ["N1"], "dataTypes": ["ops", "cursors"]
    }));
    let GraphEditStreamMessage::Enable(e) = enable(json!({
        "@type": "GraphEditStreamEnable", "nodeIds": ["N2"], "dataTypes": ["ops"]
    })) else {
        panic!("enable");
    };
    sub.replace_with(&e).expect("valid");
    let f = sub.filter().expect("active");
    assert_eq!(f.node_ids(), [Id::from("N2")]);
    assert!(f.ops() && !f.cursors());
}

/// L315: an op event is sent "for each op sequenced on a subscribed node,
/// when `"ops"` is in `dataTypes`"; L326 it "includes the author's own
/// ops".
#[tokio::test]
async fn op_events_follow_the_subscription() {
    let backend = backend();
    let (g, n) = graph_and_node(&backend, Value::Null).await;
    let e = op_event(&g, &n, 1, ALICE);
    let ops = subscribe(json!({
        "@type": "GraphEditStreamEnable", "nodeIds": [n], "dataTypes": ["ops"]
    }));
    assert!(delivers(&backend, ALICE, &ops, &e).await);
    let cursors = subscribe(json!({
        "@type": "GraphEditStreamEnable", "nodeIds": [n], "dataTypes": ["cursors"]
    }));
    assert!(!delivers(&backend, ALICE, &cursors, &e).await);
    let other = subscribe(json!({
        "@type": "GraphEditStreamEnable", "nodeIds": ["other"], "dataTypes": ["ops"]
    }));
    assert!(!delivers(&backend, ALICE, &other, &e).await);
    // L311: GraphEditStreamDisable "ends the subscription".
    assert!(!delivers(&backend, ALICE, &GraphEditSubscription::default(), &e).await);
}

/// L307: "Ids in `nodeIds` that do not identify a node the user may read
/// produce no events but remain in the subscription; rights are evaluated
/// at each delivery". L328: "The server delivers an op event only to users
/// who have `mayRead` on the node's Graph at the time of delivery."
#[tokio::test]
async fn rights_are_evaluated_at_delivery() {
    let backend = backend();
    let (g, n) = graph_and_node(&backend, Value::Null).await;
    let sub = subscribe(json!({
        "@type": "GraphEditStreamEnable", "nodeIds": [n], "dataTypes": ["ops", "cursors"]
    }));
    let e = op_event(&g, &n, 1, ALICE);
    let c = cursor_event(&n, ALICE);
    assert!(!delivers(&backend, BOB, &sub, &e).await);
    assert!(!delivers(&backend, BOB, &sub, &c).await);

    let share = |r: Value| json!({ "update": { &g: { "shareWith": { BOB: r } } } });
    call!(
        handle_graph_set,
        &backend,
        ALICE,
        share(rights(true, false, false))
    );
    assert!(delivers(&backend, BOB, &sub, &e).await);
    assert!(delivers(&backend, BOB, &sub, &c).await);

    call!(
        handle_graph_set,
        &backend,
        ALICE,
        share(rights(false, false, false))
    );
    assert!(!delivers(&backend, BOB, &sub, &e).await);
}

/// L368: the server "SHOULD NOT deliver it to the sender's own
/// connections". L370: it "SHOULD NOT deliver a cursor event to a user who
/// has blocked the sender ... This applies only to cursor events. Ops are
/// shared document state ... so they are always delivered."
#[tokio::test]
async fn cursor_sender_and_block_rules() {
    let backend = backend();
    let (g, n) = graph_and_node(&backend, json!({ BOB: rights(true, true, false) })).await;
    let sub = subscribe(json!({
        "@type": "GraphEditStreamEnable", "nodeIds": [n], "dataTypes": ["ops", "cursors"]
    }));
    assert!(!delivers(&backend, ALICE, &sub, &cursor_event(&n, ALICE)).await);
    assert!(delivers(&backend, ALICE, &sub, &cursor_event(&n, BOB)).await);

    backend.block(&Id::from(ALICE), &Id::from(BOB));
    assert!(!delivers(&backend, ALICE, &sub, &cursor_event(&n, BOB)).await);
    assert!(delivers(&backend, ALICE, &sub, &op_event(&g, &n, 1, BOB)).await);
}
