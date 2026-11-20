//! A `MemoryBackend<()>`: a deployment without identity, whose every
//! caller acts as the account owner (`jmap_server::MemoryCaller`). The
//! owner "implicitly has all rights" and is the only user who may create a
//! Graph (draft-atwood-jmap-graph-00 L173 at jmap-chat-spec 6bcd9d8).

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNT, ALICE, rights};
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::{GraphBackend, handle_graph_get, handle_graph_node_set, handle_graph_set};
use jmap_graph_types::GraphCapability;
use jmap_server::JmapAccounts;
use jmap_types::Id;
use serde_json::{Value, json};

fn backend() -> MemoryBackend<()> {
    let backend = MemoryBackend::<()>::default();
    let cap: GraphCapability =
        serde_json::from_value(json!({ "maxAnnotationSize": null })).expect("capability fixture");
    backend.register_account(&Id::from(ACCOUNT), &Id::from(ALICE), cap);
    backend
}

fn args(rest: Value) -> Value {
    let mut args = rest;
    args["accountId"] = json!(ACCOUNT);
    args
}

#[tokio::test]
async fn caller_without_identity_acts_as_the_owner() {
    let backend = backend();
    assert_eq!(MemoryBackend::<()>::principal_id(&()), None);

    let resp = handle_graph_set(&backend, &(), args(json!({ "create": { "g": {} } })))
        .await
        .expect("Graph/set")
        .primary;
    let created = &resp["created"]["g"];
    assert_eq!(created["myRights"], rights(true, true, true), "{resp}");
    assert_eq!(created["isSubscribed"], json!(true), "{resp}");
    let g = created["id"].as_str().expect("graph id").to_owned();

    let resp = handle_graph_node_set(
        &backend,
        &(),
        &BTreeMap::new(),
        args(json!({ "create": { "n": { "graphId": g } } })),
    )
    .await
    .expect("GraphNode/set")
    .primary;
    // `createdBy` comes from `principal_id`: "`null` if the server does
    // not support Principals or cannot determine it", as here.
    assert!(resp["created"]["n"]["id"].is_string(), "{resp}");
    assert_eq!(resp["created"]["n"]["createdBy"], Value::Null, "{resp}");

    let resp = handle_graph_get(&backend, &(), args(json!({ "ids": [g] })))
        .await
        .expect("Graph/get")
        .primary;
    assert_eq!(
        resp["list"][0]["myRights"],
        rights(true, true, true),
        "{resp}"
    );

    // A caller without identity has blocked no one.
    backend.block(&Id::from(ALICE), &Id::from("bob"));
    assert!(
        !backend
            .is_sender_blocked(&(), &Id::from("bob"))
            .await
            .expect("is_sender_blocked")
    );
}
