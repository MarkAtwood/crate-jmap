//! Sibling-name uniqueness and `onExists` on update and across one
//! FileNode/set (bd JMAP-6xas).
//!
//! Oracle: draft-ietf-jmap-filenode-20 §3.1.1 ("There MUST NOT be two
//! sibling FileNodes with both the same parent and the same name") and
//! §3.2.3 ("Within a single FileNode/set, the server MUST check the
//! uniqueness of sibling names against the result of all its creates,
//! updates and destroys together, so that a client can destroy a node and
//! create its replacement in the same method"; `onExists` "Controls what
//! happens when a create or update would give a FileNode the same name as
//! an existing sibling").

use std::sync::Arc;

use jmap_filenode_server::memory::MemoryBackend;

mod common;
use common::owner;
use jmap_filenode_server::register_filenode_handlers;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const USING: &[&str] = &["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:filenode"];

fn dispatcher() -> Dispatcher<Id> {
    let b = Arc::new(MemoryBackend::new().with_account_owner(&Id::from("u1"), &owner()));
    let mut d = Dispatcher::new();
    register_filenode_handlers(&mut d, b);
    d
}

/// Run one FileNode/set with `args` (accountId added); its response.
async fn set(d: &Dispatcher<Id>, mut args: Value) -> Value {
    args["accountId"] = json!("u1");
    let req: JmapRequest = serde_json::from_value(
        json!({"using": USING, "methodCalls": [["FileNode/set", args, "0"]]}),
    )
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, owner(), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    assert_eq!(inv[0], "FileNode/set", "{inv}");
    inv[1].clone()
}

/// Every node: id → (name, parentId).
async fn tree(d: &Dispatcher<Id>) -> Vec<(String, String, Value)> {
    let req: JmapRequest = serde_json::from_value(json!({
        "using": USING,
        "methodCalls": [["FileNode/get", {"accountId": "u1", "properties": ["name", "parentId"]}, "g"]]
    }))
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, owner(), State::from("s")).await).unwrap();
    let mut nodes: Vec<_> = resp["methodResponses"][0][1]["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            (
                n["id"].as_str().unwrap().to_owned(),
                n["name"].as_str().unwrap().to_owned(),
                n["parentId"].clone(),
            )
        })
        .collect();
    nodes.sort_by(|a, b| a.1.cmp(&b.1));
    nodes
}

fn dir(name: &str, parent: Option<&str>) -> Value {
    json!({"@type": "Directory", "name": name, "parentId": parent})
}

/// Create top-level Directories with `names`; their ids, in order.
async fn seed(d: &Dispatcher<Id>, names: &[&str]) -> Vec<String> {
    let create: serde_json::Map<String, Value> = names
        .iter()
        .map(|n| ((*n).to_owned(), dir(n, None)))
        .collect();
    let r = set(d, json!({"create": create})).await;
    names
        .iter()
        .map(|n| r["created"][n]["id"].as_str().expect("seeded").to_owned())
        .collect()
}

fn names(nodes: &[(String, String, Value)]) -> Vec<&str> {
    nodes.iter().map(|n| n.1.as_str()).collect()
}

#[tokio::test]
async fn rename_onto_a_sibling_is_already_exists() {
    let d = dispatcher();
    let ids = seed(&d, &["a", "b"]).await;
    let r = set(&d, json!({"update": {&ids[1]: {"name": "a"}}})).await;
    assert_eq!(
        r["notUpdated"][&ids[1]],
        json!({"type": "alreadyExists", "existingId": ids[0]})
    );
    assert_eq!(names(&tree(&d).await), ["a", "b"]);
}

#[tokio::test]
async fn move_onto_a_sibling_name_is_already_exists() {
    let d = dispatcher();
    let ids = seed(&d, &["x", "parent"]).await;
    let r = set(&d, json!({"create": {"inner": dir("x", Some(&ids[1]))}})).await;
    let inner = r["created"]["inner"]["id"].as_str().unwrap().to_owned();

    let r = set(&d, json!({"update": {&ids[0]: {"parentId": &ids[1]}}})).await;
    assert_eq!(
        r["notUpdated"][&ids[0]],
        json!({"type": "alreadyExists", "existingId": inner})
    );
    assert_eq!(
        tree(&d)
            .await
            .iter()
            .filter(|n| n.2 == json!(ids[1]))
            .count(),
        1
    );
}

/// Renaming a node to its own name, or to a case variant of it under case
/// folding, is not a collision with itself.
#[tokio::test]
async fn renaming_to_own_name_is_not_a_collision() {
    let d = dispatcher();
    let ids = seed(&d, &["Doc"]).await;
    let r = set(
        &d,
        json!({"compareCaseInsensitively": true, "update": {&ids[0]: {"name": "doc"}}}),
    )
    .await;
    assert!(r["updated"].get(&ids[0]).is_some(), "{r}");
    assert_eq!(names(&tree(&d).await), ["doc"]);
}

#[tokio::test]
async fn update_on_exists_replace_destroys_the_holder() {
    let d = dispatcher();
    let ids = seed(&d, &["a", "b"]).await;
    let r = set(
        &d,
        json!({"onExists": "replace", "update": {&ids[1]: {"name": "a"}}}),
    )
    .await;
    assert!(r["updated"].get(&ids[1]).is_some(), "{r}");
    assert_eq!(r["destroyed"], json!([ids[0]]), "{r}");
    let t = tree(&d).await;
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].0.as_str(), t[0].1.as_str()), (ids[1].as_str(), "a"));
}

#[tokio::test]
async fn update_on_exists_rename_reports_the_new_name() {
    let d = dispatcher();
    let ids = seed(&d, &["a", "b"]).await;
    let r = set(
        &d,
        json!({"onExists": "rename", "update": {&ids[1]: {"name": "a"}}}),
    )
    .await;
    // "MUST include the new name in the "created" or "updated" entry".
    let new_name = r["updated"][&ids[1]]["name"]
        .as_str()
        .expect("name in updated")
        .to_owned();
    assert_ne!(new_name, "a");
    let t = tree(&d).await;
    assert_eq!(t.len(), 2);
    assert!(t.iter().any(|n| n.0 == ids[1] && n.1 == new_name), "{t:?}");
}

/// "so that a client can destroy a node and create its replacement in the
/// same method".
#[tokio::test]
async fn destroy_and_create_the_same_name_in_one_set() {
    let d = dispatcher();
    let ids = seed(&d, &["report"]).await;
    let r = set(
        &d,
        json!({"create": {"new": dir("report", None)}, "destroy": [&ids[0]]}),
    )
    .await;
    let new_id = r["created"]["new"]["id"]
        .as_str()
        .expect("created")
        .to_owned();
    assert_eq!(r["destroyed"], json!([ids[0]]), "{r}");
    assert!(r["notDestroyed"].is_null(), "{r}");
    let t = tree(&d).await;
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].0, new_id);
}

/// The same for an update: rename "t" over "r" while destroying "r"
/// (conditional-02 example 5.9 without `atomic`).
#[tokio::test]
async fn rename_over_a_node_destroyed_in_the_same_set() {
    let d = dispatcher();
    let ids = seed(&d, &["report.txt", "report.txt.tmp"]).await;
    let (r_id, t_id) = (&ids[0], &ids[1]);
    let r = set(
        &d,
        json!({"update": {t_id: {"name": "report.txt"}}, "destroy": [r_id]}),
    )
    .await;
    assert!(r["updated"].get(t_id).is_some(), "{r}");
    assert_eq!(r["destroyed"], json!([r_id]), "{r}");
    let t = tree(&d).await;
    assert_eq!(t.len(), 1);
    assert_eq!(
        (t[0].0.as_str(), t[0].1.as_str()),
        (t_id.as_str(), "report.txt")
    );
}

/// A destroy that fails leaves the name taken, as in the final state: the
/// create collides and the destroy reports its own error, once.
#[tokio::test]
async fn failed_destroy_keeps_the_name() {
    let d = dispatcher();
    let ids = seed(&d, &["full"]).await;
    let r = set(&d, json!({"create": {"child": dir("c", Some(&ids[0]))}})).await;
    assert!(r["created"].get("child").is_some(), "{r}");

    let r = set(
        &d,
        json!({"create": {"new": dir("full", None)}, "destroy": [&ids[0]]}),
    )
    .await;
    assert_eq!(
        r["notCreated"]["new"],
        json!({"type": "alreadyExists", "existingId": ids[0]})
    );
    assert_eq!(r["notDestroyed"][&ids[0]]["type"], "nodeHasChildren", "{r}");
    assert!(r["destroyed"].is_null(), "{r}");
    assert_eq!(names(&tree(&d).await), ["c", "full"]);
}

/// With the cascade, a listed descendant the early destroy removed is
/// reported destroyed once, not `notFound`.
#[tokio::test]
async fn cascade_from_an_early_destroy_covers_listed_children() {
    let d = dispatcher();
    let ids = seed(&d, &["full"]).await;
    let r = set(&d, json!({"create": {"child": dir("c", Some(&ids[0]))}})).await;
    let child = r["created"]["child"]["id"].as_str().unwrap().to_owned();

    let r = set(
        &d,
        json!({
            "onDestroyRemoveChildren": true,
            "create": {"new": dir("full", None)},
            "destroy": [&ids[0], &child]
        }),
    )
    .await;
    assert!(r["created"].get("new").is_some(), "{r}");
    assert!(r["notDestroyed"].is_null(), "{r}");
    let mut destroyed: Vec<_> = r["destroyed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    destroyed.sort();
    let mut expected = vec![ids[0].clone(), child];
    expected.sort();
    assert_eq!(destroyed, expected);
}
