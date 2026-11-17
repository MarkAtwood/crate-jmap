//! `#creationId` references in `FileNode/set` `parentId`, through the full
//! dispatcher (bd:JMAP-1p88).
//!
//! Oracle: RFC 8620 §5.3 (rfc8620.txt:1983-1996): "Some records may hold
//! references to other records (foreign keys). That reference may be set
//! (via create or update) in the same request as the referenced record is
//! created. To do this, the client refers to the new record using its
//! creation id prefixed with a "#". [...] the server MUST keep a simple map
//! for the duration of the request of creation id to record id for each
//! newly created record, so it can substitute in the correct value if
//! necessary in later method calls. In the case of records with
//! references to the same type, the server MUST order the creates and
//! updates within a single method call so that creates happen before their
//! creation ids are referenced by another create/update/destroy in the same
//! call."
//!
//! Each assertion compares `parentId` with the id the server reported for
//! the referenced FileNode, or with a hand-written SetError shape.

mod common;

use std::sync::Arc;

use jmap_types::Id;

use common::MemoryBackend;
use jmap_filenode_server::register_filenode_handlers;
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

fn dispatcher() -> Dispatcher<()> {
    let mut dispatcher = Dispatcher::new();
    register_filenode_handlers(
        &mut dispatcher,
        Arc::new(MemoryBackend::new().with_account(&Id::from(ACCOUNT))),
    );
    dispatcher
}

async fn run(dispatcher: &Dispatcher<()>, method_calls: Value) -> JmapResponse {
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:filenode"],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn created_id(resp: &JmapResponse, i: usize, key: &str) -> String {
    args(resp, i)["created"][key]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created {key}: {}", args(resp, i)))
        .to_owned()
}

fn dir(name: &str, parent: Value) -> Value {
    json!({ "@type": "Directory", "name": name, "parentId": parent })
}

async fn parent_of(d: &Dispatcher<()>, id: &str) -> Value {
    let resp = run(
        d,
        json!([["FileNode/get", { "accountId": ACCOUNT, "ids": [id],
            "properties": ["parentId"] }, "0"]]),
    )
    .await;
    args(&resp, 0)["list"][0]["parentId"].clone()
}

#[tokio::test]
async fn parent_id_resolves_earlier_call_creation() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["FileNode/set", { "accountId": ACCOUNT, "create": { "p": dir("docs", Value::Null) } }, "0"],
            ["FileNode/set", { "accountId": ACCOUNT, "create": { "c": dir("drafts", json!("#p")) } }, "1"]
        ]),
    )
    .await;
    let p = created_id(&resp, 0, "p");
    let c = created_id(&resp, 1, "c");
    assert_eq!(parent_of(&d, &c).await, json!(p));
}

#[tokio::test]
async fn parent_id_resolves_same_call_creation_listed_after_child() {
    // The child sorts before its parent in the create map; §5.3 still
    // requires the parent to be created first.
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["FileNode/set", { "accountId": ACCOUNT, "create": {
            "a-child": dir("drafts", json!("#z-parent")),
            "z-parent": dir("docs", Value::Null)
        } }, "0"]]),
    )
    .await;
    let parent = created_id(&resp, 0, "z-parent");
    let child = created_id(&resp, 0, "a-child");
    assert_eq!(parent_of(&d, &child).await, json!(parent));
}

#[tokio::test]
async fn parent_id_update_resolves_creation() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["FileNode/set", { "accountId": ACCOUNT,
            "create": { "m": dir("moved", Value::Null) } }, "0"]]),
    )
    .await;
    let moved = created_id(&resp, 0, "m");
    let resp = run(
        &d,
        json!([["FileNode/set", { "accountId": ACCOUNT,
            "create": { "n": dir("new-home", Value::Null) },
            "update": { (moved.clone()): { "parentId": "#n" } } }, "0"]]),
    )
    .await;
    let new_home = created_id(&resp, 0, "n");
    assert!(
        args(&resp, 0)["updated"].get(&moved).is_some(),
        "{}",
        args(&resp, 0)
    );
    assert_eq!(parent_of(&d, &moved).await, json!(new_home));
}

#[tokio::test]
async fn unknown_parent_reference_is_invalid_properties() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["FileNode/set", { "accountId": ACCOUNT,
            "create": { "c": dir("drafts", json!("#nope")) } }, "0"]]),
    )
    .await;
    let err = &args(&resp, 0)["notCreated"]["c"];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 0));
    assert_eq!(err["properties"], json!(["parentId"]), "{err}");
}

/// RFC 8620 §5.3: creates happen "before their creation ids are referenced
/// by another create/update/destroy in the same call", so update keys and
/// destroy ids may be creation ids, from an earlier call (`#a`, `#b`) or
/// this one (`#c`); the response names each by its real id
/// (bd:JMAP-tna1.16).
#[tokio::test]
async fn update_keys_and_destroy_ids_resolve_creations() {
    let obj = |name: &str| dir(name, Value::Null);
    let resp = run(
        &dispatcher(),
        json!([
            ["FileNode/set", {"accountId": ACCOUNT, "create": {"a": obj("a"), "b": obj("b")}}, "0"],
            ["FileNode/set", {"accountId": ACCOUNT,
                "create": {"c": obj("c")},
                "update": {"#a": {"name": "a2"}, "#c": {"name": "c2"}},
                "destroy": ["#b"]
            }, "1"]
        ]),
    )
    .await;
    let id = |i: usize, key: &str| {
        resp.method_responses[i].arguments["created"][key]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("created {key}: {:?}", resp.method_responses[i]))
            .to_owned()
    };
    let (a, b, c) = (id(0, "a"), id(0, "b"), id(1, "c"));
    let set = &resp.method_responses[1].arguments;
    assert!(set["updated"].get(&a).is_some(), "{set}");
    assert!(set["updated"].get(&c).is_some(), "{set}");
    assert_eq!(set["destroyed"], json!([b]), "{set}");
    assert!(set["notUpdated"].is_null(), "{set}");
    assert!(set["notDestroyed"].is_null(), "{set}");
}
