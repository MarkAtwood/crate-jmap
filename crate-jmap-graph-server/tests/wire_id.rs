//! RFC 8620 §1.2 wire-id validation on `jmap-graph-server` `/set` handlers
//! (bd JMAP-k9va), mirroring the canonical
//! `crate-jmap-mail-server/tests/integration.rs`
//! `mailbox_set_and_query_reject_malformed_ids`.
//!
//! Oracle: RFC 8620 §1.2, an Id "MUST only contain characters from the
//! 'URL and Filename Safe' base64 alphabet". `"bad id"` contains a space,
//! so a `/set` `update` key or `destroy` element naming it fails that
//! entry with `invalidProperties` naming `id`, written out by hand below.

#[macro_use]
mod common;

use common::*;
use jmap_graph_server::{handle_graph_node_set, handle_graph_set};
use serde_json::{json, Value};

/// Asserts the hand-written RFC 8620 §5.3 SetError for a malformed id.
fn assert_invalid_id(entry: &Value, resp: &Value) {
    assert_eq!(entry["type"], "invalidProperties", "{resp}");
    assert_eq!(entry["properties"], json!(["id"]), "{resp}");
}

#[tokio::test]
async fn graph_set_rejects_malformed_update_and_destroy_ids() {
    let backend = backend();
    let args = json!({
        "update": { "bad id": { "name": "x" } },
        "destroy": ["bad id"]
    });
    let resp = call!(handle_graph_set, &backend, ALICE, args);
    assert_invalid_id(&resp["notUpdated"]["bad id"], &resp);
    assert_invalid_id(&resp["notDestroyed"]["bad id"], &resp);
}

/// draft-atwood-jmap-graph-00 §GraphNode/set: "On create, `graphId` is
/// required"; a malformed Id fails the create with `invalidProperties`
/// naming `graphId`.
#[tokio::test]
async fn graph_node_set_rejects_malformed_graph_id() {
    let backend = backend();
    let args = json!({ "create": { "n": { "graphId": "bad id" } } });
    let resp = call!(handle_graph_node_set, &backend, ALICE, args);
    let entry = &resp["notCreated"]["n"];
    assert_eq!(entry["type"], "invalidProperties", "{resp}");
    assert_eq!(entry["properties"], json!(["graphId"]), "{resp}");
}
