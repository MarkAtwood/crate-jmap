//! Task `workflowStatus` and `progress` (docs/design/DECISIONS.md decision
//! 42, jmapio/jmap #465; bd JMAP-jkl8.6) through `Task/set` and the memory
//! backend.
//!
//! Oracle: decision 42's text; draft-ietf-jmap-tasks-06 §3
//! (draft-ietf-jmap-tasks-06.txt: `workflowStatuses` "Default:
//! ["completed", "failed", "in-process", "needs-action", "cancelled",
//! "pending"]"); RFC 8984 §5.2.5 (the progress values) and §5.2.6
//! (`progressUpdated`). Expected values are written out by hand.

mod common;

use std::collections::BTreeMap;

use common::MemoryBackend;
use jmap_tasks_server::{handle_task_get, handle_task_set};
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

/// A backend with list `L1` (default statuses) and `L2` (a custom list
/// with "in-review" but no "pending").
fn backend() -> MemoryBackend {
    let b = MemoryBackend::new().with_account(&Id::from(ACCOUNT));
    for (id, statuses) in [
        ("L1", Value::Null),
        ("L2", json!(["needs-action", "in-review", "completed"])),
    ] {
        let mut list = json!({
            "id": id, "name": id, "sortOrder": 0, "isSubscribed": true,
            "myRights": {
                "mayReadItems": true, "mayWriteAll": true, "mayWriteOwn": true,
                "mayUpdatePrivate": true, "mayRSVP": true, "mayAdmin": true,
                "mayDelete": true
            }
        });
        if !statuses.is_null() {
            list["workflowStatuses"] = statuses;
        }
        b.seed_object(&Id::from(ACCOUNT), "TaskList", &Id::from(id), list);
    }
    b
}

async fn set(b: &MemoryBackend, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT);
    handle_task_set(b, &(), &BTreeMap::new(), args, &[])
        .await
        .expect("Task/set")
        .primary
}

/// Create one Task; the `created` entry, or `notCreated` as `Err`.
async fn create(b: &MemoryBackend, task: Value) -> Result<Value, Value> {
    let mut task = task;
    task["@type"] = json!("Task");
    task["uid"] = json!("u1");
    task["title"] = json!("t");
    let out = set(b, json!({ "create": { "c": task } })).await;
    match out["notCreated"].get("c") {
        Some(e) => Err(e.clone()),
        None => Ok(out["created"]["c"].clone()),
    }
}

async fn update(b: &MemoryBackend, id: &str, patch: Value) -> Result<Value, Value> {
    let out = set(b, json!({ "update": { id: patch } })).await;
    match out["notUpdated"].get(id) {
        Some(e) => Err(e.clone()),
        None => Ok(out["updated"][id].clone()),
    }
}

async fn stored(b: &MemoryBackend, id: &str) -> Value {
    let out = handle_task_get(b, &(), json!({ "accountId": ACCOUNT, "ids": [id] }), &[])
        .await
        .expect("Task/get")
        .primary;
    out["list"][0].clone()
}

fn invalid(e: &Value, property: &str) {
    assert_eq!(e["type"], "invalidProperties", "{e}");
    assert_eq!(e["properties"], json!([property]), "{e}");
}

/// "When workflowStatus is one of the RFC 8984 progress values, the server
/// sets progress to the same value (and progressUpdated)".
#[tokio::test]
async fn a_progress_value_is_mirrored() {
    let b = backend();
    let c = create(
        &b,
        json!({ "taskListId": "L1", "workflowStatus": "completed" }),
    )
    .await
    .unwrap();
    assert_eq!(c["progress"], "completed", "{c}");
    assert!(c["progressUpdated"].is_string(), "{c}");
    let id = c["id"].as_str().unwrap();
    let echo = update(&b, id, json!({ "workflowStatus": "in-process" }))
        .await
        .unwrap();
    assert_eq!(
        echo["progress"], "in-process",
        "the mirror is echoed: {echo}"
    );
    assert_eq!(stored(&b, id).await["progress"], "in-process");
}

/// "for any other value (pending, a list-defined one) progress is null".
#[tokio::test]
async fn other_values_leave_progress_null() {
    let b = backend();
    let c = create(
        &b,
        json!({ "taskListId": "L1", "workflowStatus": "pending" }),
    )
    .await
    .unwrap();
    assert!(c.get("progress").is_none_or(Value::is_null), "{c}");
    let c = create(
        &b,
        json!({ "taskListId": "L2", "workflowStatus": "in-review" }),
    )
    .await
    .unwrap();
    assert!(c.get("progress").is_none_or(Value::is_null), "{c}");
}

/// "A /set that sets progress to a value other than the mirrored one while
/// workflowStatus is set is invalidProperties".
#[tokio::test]
async fn a_conflicting_progress_is_rejected() {
    let b = backend();
    let e = create(
        &b,
        json!({ "taskListId": "L1", "workflowStatus": "completed", "progress": "in-process" }),
    )
    .await
    .unwrap_err();
    invalid(&e, "progress");
    let c = create(
        &b,
        json!({ "taskListId": "L1", "workflowStatus": "completed" }),
    )
    .await
    .unwrap();
    let e = update(
        &b,
        c["id"].as_str().unwrap(),
        json!({ "progress": "failed" }),
    )
    .await
    .unwrap_err();
    invalid(&e, "progress");
    // The matching value is not a conflict.
    create(
        &b,
        json!({ "taskListId": "L1", "workflowStatus": "failed", "progress": "failed" }),
    )
    .await
    .unwrap();
}

/// "a workflowStatus not in the task list's workflowStatuses is
/// invalidProperties"; a list without `workflowStatuses` has the -06
/// default.
#[tokio::test]
async fn the_status_must_be_in_the_list() {
    let b = backend();
    invalid(
        &create(
            &b,
            json!({ "taskListId": "L1", "workflowStatus": "in-review" }),
        )
        .await
        .unwrap_err(),
        "workflowStatus",
    );
    invalid(
        &create(
            &b,
            json!({ "taskListId": "L2", "workflowStatus": "pending" }),
        )
        .await
        .unwrap_err(),
        "workflowStatus",
    );
    let c = create(&b, json!({ "taskListId": "L1" })).await.unwrap();
    invalid(
        &update(
            &b,
            c["id"].as_str().unwrap(),
            json!({ "workflowStatus": "in-review" }),
        )
        .await
        .unwrap_err(),
        "workflowStatus",
    );
}

/// "changing a list's workflowStatuses does not touch existing tasks": a
/// later update that does not set workflowStatus succeeds.
#[tokio::test]
async fn a_list_change_leaves_tasks_alone() {
    let b = backend();
    let c = create(
        &b,
        json!({ "taskListId": "L2", "workflowStatus": "in-review" }),
    )
    .await
    .unwrap();
    let id = c["id"].as_str().unwrap();
    let out =
        handle_task_set_list(&b, json!({ "L2": { "workflowStatuses": ["completed"] } })).await;
    assert!(out["notUpdated"].is_null(), "{out}");
    update(&b, id, json!({ "title": "renamed" })).await.unwrap();
    assert_eq!(stored(&b, id).await["workflowStatus"], "in-review");
}

async fn handle_task_set_list(b: &MemoryBackend, update: Value) -> Value {
    jmap_tasks_server::handle_task_list_set(
        b,
        &(),
        json!({ "accountId": ACCOUNT, "update": update }),
        &[],
    )
    .await
    .expect("TaskList/set")
    .primary
}

/// "moving a task to a list whose workflowStatuses lacks its value is
/// invalidProperties naming workflowStatus unless the same update sets a
/// value the list has, or null".
#[tokio::test]
async fn moving_to_a_list_without_the_status() {
    let b = backend();
    let c = create(
        &b,
        json!({ "taskListId": "L2", "workflowStatus": "in-review" }),
    )
    .await
    .unwrap();
    let id = c["id"].as_str().unwrap();
    invalid(
        &update(&b, id, json!({ "taskListId": "L1" }))
            .await
            .unwrap_err(),
        "workflowStatus",
    );
    update(
        &b,
        id,
        json!({ "taskListId": "L1", "workflowStatus": "pending" }),
    )
    .await
    .unwrap();
    update(
        &b,
        id,
        json!({ "taskListId": "L2", "workflowStatus": null }),
    )
    .await
    .unwrap();
}
