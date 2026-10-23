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
use serde_json::{Value, json};

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

/// RFC 8620 §5.3 types `destroy` as "Id[]|null", so an object element is
/// "invalidArguments: One of the arguments is of the wrong type" (§3.6.2).
/// The description names only the JSON type: §3.6.1 reserves it for
/// "unstructured English text suitable for displaying to a developer", so
/// the client-supplied value is never echoed (bd JMAP-zegc).
#[tokio::test]
async fn graph_set_non_string_destroy_element_does_not_echo_value() {
    let backend = backend();
    let mut args = json!({ "destroy": [{ "note": "CANARY-zegc" }] });
    args["accountId"] = json!(ACCOUNT);
    let err = handle_graph_set(&backend, &jmap_types::Id::from(ALICE), args)
        .await
        .expect_err("non-string destroy element must be a method error");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
    let desc = err.description.as_deref().unwrap_or_default();
    assert!(desc.contains("got object"), "{desc}");
    assert!(!desc.contains("CANARY"), "{desc}");
}
