//! `Graph/set`, `/get` and `/changes` (draft-atwood-jmap-graph-00 §Graph,
//! §Graph/set, §Access Control; L134-173, L246-254, L449-451 at
//! jmap-chat-spec 6bcd9d8). Each assertion quotes the draft text it checks.

#[macro_use]
mod common;

use common::*;
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::{
    handle_graph_edge_changes, handle_graph_get, handle_graph_node_changes, handle_graph_set,
};
use serde_json::{Value, json};

async fn set(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(handle_graph_set, backend, caller, args)
}

/// The id of the L365 Graph `{ "name": "Design notes" }` created by
/// [`ALICE`], shared with `share_with`.
async fn design_notes(backend: &MemoryBackend, share_with: Value) -> String {
    let resp = set(
        backend,
        ALICE,
        json!({ "create": { "g": { "name": "Design notes", "shareWith": share_with } } }),
    )
    .await;
    resp["created"]["g"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("create must succeed: {resp}"))
        .to_owned()
}

async fn get_one(backend: &MemoryBackend, caller: &str, id: &str) -> Value {
    call!(handle_graph_get, backend, caller, json!({ "ids": [id] }))
}

/// L138-160: `id`, `created`, `updated` and `myRights` are server-set,
/// `name` defaults to `""`, `annotations` to `{}`, `shareWith` to `null`,
/// and `isSubscribed` to "`true` for the user who creates the Graph"; L173
/// "The owner ... implicitly has all rights".
#[tokio::test]
async fn create_echoes_server_set_and_defaults() {
    let backend = backend();
    let resp = set(&backend, ALICE, json!({ "create": { "g": {} } })).await;
    let created = &resp["created"]["g"];
    assert!(created["id"].is_string(), "{resp}");
    assert!(created["created"].is_string(), "{resp}");
    assert!(created["updated"].is_string(), "{resp}");
    assert_eq!(created["myRights"], rights(true, true, true), "{resp}");
    assert_eq!(created["name"], json!(""), "{resp}");
    assert_eq!(created["annotations"], json!({}), "{resp}");
    assert_eq!(created["isSubscribed"], json!(true), "{resp}");
    assert_eq!(created["shareWith"], Value::Null, "{resp}");
}

/// L138, L147, L156: server-set properties cannot be set on create.
#[tokio::test]
async fn create_refuses_server_set_properties() {
    let backend = backend();
    let resp = set(
        &backend,
        ALICE,
        json!({ "create": { "g": { "myRights": rights(true, true, true), "created": "2026-01-01T00:00:00Z" } } }),
    )
    .await;
    let err = &resp["notCreated"]["g"];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(err["properties"], json!(["created", "myRights"]), "{resp}");
}

/// L124: "Annotation keys are non-empty strings".
#[tokio::test]
async fn empty_annotation_key_is_invalid() {
    let backend = backend();
    let resp = set(
        &backend,
        ALICE,
        json!({ "create": { "g": { "annotations": { "": 1 } } } }),
    )
    .await;
    assert_eq!(resp["notCreated"]["g"]["type"], "invalidProperties");
    assert_eq!(
        resp["notCreated"]["g"]["properties"],
        json!(["annotations"])
    );
}

/// L108: "A create or update that would exceed the limit is rejected with
/// a `tooLarge` SetError". `{"k":"0123456789"}` is 18 octets of UTF-8 JSON
/// (`{` 1, `"k"` 3, `:` 1, `"0123456789"` 12, `}` 1).
#[tokio::test]
async fn max_annotation_size_is_too_large() {
    let backend = backend_with(json!(17));
    let resp = set(
        &backend,
        ALICE,
        json!({ "create": { "g": { "annotations": { "k": "0123456789" } } } }),
    )
    .await;
    assert_eq!(resp["notCreated"]["g"]["type"], "tooLarge", "{resp}");

    let backend = backend_with(json!(18));
    let resp = set(
        &backend,
        ALICE,
        json!({ "create": { "g": { "annotations": { "k": "0123456789" } } } }),
    )
    .await;
    assert!(resp["created"]["g"].is_object(), "18 octets fit: {resp}");
}

/// L451: "A Graph the user may not read SHOULD be indistinguishable from
/// one that does not exist: `notFound` on `/get` and on `/set` update or
/// destroy"; L250: "a user without `mayRead` receives `notFound`".
#[tokio::test]
async fn unreadable_graph_is_not_found() {
    let backend = backend();
    let g = design_notes(&backend, Value::Null).await;

    let resp = get_one(&backend, BOB, &g).await;
    assert_eq!(resp["list"], json!([]), "{resp}");
    assert_eq!(resp["notFound"], json!([g]), "{resp}");

    let resp = set(
        &backend,
        BOB,
        json!({ "update": { (g.clone()): { "isSubscribed": true } }, "destroy": [g] }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&g]["type"], "notFound", "{resp}");
    assert_eq!(resp["notDestroyed"][&g]["type"], "notFound", "{resp}");
}

/// L250: "Any user with `mayRead` may change their own `isSubscribed`
/// value. Changing `name` or `annotations` requires `mayWrite`. Changing
/// `shareWith` ... requires `mayAdmin`. Destroying a Graph requires
/// `mayAdmin`. A user who has `mayRead` but lacks the needed right receives
/// a `forbidden` SetError".
#[tokio::test]
async fn reader_rights() {
    let backend = backend();
    let g = design_notes(&backend, json!({ BOB: rights(true, false, false) })).await;

    let resp = set(
        &backend,
        BOB,
        json!({ "update": { (g.clone()): { "name": "x" } } }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&g]["type"], "forbidden", "{resp}");
    let resp = set(
        &backend,
        BOB,
        json!({ "update": { (g.clone()): { "annotations/com.example.label": "x" } } }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&g]["type"], "forbidden", "{resp}");
    let resp = set(
        &backend,
        BOB,
        json!({ "update": { (g.clone()): { "shareWith": { CAROL: rights(true, false, false) } } } }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&g]["type"], "forbidden", "{resp}");
    let resp = set(&backend, BOB, json!({ "destroy": [g] })).await;
    assert_eq!(resp["notDestroyed"][&g]["type"], "forbidden", "{resp}");

    let resp = set(
        &backend,
        BOB,
        json!({ "update": { (g.clone()): { "isSubscribed": true } } }),
    )
    .await;
    assert!(resp["updated"].get(&g).is_some(), "{resp}");
    // "their own `isSubscribed` value": Bob's change is not Alice's.
    assert_eq!(
        get_one(&backend, BOB, &g).await["list"][0]["isSubscribed"],
        true
    );
    let mine = get_one(&backend, ALICE, &g).await;
    assert_eq!(mine["list"][0]["isSubscribed"], true, "{mine}");
    let resp = set(
        &backend,
        BOB,
        json!({ "update": { (g.clone()): { "isSubscribed": false } } }),
    )
    .await;
    assert!(resp["updated"].get(&g).is_some(), "{resp}");
    assert_eq!(
        get_one(&backend, ALICE, &g).await["list"][0]["isSubscribed"],
        true
    );
}

/// L167-171: `mayWrite` "may change the Graph's `name` and `annotations`";
/// `mayAdmin` "may change the Graph's `shareWith` property and may destroy
/// the Graph". L157: `myRights` is "The rights the user has on this Graph".
#[tokio::test]
async fn writer_and_admin_rights() {
    let backend = backend();
    let g = design_notes(
        &backend,
        json!({ BOB: rights(true, true, false), CAROL: rights(true, false, true) }),
    )
    .await;
    assert_eq!(
        get_one(&backend, BOB, &g).await["list"][0]["myRights"],
        rights(true, true, false)
    );

    let resp = set(
        &backend,
        BOB,
        json!({ "update": { (g.clone()): { "name": "x" } } }),
    )
    .await;
    assert!(resp["updated"].get(&g).is_some(), "{resp}");
    let resp = set(&backend, BOB, json!({ "destroy": [g.clone()] })).await;
    assert_eq!(resp["notDestroyed"][&g]["type"], "forbidden", "{resp}");

    let resp = set(
        &backend,
        CAROL,
        json!({ "update": { (g.clone()): { "shareWith/bob": null } } }),
    )
    .await;
    assert!(resp["updated"].get(&g).is_some(), "{resp}");
    assert_eq!(get_one(&backend, BOB, &g).await["notFound"], json!([g]));
    let resp = set(&backend, CAROL, json!({ "destroy": [g.clone()] })).await;
    assert_eq!(resp["destroyed"], json!([g]), "{resp}");
}

/// L128 and the L436-445 example: a patch of `annotations/<key>` "changes
/// only the keys it names".
#[tokio::test]
async fn annotation_patch_changes_one_key() {
    let backend = backend();
    let resp = set(
        &backend,
        ALICE,
        json!({ "create": { "g": { "annotations": { "com.example.label": "Root" } } } }),
    )
    .await;
    let g = resp["created"]["g"]["id"].as_str().expect("id").to_owned();
    let resp = set(
        &backend,
        ALICE,
        json!({ "update": { (g.clone()): { "annotations/com.example.color": "red" } } }),
    )
    .await;
    assert!(resp["updated"].get(&g).is_some(), "{resp}");
    assert_eq!(
        get_one(&backend, ALICE, &g).await["list"][0]["annotations"],
        json!({ "com.example.color": "red", "com.example.label": "Root" })
    );
}

/// L254: "When a Graph is destroyed, the server MUST also destroy every
/// GraphNode and GraphEdge in it. These objects are reported as destroyed
/// by subsequent `GraphNode/changes` and `GraphEdge/changes` calls."
#[tokio::test]
async fn destroy_cascades_to_nodes_and_edges() {
    use jmap_graph_server::{handle_graph_edge_set, handle_graph_node_set};

    let backend = backend();
    let g = design_notes(&backend, Value::Null).await;
    let resp = call!(ids handle_graph_node_set,
        &backend,
        ALICE,
        json!({ "create": { "a": { "graphId": g }, "b": { "graphId": g } } })
    );
    let (a, b) = (
        resp["created"]["a"]["id"].as_str().expect("a").to_owned(),
        resp["created"]["b"]["id"].as_str().expect("b").to_owned(),
    );
    let resp = call!(ids handle_graph_edge_set,
        &backend,
        ALICE,
        json!({ "create": { "e": { "graphId": g, "fromId": a, "toId": b } } })
    );
    let e = resp["created"]["e"]["id"].as_str().expect("e").to_owned();

    let node_state = call!(
        handle_graph_node_changes,
        &backend,
        ALICE,
        json!({ "sinceState": "0" })
    )["newState"]
        .clone();
    let edge_state = call!(
        handle_graph_edge_changes,
        &backend,
        ALICE,
        json!({ "sinceState": "0" })
    )["newState"]
        .clone();

    let resp = set(&backend, ALICE, json!({ "destroy": [g] })).await;
    assert_eq!(resp["destroyed"], json!([g]), "{resp}");

    let nodes = call!(
        handle_graph_node_changes,
        &backend,
        ALICE,
        json!({ "sinceState": node_state })
    );
    let mut destroyed: Vec<String> =
        serde_json::from_value(nodes["destroyed"].clone()).expect("ids");
    destroyed.sort();
    let mut want = vec![a, b];
    want.sort();
    assert_eq!(destroyed, want, "{nodes}");
    let edges = call!(
        handle_graph_edge_changes,
        &backend,
        ALICE,
        json!({ "sinceState": edge_state })
    );
    assert_eq!(edges["destroyed"], json!([e]), "{edges}");
}

/// L151: `updated` is "When a property of the Graph object itself was last
/// changed. Changes to the Graph's nodes and edges do not change this
/// value."
#[tokio::test]
async fn node_changes_leave_graph_updated() {
    use jmap_graph_server::handle_graph_node_set;

    let backend = backend();
    let g = design_notes(&backend, Value::Null).await;
    let before = get_one(&backend, ALICE, &g).await["list"][0]["updated"].clone();
    let resp = call!(ids handle_graph_node_set,
        &backend,
        ALICE,
        json!({ "create": { "a": { "graphId": g } } })
    );
    assert!(resp["created"]["a"].is_object(), "{resp}");
    assert_eq!(
        get_one(&backend, ALICE, &g).await["list"][0]["updated"],
        before
    );
}
