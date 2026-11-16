//! Conditional FileNode/set (draft-ietf-jmap-conditional-02) through
//! `register_filenode_conditional` and the memory backend's `ScopedState`
//! scopes (bd JMAP-y1yu.8; docs/design/conditional.md §5.1, §7, §9).
//!
//! Oracles: conditional-02 examples 5.1 and 5.3 to 5.9, with the
//! examples' own ids and values seeded into the store, and
//! draft-ietf-jmap-filenode-20 §3.1.1 / §3.2.3 for the tree rules the
//! atomic commit checks.

use std::sync::Arc;

use jmap_filenode_server::memory::MemoryBackend;
use jmap_filenode_server::{register_filenode_conditional, register_filenode_handlers};
use jmap_filenode_types::FileNode;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:filenode",
    "urn:ietf:params:jmap:conditional",
];
const CHANGED: &str = "2026-04-30T12:00:00Z";

fn node(id: &str, kind: &str, name: &str, parent: Option<&str>, extra: Value) -> FileNode {
    let mut v = json!({
        "id": id, "@type": kind, "name": name, "parentId": parent, "role": null,
        "myRights": null, "created": "2026-01-01T00:00:00Z",
        "modified": "2026-01-01T00:00:00Z", "accessed": null, "changed": CHANGED
    });
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(v).expect("seed FileNode")
}

fn file(id: &str, name: &str, parent: Option<&str>, blob: &str) -> FileNode {
    node(
        id,
        "File",
        name,
        parent,
        json!({"blobId": blob, "size": 1, "type": "text/plain"}),
    )
}

/// The examples' nodes, in account u1:
/// - `d3` Directory "Reports", shared with bob (`mayRead` true);
/// - `f42` File in `d3`, content `G_old`; `thumbs` Directory;
/// - `r` "report.txt" (`G_old`) and `t` "report.txt.tmp" (`G_new`);
/// - `a` "current.txt" and `b` "previous.txt".
fn backend() -> Arc<MemoryBackend> {
    let u1 = Id::from("u1");
    let b = MemoryBackend::new().with_account(&u1);
    let rights = |read: bool| {
        json!({"mayRead": read, "mayAddChildren": false, "mayRename": false,
               "mayDelete": false, "mayModifyContent": false, "mayShare": false})
    };
    for n in [
        node(
            "d3",
            "Directory",
            "Reports",
            None,
            json!({"shareWith": {"bob": rights(true), "carol": rights(true)}}),
        ),
        file("f42", "f42.txt", Some("d3"), "G_old"),
        node("thumbs", "Directory", "thumbs", None, json!({})),
        file("r", "report.txt", None, "G_old"),
        file("t", "report.txt.tmp", None, "G_new"),
        file("a", "current.txt", None, "G_a"),
        file("b", "previous.txt", None, "G_b"),
    ] {
        b.seed_node(&u1, n);
    }
    Arc::new(b)
}

fn dispatcher(b: &Arc<MemoryBackend>) -> Dispatcher<()> {
    let mut d = Dispatcher::new();
    register_filenode_handlers(&mut d, Arc::clone(b));
    register_filenode_conditional(&mut d, Arc::clone(b)).unwrap();
    d
}

/// Run one FileNode/set with `args` (accountId added); the response name
/// and arguments.
async fn set(d: &Dispatcher<()>, mut args: Value) -> (String, Value) {
    args["accountId"] = json!("u1");
    let req: JmapRequest = serde_json::from_value(
        json!({"using": USING, "methodCalls": [["FileNode/set", args, "0"]]}),
    )
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

/// The stored node `id`, or `None`.
async fn get(d: &Dispatcher<()>, id: &str) -> Option<Value> {
    let req: JmapRequest = serde_json::from_value(json!({
        "using": USING,
        "methodCalls": [["FileNode/get", {"accountId": "u1", "ids": [id]}, "g"]]
    }))
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    resp["methodResponses"][0][1]["list"]
        .as_array()
        .unwrap()
        .first()
        .cloned()
}

fn state_mismatch(failed: &[&str]) -> (String, Value) {
    (
        "error".to_owned(),
        json!({"type": "stateMismatch", "failed": failed}),
    )
}

/// 5.1: force-with-lease on `blobId`.
#[tokio::test]
async fn example_5_1_force_with_lease() {
    let b = backend();
    let d = dispatcher(&b);
    let lease = json!({
        "ifUnchangedBy": {"f42": {"blobId": "G_old"}},
        "update": {"f42": {"blobId": "G_new", "modified": "2026-05-01T09:30:00Z"}}
    });
    let (name, r) = set(&d, lease.clone()).await;
    assert_eq!(name, "FileNode/set", "{r}");
    assert!(r["updated"].get("f42").is_some(), "{r}");

    // "If another client has already replaced the content ... the server
    // makes no change, returning a method-level "stateMismatch" error".
    set(&d, json!({"update": {"f42": {"blobId": "G_other"}}})).await;
    assert_eq!(set(&d, lease).await, state_mismatch(&["f42"]));
    assert_eq!(get(&d, "f42").await.unwrap()["blobId"], "G_other");
}

/// 5.3: a pointer into `shareWith` ignores unrelated changes.
#[tokio::test]
async fn example_5_3_ignores_unrelated_changes() {
    let b = backend();
    let d = dispatcher(&b);
    // Concurrent changes to d3's name and to another grantee.
    let (_, r) = set(
        &d,
        json!({"update": {"d3": {"name": "Reports 2026", "shareWith/carol": null}}}),
    )
    .await;
    assert!(r["updated"].get("d3").is_some(), "{r}");

    let (name, r) = set(
        &d,
        json!({
            "ifUnchangedBy": {"d3": {"shareWith/bob/mayRead": true}},
            "update": {"d3": {"shareWith/bob": null}}
        }),
    )
    .await;
    assert_eq!(name, "FileNode/set", "{r}");
    assert!(
        get(&d, "d3").await.unwrap()["shareWith"]
            .get("bob")
            .is_none()
    );
}

/// 5.4: a precondition on an object the method does not change.
#[tokio::test]
async fn example_5_4_condition_on_unchanged_object() {
    let write = json!({
        "ifUnchangedBy": {"f42": {"parentId": "d3"}, "d3": {"name": "Reports"}},
        "update": {"f42": {"blobId": "G_new", "modified": "2026-05-01T09:30:00Z"}}
    });

    let b = backend();
    let d = dispatcher(&b);
    set(&d, json!({"update": {"d3": {"name": "Archive"}}})).await;
    assert_eq!(set(&d, write.clone()).await, state_mismatch(&["d3"]));
    assert_eq!(get(&d, "f42").await.unwrap()["blobId"], "G_old");

    let b = backend();
    let d = dispatcher(&b);
    let (name, r) = set(&d, write).await;
    assert_eq!(name, "FileNode/set", "{r}");
    assert_eq!(get(&d, "f42").await.unwrap()["blobId"], "G_new");
}

/// 5.5: `null` requires that the object no longer exists.
#[tokio::test]
async fn example_5_5_condition_on_destroyed_object() {
    let rename = json!({
        "ifUnchangedBy": {"r": null},
        "update": {"t": {"name": "report.txt"}}
    });
    let b = backend();
    let d = dispatcher(&b);
    assert_eq!(set(&d, rename.clone()).await, state_mismatch(&["r"]));
    assert_eq!(get(&d, "t").await.unwrap()["name"], "report.txt.tmp");

    set(&d, json!({"destroy": ["r"]})).await;
    let (name, r) = set(&d, rename).await;
    assert_eq!(name, "FileNode/set", "{r}");
    assert_eq!(get(&d, "t").await.unwrap()["name"], "report.txt");
}

/// 5.6: `{}` requires only that the object exists. The example's create
/// has no `@type`, which draft-ietf-jmap-filenode-20 §3.1.1 says "MUST be
/// provided on creation"; it is added here.
#[tokio::test]
async fn example_5_6_condition_on_existing_object() {
    let thumbnail = json!({
        "ifUnchangedBy": {"f42": {}},
        "create": {"th1": {
            "@type": "File", "parentId": "thumbs", "name": "f42.png", "blobId": "G_thumb"
        }}
    });
    let b = backend();
    let d = dispatcher(&b);
    // A rename leaves f42 in place, so the precondition still holds.
    set(&d, json!({"update": {"f42": {"name": "renamed.txt"}}})).await;
    let (name, r) = set(&d, thumbnail.clone()).await;
    assert_eq!(name, "FileNode/set", "{r}");
    assert!(r["created"].get("th1").is_some(), "{r}");

    let b = backend();
    let d = dispatcher(&b);
    set(&d, json!({"destroy": ["f42"]})).await;
    assert_eq!(set(&d, thumbnail).await, state_mismatch(&["f42"]));
}

/// 5.7: the server-set `changed` timestamp as a whole-object token.
#[tokio::test]
async fn example_5_7_changed_compare_and_swap() {
    let b = backend();
    let d = dispatcher(&b);
    let cas = |name: &str| {
        json!({
            "ifUnchangedBy": {"f42": {"changed": CHANGED}},
            "update": {"f42": {"name": name}}
        })
    };
    let (name, r) = set(&d, cas("first.txt")).await;
    assert_eq!(name, "FileNode/set", "{r}");
    // The server moved `changed` on that update, so the same token fails.
    assert_ne!(get(&d, "f42").await.unwrap()["changed"], CHANGED);
    assert_eq!(set(&d, cas("second.txt")).await, state_mismatch(&["f42"]));
    assert_eq!(get(&d, "f42").await.unwrap()["name"], "first.txt");
}

/// 5.8: an atomic swap of two names succeeds; a third rename that
/// conflicts fails the whole method, naming the offender, and nothing is
/// renamed.
#[tokio::test]
async fn example_5_8_atomic_name_swap() {
    let b = backend();
    let d = dispatcher(&b);
    let (name, r) = set(
        &d,
        json!({
            "atomic": true,
            "update": {"a": {"name": "previous.txt"}, "b": {"name": "current.txt"}}
        }),
    )
    .await;
    assert_eq!(name, "FileNode/set", "{r}");
    assert_eq!(get(&d, "a").await.unwrap()["name"], "previous.txt");
    assert_eq!(get(&d, "b").await.unwrap()["name"], "current.txt");

    let b = backend();
    let d = dispatcher(&b);
    let (name, r) = set(
        &d,
        json!({
            "atomic": true,
            "update": {
                "a": {"name": "previous.txt"},
                "b": {"name": "current.txt"},
                "t": {"name": "current.txt"}
            }
        }),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    let offenders: Vec<&String> = r["notUpdated"].as_object().unwrap().keys().collect();
    assert!(!offenders.is_empty(), "{r}");
    for id in &offenders {
        assert_eq!(r["notUpdated"][id.as_str()]["type"], "alreadyExists", "{r}");
    }
    assert_eq!(get(&d, "a").await.unwrap()["name"], "current.txt");
    assert_eq!(get(&d, "b").await.unwrap()["name"], "previous.txt");
    assert_eq!(get(&d, "t").await.unwrap()["name"], "report.txt.tmp");
}

/// 5.9: atomic replace of one file by another, guarded on its content.
#[tokio::test]
async fn example_5_9_atomic_replace() {
    let replace = json!({
        "atomic": true,
        "ifUnchangedBy": {"r": {"blobId": "G_old"}},
        "update": {"t": {"name": "report.txt"}},
        "destroy": ["r"]
    });

    let b = backend();
    let d = dispatcher(&b);
    let (name, r) = set(&d, replace.clone()).await;
    assert_eq!(name, "FileNode/set", "{r}");
    assert_eq!(r["destroyed"], json!(["r"]));
    assert!(get(&d, "r").await.is_none());
    assert_eq!(get(&d, "t").await.unwrap()["name"], "report.txt");

    // Stale G_old: "neither file is changed".
    let b = backend();
    let d = dispatcher(&b);
    set(&d, json!({"update": {"r": {"blobId": "G_other"}}})).await;
    assert_eq!(set(&d, replace).await, state_mismatch(&["r"]));
    assert_eq!(get(&d, "r").await.unwrap()["name"], "report.txt");
    assert_eq!(get(&d, "t").await.unwrap()["name"], "report.txt.tmp");
}

/// Under atomic the move-under-self check runs on the final state: moving
/// a Directory under its child is rejected at commit, and rolled back.
#[tokio::test]
async fn atomic_parent_loop_is_rejected_at_commit() {
    let b = backend();
    let d = dispatcher(&b);
    let (name, r) = set(
        &d,
        json!({"atomic": true, "update": {"d3": {"parentId": "#inner"}}, "create": {
            "inner": {"@type": "Directory", "name": "inner", "parentId": "d3"}
        }}),
    )
    .await;
    // The create runs first, then the move closes the loop.
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert!(get(&d, "d3").await.unwrap()["parentId"].is_null());
    assert_eq!(get(&d, "f42").await.unwrap()["parentId"], "d3");
}

#[test]
fn ready_after_handlers_and_conditional() {
    let b = backend();
    let mut d = Dispatcher::new();
    register_filenode_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps(), ["FileNode/set"]);
    register_filenode_conditional(&mut d, b).unwrap();
    assert!(d.conditional_gaps().is_empty());
    assert!(d.conditional_ready());
}

/// Under atomic a parent that is gone in the final state fails the
/// change: a node moved under a parent this call destroys blames the
/// destroy (`nodeHasChildren`).
#[tokio::test]
async fn atomic_move_under_destroyed_parent_is_rejected_at_commit() {
    let b = backend();
    let d = dispatcher(&b);
    let (name, r) = set(
        &d,
        json!({"atomic": true, "update": {"a": {"parentId": "thumbs"}}, "destroy": ["thumbs"]}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(
        r,
        json!({"type": "atomicFailure", "notDestroyed": {"thumbs": {"type": "nodeHasChildren"}}})
    );
    assert!(get(&d, "thumbs").await.is_some());
    assert!(get(&d, "a").await.unwrap()["parentId"].is_null());
}
