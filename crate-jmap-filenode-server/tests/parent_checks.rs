//! A FileNode's parent must exist and permit children, on create and on
//! update (bd JMAP-hy30).
//!
//! Oracle: draft-ietf-jmap-filenode-20 §3.1.1 parentId: "The parent MUST
//! be a node whose @type permits children; of the types defined in this
//! document, only "Directory" does. Setting parentId to any other node is
//! an "invalidProperties" error."

use std::sync::Arc;

use jmap_filenode_server::memory::MemoryBackend;
use jmap_filenode_server::register_filenode_handlers;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

/// `d` a Directory and `f` a File, both top level.
async fn dispatcher() -> (Dispatcher<()>, String, String) {
    let b = Arc::new(MemoryBackend::new().with_account(&Id::from("u1")));
    let mut d = Dispatcher::new();
    register_filenode_handlers(&mut d, b);
    let r = set(
        &d,
        json!({"create": {
            "d": {"@type": "Directory", "name": "docs", "parentId": null},
            "f": {"@type": "File", "name": "a.txt", "parentId": null, "blobId": "G1"}
        }}),
    )
    .await;
    let id = |k: &str| r["created"][k]["id"].as_str().expect("seeded").to_owned();
    let (dir, file) = (id("d"), id("f"));
    (d, dir, file)
}

async fn set(d: &Dispatcher<()>, mut args: Value) -> Value {
    args["accountId"] = json!("u1");
    let req: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:filenode"],
        "methodCalls": [["FileNode/set", args, "0"]]
    }))
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    resp["methodResponses"][0][1].clone()
}

fn assert_parent_error(err: &Value) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["parentId"]), "{err}");
}

#[tokio::test]
async fn create_under_a_missing_or_childless_parent() {
    let (d, _, file) = dispatcher().await;
    let r = set(
        &d,
        json!({"create": {
            "a": {"@type": "Directory", "name": "x", "parentId": "nope"},
            "b": {"@type": "Directory", "name": "y", "parentId": &file}
        }}),
    )
    .await;
    assert_parent_error(&r["notCreated"]["a"]);
    assert_parent_error(&r["notCreated"]["b"]);
}

#[tokio::test]
async fn move_under_a_missing_or_childless_parent() {
    let (d, dir, file) = dispatcher().await;
    let r = set(&d, json!({"update": {&dir: {"parentId": "nope"}}})).await;
    assert_parent_error(&r["notUpdated"][&dir]);
    let r = set(&d, json!({"update": {&dir: {"parentId": &file}}})).await;
    assert_parent_error(&r["notUpdated"][&dir]);
}

#[tokio::test]
async fn a_directory_parent_is_fine() {
    let (d, dir, file) = dispatcher().await;
    let r = set(
        &d,
        json!({
            "create": {"c": {"@type": "Directory", "name": "child", "parentId": &dir}},
            "update": {&file: {"parentId": &dir}}
        }),
    )
    .await;
    assert!(r["created"].get("c").is_some(), "{r}");
    assert!(r["updated"].get(&file).is_some(), "{r}");
}
