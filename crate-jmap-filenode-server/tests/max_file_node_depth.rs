//! `maxFileNodeDepth` enforcement in `FileNode/set` and `FileNode/copy`,
//! through the full dispatcher (bd:JMAP-t8ej.14).
//!
//! Oracle: draft-ietf-jmap-filenode-19 §Capability: "maxFileNodeDepth:
//! "UnsignedInt|null" The maximum depth of the FileNode hierarchy (that is,
//! one more than the maximum number of ancestors a FileNode may have), or
//! null for no limit." With a limit of 3 a node may have at most 2
//! ancestors: a top-level node has depth 1, its child depth 2, its
//! grandchild depth 3.
//!
//! The draft names no error. The expected SetError is the hand-written
//! `invalidProperties` naming `parentId` proposed in
//! brong/draft-gondwana-jmap-filenode#16.

mod common;

use std::sync::Arc;

use jmap_types::Id;

use common::MemoryBackend;
use jmap_filenode_server::register_filenode_handlers;
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

fn dispatcher(backend: MemoryBackend) -> Dispatcher<()> {
    let mut dispatcher = Dispatcher::new();
    register_filenode_handlers(&mut dispatcher, Arc::new(backend));
    dispatcher
}

fn limited(max: u64) -> Dispatcher<()> {
    dispatcher(
        MemoryBackend::new()
            .with_account(&Id::from(ACCOUNT))
            .with_max_file_node_depth(max),
    )
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

fn dir(name: &str, parent: Value) -> Value {
    json!({ "@type": "Directory", "name": name, "parentId": parent })
}

/// Create the given `(creation id, directory)` entries in one call and
/// return the server id of each, panicking if any create failed.
async fn create_all(d: &Dispatcher<()>, entries: &[(&str, Value)]) -> Vec<String> {
    let create: serde_json::Map<String, Value> = entries
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();
    let resp = run(
        d,
        json!([["FileNode/set", { "accountId": ACCOUNT, "create": create }, "0"]]),
    )
    .await;
    entries
        .iter()
        .map(|(k, _)| {
            args(&resp, 0)["created"][*k]["id"]
                .as_str()
                .unwrap_or_else(|| panic!("create {k}: {}", args(&resp, 0)))
                .to_owned()
        })
        .collect()
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

fn depth_error() -> Value {
    json!({
        "type": "invalidProperties",
        "properties": ["parentId"],
        "description": "the node would exceed maxFileNodeDepth"
    })
}

#[tokio::test]
async fn create_at_limit_succeeds_and_one_deeper_is_rejected() {
    let d = limited(3);
    let resp = run(
        &d,
        json!([["FileNode/set", { "accountId": ACCOUNT, "create": {
            "a": dir("a", Value::Null),
            "b": dir("b", json!("#a")),
            "c": dir("c", json!("#b")),
            "d": dir("d", json!("#c")),
        } }, "0"]]),
    )
    .await;
    let set = args(&resp, 0);
    for key in ["a", "b", "c"] {
        assert!(
            set["created"][key]["id"].is_string(),
            "{key} at depth <= 3: {set}"
        );
    }
    assert_eq!(set["notCreated"]["d"], depth_error(), "{set}");
}

#[tokio::test]
async fn move_that_pushes_subtree_past_limit_is_rejected() {
    let d = limited(3);
    let ids = create_all(
        &d,
        &[
            ("a", dir("a", Value::Null)),
            ("b", dir("b", json!("#a"))),
            ("x", dir("x", Value::Null)),
            ("y", dir("y", json!("#x"))),
        ],
    )
    .await;
    let (a, b, x) = (&ids[0], &ids[1], &ids[2]);

    // x under b: x at depth 3, its child y at depth 4.
    let resp = run(
        &d,
        json!([["FileNode/set", { "accountId": ACCOUNT,
            "update": { x: { "parentId": b } } }, "0"]]),
    )
    .await;
    assert_eq!(
        args(&resp, 0)["notUpdated"][x],
        depth_error(),
        "{}",
        args(&resp, 0)
    );
    assert_eq!(
        parent_of(&d, x).await,
        Value::Null,
        "rejected move must not apply"
    );

    // x under a: x at depth 2, y at depth 3.
    let resp = run(
        &d,
        json!([["FileNode/set", { "accountId": ACCOUNT,
            "update": { x: { "parentId": a } } }, "0"]]),
    )
    .await;
    assert!(
        args(&resp, 0)["updated"].get(x).is_some(),
        "move within the limit: {}",
        args(&resp, 0)
    );
    assert_eq!(parent_of(&d, x).await, json!(a));
}

#[tokio::test]
async fn copy_past_limit_is_rejected() {
    let d = limited(3);
    let ids = create_all(
        &d,
        &[
            ("a", dir("a", Value::Null)),
            ("b", dir("b", json!("#a"))),
            ("c", dir("c", json!("#b"))),
            ("f", dir("f", Value::Null)),
        ],
    )
    .await;
    let (b, c, f) = (&ids[1], &ids[2], &ids[3]);

    let resp = run(
        &d,
        json!([["FileNode/copy", { "fromAccountId": ACCOUNT, "accountId": ACCOUNT,
            "create": {
                "deep": { "id": f, "parentId": c },
                "ok": { "id": f, "parentId": b },
            } }, "0"]]),
    )
    .await;
    let copy = args(&resp, 0);
    assert_eq!(copy["notCreated"]["deep"], depth_error(), "{copy}");
    assert!(
        copy["created"]["ok"]["id"].is_string(),
        "copy at depth 3: {copy}"
    );
}

#[tokio::test]
async fn no_limit_by_default() {
    let d = dispatcher(MemoryBackend::new().with_account(&Id::from(ACCOUNT)));
    create_all(
        &d,
        &[
            ("a", dir("a", Value::Null)),
            ("b", dir("b", json!("#a"))),
            ("c", dir("c", json!("#b"))),
            ("d", dir("d", json!("#c"))),
        ],
    )
    .await;
}
