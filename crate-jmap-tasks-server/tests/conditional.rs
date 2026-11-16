//! Conditional tasks Foo/set (draft-ietf-jmap-conditional-02) through
//! `register_tasks_conditional` and the memory backend's `ScopedState`
//! scopes (bd JMAP-y1yu.11; docs/design/conditional.md §5.1, §7).
//!
//! Oracles: conditional-02 §3.1 and §4.2 ("the server MUST NOT make any
//! of the changes"), and draft-ietf-jmap-tasks-06 (`onDestroyRemoveTasks`
//! destroys the TaskList's Tasks with it).
//!
//! The crate has no cross-object cap or uniqueness rule for an atomic
//! commit to check (`taskListHasTask` is judged within one TaskList/set,
//! which moves no Tasks; tasks-06 sets no uniqueness rule for `role`), so
//! there is no final-state cap test here; rollback covers the cascade.

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_server::Dispatcher;
use jmap_tasks_server::{register_tasks_conditional, register_tasks_handlers};
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:tasks",
    "urn:ietf:params:jmap:conditional",
];

fn rights() -> Value {
    json!({"mayReadItems": true, "mayWriteAll": true, "mayWriteOwn": true,
           "mayUpdatePrivate": true, "mayRSVP": true, "mayAdmin": true, "mayDelete": true})
}

/// Account acc1 with TaskList tl1 "Chores" holding Task t1.
fn backend() -> Arc<MemoryBackend> {
    let acc = Id::from("acc1");
    let b = MemoryBackend::new().with_account(&acc);
    b.seed_object(
        &acc,
        "TaskList",
        &Id::from("tl1"),
        json!({"id": "tl1", "name": "Chores", "sortOrder": 0, "isSubscribed": true,
               "myRights": rights()}),
    );
    b.seed_object(
        &acc,
        "Task",
        &Id::from("t1"),
        json!({"id": "t1", "@type": "Task", "uid": "t1-uid", "title": "Review",
               "taskListId": "tl1"}),
    );
    Arc::new(b)
}

fn dispatcher(b: &Arc<MemoryBackend>) -> Dispatcher<()> {
    let mut d = Dispatcher::new();
    register_tasks_handlers(&mut d, Arc::clone(b));
    register_tasks_conditional(&mut d, Arc::clone(b));
    d
}

async fn call(d: &Dispatcher<()>, method: &str, mut args: Value) -> (String, Value) {
    args["accountId"] = json!("acc1");
    let req: JmapRequest =
        serde_json::from_value(json!({"using": USING, "methodCalls": [[method, args, "0"]]}))
            .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

/// The ids `method` (a `/get`) returns, sorted, and the state string.
async fn ids(d: &Dispatcher<()>, method: &str) -> (Vec<String>, Value) {
    let (name, r) = call(d, method, json!({"properties": ["id"]})).await;
    assert_eq!(name, method, "{r}");
    let mut ids: Vec<String> = r["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    (ids, r["state"].clone())
}

#[tokio::test]
async fn precondition_on_a_task_list_name() {
    let b = backend();
    let d = dispatcher(&b);
    let rename_if = |expected: &str| json!({"ifUnchangedBy": {"tl1": {"name": expected}}, "update": {"tl1": {"name": "Errands"}}});
    let (name, r) = call(&d, "TaskList/set", rename_if("Work")).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": ["tl1"]}))
    );
    let (name, r) = call(&d, "TaskList/set", rename_if("Chores")).await;
    assert_eq!(name, "TaskList/set", "{r}");
    assert!(r["updated"].get("tl1").is_some(), "{r}");
}

/// §4.2 with a cascade: the `onDestroyRemoveTasks` destroy of tl1 and its
/// Task roll back with the failing update, state strings included.
#[tokio::test]
async fn atomic_failure_rolls_back_a_destroy_cascade() {
    let b = backend();
    let d = dispatcher(&b);
    let lists_before = ids(&d, "TaskList/get").await;
    let tasks_before = ids(&d, "Task/get").await;
    assert_eq!(tasks_before.0, ["t1"]);

    let (name, r) = call(
        &d,
        "TaskList/set",
        json!({
            "atomic": true,
            "onDestroyRemoveTasks": true,
            "destroy": ["tl1"],
            "update": {"nope": {"name": "x"}}
        }),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert_eq!(r["notUpdated"]["nope"]["type"], "notFound", "{r}");
    assert_eq!(ids(&d, "TaskList/get").await, lists_before);
    assert_eq!(ids(&d, "Task/get").await, tasks_before);

    let (name, r) = call(
        &d,
        "TaskList/set",
        json!({"atomic": true, "onDestroyRemoveTasks": true, "destroy": ["tl1"]}),
    )
    .await;
    assert_eq!(name, "TaskList/set", "{r}");
    assert!(ids(&d, "Task/get").await.0.is_empty());
}

#[test]
fn ready_after_handlers_and_conditional() {
    let b = backend();
    let mut d = Dispatcher::new();
    register_tasks_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps().len(), 3, "{:?}", d.conditional_gaps());
    register_tasks_conditional(&mut d, b);
    assert!(
        d.conditional_gaps().is_empty(),
        "{:?}",
        d.conditional_gaps()
    );
    assert!(d.conditional_ready());
}
