//! Integration tests for `jmap-tasks-server` using `MemoryBackend`.
//!
//! All expected values are derived from the spec (draft-ietf-jmap-tasks-06)
//! and RFC 8620, not from the code under test. Wire-shape literals are
//! hand-written from the draft's prose.
//!
//! Bead: JMAP-hwdv.7.

mod common;

use common::MemoryBackend;
use jmap_tasks_server::{
    handle_task_changes, handle_task_get, handle_task_list_changes, handle_task_list_get,
    handle_task_list_set, handle_task_notification_set, handle_task_query, handle_task_set,
};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a minimal valid TaskList JSON value with the given id and name.
///
/// Fields below cover draft-tasks-06 §3 mandatory shape. `myRights` is
/// server-set and the `#[non_exhaustive]` TaskList struct demands every
/// known field present at deserialize.
fn task_list_fixture(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "description": null,
        "color": null,
        "sortOrder": 0,
        "isSubscribed": true,
        "isVisible": true,
        "includeInAvailability": "all",
        "defaultAlertsWithTime": null,
        "defaultAlertsWithoutTime": null,
        "timeZone": null,
        "shareWith": null,
        "myRights": {
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayWriteOwn": true,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "mayAdmin": true,
            "mayDelete": true
        }
    })
}

/// Build a minimal valid Task JSON value referencing the given task list.
fn task_fixture(id: &str, task_list_id: &str, title: &str) -> Value {
    json!({
        "id": id,
        "@type": "Task",
        "uid": format!("{id}-uid"),
        "title": title,
        "taskListId": task_list_id
    })
}

// ---------------------------------------------------------------------------
// Test 1: TaskList/get against an empty account → empty list, notFound=[]
// Oracle: RFC 8620 §5.1.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_get_empty_account_returns_empty_list() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({
        "accountId": "acc1",
        "ids": null
    });

    let (resp, _) = handle_task_list_get(&backend, &(), args)
        .await
        .expect("/get must not return top-level error");

    assert_eq!(resp["accountId"], "acc1");
    assert!(
        resp["list"].is_array() && resp["list"].as_array().unwrap().is_empty(),
        "list must be empty: {resp}"
    );
    assert!(
        resp["notFound"].is_array(),
        "notFound MUST be an array (never null) per RFC 8620 §5.1: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: TaskList/get with seed + unknown id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_get_seeded_and_unknown_id() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));

    let args = json!({
        "accountId": "acc1",
        "ids": ["tl1", "missing"]
    });

    let (resp, _) = handle_task_list_get(&backend, &(), args)
        .await
        .expect("/get must succeed");

    let list = resp["list"].as_array().unwrap();
    assert_eq!(list.len(), 1, "exactly one TaskList: {resp}");
    assert_eq!(list[0]["id"], "tl1");
    assert_eq!(list[0]["name"], "Todo");

    let not_found = resp["notFound"].as_array().unwrap();
    assert_eq!(not_found.len(), 1, "one unknown id: {resp}");
    assert_eq!(not_found[0], "missing");
}

// ---------------------------------------------------------------------------
// Test 3: TaskList/changes since current state → empty
// Oracle: RFC 8620 §5.2.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_changes_empty_store_empty_result() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({ "accountId": "acc1", "sinceState": "0" });
    let (resp, _) = handle_task_list_changes(&backend, &(), args)
        .await
        .expect("/changes must succeed");

    assert_eq!(resp["oldState"], "0");
    assert_eq!(resp["newState"], "0");
    assert!(resp["created"].as_array().unwrap().is_empty());
    assert!(resp["updated"].as_array().unwrap().is_empty());
    assert!(resp["destroyed"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Test 4: TaskList/set destroy empty list succeeds
// Oracle: draft-tasks-06 §3.4 — destroy proceeds when the task list has
// no tasks.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_set_destroy_empty_list_succeeds() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));

    let args = json!({
        "accountId": "acc1",
        "destroy": ["tl1"]
    });
    let (resp, _) = handle_task_list_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    let destroyed = resp["destroyed"].as_array().unwrap();
    assert_eq!(destroyed.len(), 1);
    assert_eq!(destroyed[0], "tl1");
    assert_ne!(resp["oldState"], resp["newState"], "state must bump");
}

// ---------------------------------------------------------------------------
// Test 5: TaskList/set destroy non-empty list → taskListHasTask error
// Oracle: draft-tasks-06 §3.4 — when onDestroyRemoveTasks is false and
// the list has tasks, destroy MUST fail with `taskListHasTask`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_set_destroy_with_tasks_returns_error() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));
    backend.seed_object("acc1", "Task", "t1", task_fixture("t1", "tl1", "Buy milk"));

    let args = json!({
        "accountId": "acc1",
        "destroy": ["tl1"]
        // onDestroyRemoveTasks defaults to false
    });
    let (resp, _) = handle_task_list_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert!(
        resp["notDestroyed"].is_object(),
        "notDestroyed must be present: {resp}"
    );
    assert_eq!(
        resp["notDestroyed"]["tl1"]["type"], "taskListHasTask",
        "draft §3.4 requires taskListHasTask: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: TaskList/set create rejects client-supplied id
// Oracle: RFC 8620 §5.3.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_set_create_with_client_id_rejects() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": { "id": "client-id", "name": "Inbox" }
        }
    });
    let (resp, _) = handle_task_list_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert_eq!(
        resp["notCreated"]["c1"]["type"], "invalidProperties",
        "client-supplied id must be rejected: {resp}"
    );
    assert_eq!(resp["notCreated"]["c1"]["properties"][0], "id");
}

// ---------------------------------------------------------------------------
// Test 7: TaskList/set on unknown accountId → accountNotFound
// Oracle: RFC 8620 §3.6.2.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_set_unknown_account_returns_account_not_found() {
    let backend = MemoryBackend::new();

    let args = json!({
        "accountId": "nobody",
        "create": { "c1": { "name": "x" } }
    });
    let err = handle_task_list_set(&backend, &(), args)
        .await
        .expect_err("must produce method-level error");

    let err_str = format!("{err:?}");
    assert!(
        err_str.contains("accountNotFound") || err_str.contains("AccountNotFound"),
        "must be accountNotFound: {err_str}"
    );
}

// ---------------------------------------------------------------------------
// Test 8: Task/get against empty store
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_get_empty_account_returns_empty_list() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({ "accountId": "acc1", "ids": null });
    let (resp, _) = handle_task_get(&backend, &(), args)
        .await
        .expect("/get must succeed");

    assert!(resp["list"].as_array().unwrap().is_empty());
    assert!(resp["notFound"].is_array());
}

// ---------------------------------------------------------------------------
// Test 9: Task/query against empty store returns no ids
// Oracle: RFC 8620 §5.5.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_query_empty_store_returns_no_ids() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({
        "accountId": "acc1",
        "calculateTotal": true
    });
    let (resp, _) = handle_task_query(&backend, &(), args)
        .await
        .expect("/query must succeed");

    assert!(resp["ids"].as_array().unwrap().is_empty());
    assert_eq!(resp["total"], 0);
}

// ---------------------------------------------------------------------------
// Test 10: Task/changes on empty store
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_changes_empty_store_empty_result() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({ "accountId": "acc1", "sinceState": "0" });
    let (resp, _) = handle_task_changes(&backend, &(), args)
        .await
        .expect("/changes must succeed");

    assert!(resp["created"].as_array().unwrap().is_empty());
    assert!(resp["updated"].as_array().unwrap().is_empty());
    assert!(resp["destroyed"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Test 11: state bump verified via /changes
// Oracle: RFC 8620 §5.2.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_set_state_bumps_visible_via_changes() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));

    let args = json!({ "accountId": "acc1", "destroy": ["tl1"] });
    let (resp, _) = handle_task_list_set(&backend, &(), args)
        .await
        .expect("/set must succeed");
    let old_state = resp["oldState"].clone();
    let new_state = resp["newState"].clone();
    assert_ne!(old_state, new_state);

    let args = json!({ "accountId": "acc1", "sinceState": old_state });
    let (changes, _) = handle_task_list_changes(&backend, &(), args)
        .await
        .expect("/changes must succeed");

    let destroyed = changes["destroyed"].as_array().unwrap();
    assert_eq!(destroyed.len(), 1);
    assert_eq!(destroyed[0], "tl1");
    assert_eq!(changes["newState"], new_state);
}

// ---------------------------------------------------------------------------
// Test 12: Task/set create with client-supplied id rejected
// Oracle: RFC 8620 §5.3.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_set_create_with_client_id_rejects() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));

    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": {
                "id": "client-id",
                "@type": "Task",
                "uid": "u1",
                "title": "Buy milk",
                "taskListId": "tl1"
            }
        }
    });
    let (resp, _) = handle_task_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert_eq!(
        resp["notCreated"]["c1"]["type"], "invalidProperties",
        "client-supplied id must be rejected: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 13: Task/set on unknown accountId → accountNotFound (JMAP-gpt1)
// Oracle: RFC 8620 §3.6.2.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_set_unknown_account_returns_account_not_found() {
    let backend = MemoryBackend::new();

    let args = json!({
        "accountId": "nobody",
        "create": { "c1": { "@type": "Task", "uid": "u1", "title": "x", "taskListId": "tl1" } }
    });
    let err = handle_task_set(&backend, &(), args)
        .await
        .expect_err("unknown accountId must produce method-level error");

    let err_str = format!("{err:?}");
    assert!(
        err_str.contains("accountNotFound") || err_str.contains("AccountNotFound"),
        "must be accountNotFound: {err_str}"
    );
}

// ---------------------------------------------------------------------------
// Test 14: TaskNotification/set on unknown accountId → accountNotFound (JMAP-gpt1)
// Oracle: RFC 8620 §3.6.2.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_notification_set_unknown_account_returns_account_not_found() {
    let backend = MemoryBackend::new();

    let args = json!({
        "accountId": "nobody",
        "destroy": ["nope"]
    });
    let err = handle_task_notification_set(&backend, &(), args)
        .await
        .expect_err("unknown accountId must produce method-level error");

    let err_str = format!("{err:?}");
    assert!(
        err_str.contains("accountNotFound") || err_str.contains("AccountNotFound"),
        "must be accountNotFound: {err_str}"
    );
}

// ---------------------------------------------------------------------------
// Test 15: MemoryBackend enforces isDraft immutability at the backend layer
// Oracle: draft-ietf-jmap-tasks-06 §4 (isDraft paragraph) — once isDraft is
// set to false, it MUST NOT be updated back to true. Workspace AGENTS.md
// "Permission enforcement: backend canonical" requires the backend to
// re-verify atomically; the reference MemoryBackend models that.
//
// This test exercises Task/set end-to-end. MemoryBackend now returns
// enforce_is_draft_atomically() = true, so the handler skips its pre-fetch
// path and the rejection comes from the backend (not the handler).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_set_isdraft_revert_rejected_by_memory_backend() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "Task",
        "t1",
        json!({
            "id": "t1",
            "@type": "Task",
            "uid": "t1-uid",
            "title": "Published task",
            "isDraft": false
        }),
    );

    let args = json!({
        "accountId": "acc1",
        "update": { "t1": { "isDraft": true } }
    });
    let (resp, _) = handle_task_set(&backend, &(), args)
        .await
        .expect("/set must succeed at the top level");

    let not_updated = resp["notUpdated"].as_object().expect("notUpdated present");
    let err = not_updated.get("t1").expect("t1 in notUpdated");
    assert_eq!(
        err["type"], "invalidProperties",
        "isDraft revert must be invalidProperties: {resp}"
    );
    assert_eq!(
        err["properties"][0], "isDraft",
        "isDraft must be listed in properties: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test: Task/query against MemoryBackend with a non-trivial filter fails
// loud rather than silently returning all ids.
//
// Oracle: RFC 8620 §5.5 — "A server MUST reject the call with an
// `unsupportedFilter` error if it cannot process the given filter."
// The reference MemoryBackend cannot process any filter, so a query
// carrying one must surface a method-level error rather than silently
// matching all objects. The reference impl uses `serverFail` as a
// workspace-canonical approximation (a richer error variant on
// JmapBackend would be needed to plumb `unsupportedFilter` proper).
//
// Bead: JMAP-h47t.5.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_query_with_filter_fails_loud_on_memory_backend() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));
    backend.seed_object("acc1", "Task", "t1", task_fixture("t1", "tl1", "Buy milk"));

    let args = json!({
        "accountId": "acc1",
        "filter": { "taskListId": "tl1" }
    });
    let err = handle_task_query(&backend, &(), args)
        .await
        .expect_err("Task/query with filter must fail loud on the reference MemoryBackend");
    assert_eq!(
        err.error_type.as_str(),
        "serverFail",
        "fail-loud should surface as serverFail (the workspace-canonical \
         approximation of RFC 8620 §5.5 unsupportedFilter when the backend \
         cannot signal it directly): {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Test: Task/query against MemoryBackend with a non-empty sort fails loud
// rather than silently returning ids in id-lexicographic order.
//
// Oracle: RFC 8620 §5.5 — "A server MUST reject the call with an
// `unsupportedSort` error if it cannot sort by the given properties."
// The reference MemoryBackend cannot honor any sort key, so a query
// carrying one must surface a method-level error rather than silently
// returning ids in id-creation order (which a client following the
// `task<n:016x>` demo id scheme might mistake for working sort).
//
// Bead: JMAP-h47t.5.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_query_with_sort_fails_loud_on_memory_backend() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));
    backend.seed_object("acc1", "Task", "t1", task_fixture("t1", "tl1", "Buy milk"));

    let args = json!({
        "accountId": "acc1",
        "sort": [ { "property": "due", "isAscending": true } ]
    });
    let err = handle_task_query(&backend, &(), args)
        .await
        .expect_err("Task/query with sort must fail loud on the reference MemoryBackend");
    assert_eq!(
        err.error_type.as_str(),
        "serverFail",
        "fail-loud should surface as serverFail (the workspace-canonical \
         approximation of RFC 8620 §5.5 unsupportedSort when the backend \
         cannot signal it directly): {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Test: Task/query with no filter and no sort continues to succeed and
// return all task ids in id-lexicographic order, the documented
// MemoryBackend baseline.
//
// Oracle: the fail-loud change to query_objects must NOT regress the
// trivial-filter case (which is the only path internal consumers and
// integration tests rely on).
//
// Bead: JMAP-h47t.5.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_query_with_no_filter_or_sort_succeeds() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));
    backend.seed_object("acc1", "Task", "t1", task_fixture("t1", "tl1", "Buy milk"));
    backend.seed_object("acc1", "Task", "t2", task_fixture("t2", "tl1", "Walk dog"));

    let args = json!({ "accountId": "acc1", "calculateTotal": true });
    let (resp, _) = handle_task_query(&backend, &(), args)
        .await
        .expect("Task/query with no filter / no sort must succeed");

    let ids = resp["ids"].as_array().expect("ids array present");
    assert_eq!(ids.len(), 2, "two tasks seeded: {resp}");
    assert_eq!(resp["total"], 2);
    // id-lexicographic order; t1 < t2.
    assert_eq!(ids[0], "t1");
    assert_eq!(ids[1], "t2");
}
