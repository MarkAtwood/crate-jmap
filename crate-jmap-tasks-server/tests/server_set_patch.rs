//! Server-set values and empty patches in `Task/set` and `TaskList/set`
//! updates (bd:JMAP-t8ej.27.4).
//!
//! Oracle, RFC 8620 §5.3 (update): "Any server-set properties MAY be
//! included in the patch if their value is identical to the current server
//! value (before applying the patches to the object).  Otherwise, the
//! update MUST be rejected with an "invalidProperties" SetError."
//!
//! draft-ietf-jmap-tasks-06: TaskList `id` is "(immutable; server-set)",
//! `myRights` "(server-set)"; Task `id` is "(immutable; server-set)".
//!
//! An empty patch `{}`, literal or left empty once the identical
//! server-set values are dropped, succeeds as a no-op: `updated` maps the
//! id to null, nothing is written, and the state is unchanged (decision
//! recorded on JMAP-t8ej.25).

mod common;

use std::collections::BTreeMap;

use common::MemoryBackend;
use jmap_tasks_server::{
    handle_task_get, handle_task_list_get, handle_task_list_set, handle_task_set,
};
use jmap_types::Id;
use serde_json::{Value, json};

fn my_rights() -> Value {
    json!({
        "mayReadItems": true,
        "mayWriteAll": true,
        "mayWriteOwn": true,
        "mayUpdatePrivate": true,
        "mayRSVP": true,
        "mayAdmin": true,
        "mayDelete": true
    })
}

/// A backend with TaskList `tl1` and Task `t1` in it.
fn seeded_backend() -> MemoryBackend {
    let acc = Id::from("acc1");
    let backend = MemoryBackend::new().with_account(&acc);
    backend.seed_object(
        &acc,
        "TaskList",
        &Id::from("tl1"),
        json!({
            "id": "tl1",
            "name": "Chores",
            "sortOrder": 0,
            "isSubscribed": true,
            "myRights": my_rights()
        }),
    );
    backend.seed_object(
        &acc,
        "Task",
        &Id::from("t1"),
        json!({
            "id": "t1",
            "@type": "Task",
            "uid": "t1-uid",
            "title": "Review"
        }),
    );
    backend
}

async fn task_set(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { (id): patch } });
    handle_task_set(backend, &(), &BTreeMap::new(), args, &[])
        .await
        .expect("/set must succeed at the top level")
        .primary
}

async fn task_get(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["t1"] });
    handle_task_get(backend, &(), args, &[])
        .await
        .expect("/get must succeed")
        .primary["list"][0]
        .clone()
}

async fn list_set(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { (id): patch } });
    handle_task_list_set(backend, &(), args, &[])
        .await
        .expect("/set must succeed at the top level")
        .primary
}

async fn list_get(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["tl1"] });
    handle_task_list_get(backend, &(), args, &[])
        .await
        .expect("/get must succeed")
        .primary["list"][0]
        .clone()
}

/// Assert `resp` reports `id` as a no-op update: `updated[id]` is null,
/// nothing failed, and the state did not move.
fn assert_noop(resp: &Value, id: &str) {
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(resp["updated"], json!({ (id): null }), "{resp}");
    assert_eq!(resp["oldState"], resp["newState"], "{resp}");
}

#[tokio::test]
async fn task_identical_id_is_a_noop() {
    let backend = seeded_backend();
    let before = task_get(&backend).await;
    let resp = task_set(&backend, "t1", json!({ "id": "t1" })).await;
    assert_noop(&resp, "t1");
    assert_eq!(task_get(&backend).await, before);
}

#[tokio::test]
async fn task_identical_id_with_change_applies() {
    let backend = seeded_backend();
    let resp = task_set(&backend, "t1", json!({ "id": "t1", "title": "Done" })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(task_get(&backend).await["title"], "Done");
}

#[tokio::test]
async fn task_changed_id_is_invalid_properties() {
    let backend = seeded_backend();
    let resp = task_set(&backend, "t1", json!({ "id": "t2", "title": "Done" })).await;
    assert_eq!(
        resp["notUpdated"]["t1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["t1"]["properties"],
        json!(["id"]),
        "{resp}"
    );
    assert_eq!(task_get(&backend).await["title"], "Review");
}

#[tokio::test]
async fn task_empty_patch_is_a_noop() {
    let backend = seeded_backend();
    let before = task_get(&backend).await;
    let resp = task_set(&backend, "t1", json!({})).await;
    assert_noop(&resp, "t1");
    assert_eq!(task_get(&backend).await, before);
}

#[tokio::test]
async fn task_empty_patch_of_missing_id_is_not_found() {
    // RFC 8620 §5.3 notFound: "The id given to update/destroy cannot be
    // found."
    let backend = seeded_backend();
    let resp = task_set(&backend, "t9", json!({})).await;
    assert_eq!(resp["notUpdated"]["t9"]["type"], "notFound", "{resp}");
}

#[tokio::test]
async fn task_list_identical_server_set_values_are_a_noop() {
    let backend = seeded_backend();
    let before = list_get(&backend).await;
    let resp = list_set(
        &backend,
        "tl1",
        json!({ "id": "tl1", "myRights": my_rights() }),
    )
    .await;
    assert_noop(&resp, "tl1");
    assert_eq!(list_get(&backend).await, before);
}

#[tokio::test]
async fn task_list_identical_id_with_change_applies() {
    let backend = seeded_backend();
    let resp = list_set(&backend, "tl1", json!({ "id": "tl1", "name": "Errands" })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(list_get(&backend).await["name"], "Errands");
}

#[tokio::test]
async fn task_list_changed_my_rights_is_invalid_properties() {
    let backend = seeded_backend();
    let mut rights = my_rights();
    rights["mayDelete"] = json!(false);
    let resp = list_set(&backend, "tl1", json!({ "myRights": rights })).await;
    assert_eq!(
        resp["notUpdated"]["tl1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["tl1"]["properties"],
        json!(["myRights"]),
        "{resp}"
    );
}

#[tokio::test]
async fn task_list_empty_patch_is_a_noop() {
    let backend = seeded_backend();
    let before = list_get(&backend).await;
    let resp = list_set(&backend, "tl1", json!({})).await;
    assert_noop(&resp, "tl1");
    assert_eq!(list_get(&backend).await, before);
}
