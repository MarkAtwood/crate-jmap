//! Integration tests for `jmap-tasks-server` using `MemoryBackend`.
//!
//! All expected values are derived from the spec (draft-ietf-jmap-tasks-06)
//! and RFC 8620, not from the code under test. Wire-shape literals are
//! hand-written from the draft's prose.
//!
//! Bead: JMAP-hwdv.7.

mod common;

use common::MemoryBackend;
use jmap_server::HandlerOutput;
use jmap_tasks_server::{
    handle_task_changes, handle_task_get, handle_task_list_changes, handle_task_list_get,
    handle_task_list_set, handle_task_notification_set, handle_task_query,
    handle_task_query_changes, handle_task_set,
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

    let HandlerOutput { primary: resp, .. } = handle_task_list_get(&backend, &(), args)
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

    let HandlerOutput { primary: resp, .. } = handle_task_list_get(&backend, &(), args)
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
    let HandlerOutput { primary: resp, .. } = handle_task_list_changes(&backend, &(), args)
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
    let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args)
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
    let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args)
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
// Test 5b: TaskList/set onDestroyRemoveTasks=true destroys the list's Tasks
// Oracle: draft-tasks-06 §3.4 — "If true, any Tasks that were in the
// TaskList will be removed from it, and they will be destroyed." RFC 8620
// §5.2: a destroyed record appears in `destroyed` of /changes since a prior
// state. Tasks in other lists are untouched. Bead: JMAP-ji1v.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_set_destroy_remove_tasks_destroys_tasks() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));
    backend.seed_object("acc1", "TaskList", "tl2", task_list_fixture("tl2", "Work"));
    backend.seed_object("acc1", "Task", "t1", task_fixture("t1", "tl1", "Buy milk"));
    backend.seed_object("acc1", "Task", "t2", task_fixture("t2", "tl1", "Buy eggs"));
    backend.seed_object(
        "acc1",
        "Task",
        "t3",
        task_fixture("t3", "tl2", "File report"),
    );

    let HandlerOutput {
        primary: before, ..
    } = handle_task_get(&backend, &(), json!({ "accountId": "acc1", "ids": null }))
        .await
        .expect("Task/get must succeed");
    let task_state = before["state"].clone();

    let args = json!({
        "accountId": "acc1",
        "destroy": ["tl1"],
        "onDestroyRemoveTasks": true
    });
    let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args)
        .await
        .expect("/set must succeed");
    assert_eq!(
        resp["destroyed"],
        json!(["tl1"]),
        "list must be destroyed: {resp}"
    );
    assert!(
        resp["notDestroyed"].is_null(),
        "no destroy failures: {resp}"
    );

    let HandlerOutput { primary: got, .. } = handle_task_get(
        &backend,
        &(),
        json!({ "accountId": "acc1", "ids": ["t1", "t2", "t3"] }),
    )
    .await
    .expect("Task/get must succeed");
    assert_eq!(
        got["notFound"],
        json!(["t1", "t2"]),
        "tl1 tasks destroyed: {got}"
    );
    assert_eq!(got["list"][0]["id"], "t3", "tl2 task survives: {got}");

    let HandlerOutput {
        primary: changes, ..
    } = handle_task_changes(
        &backend,
        &(),
        json!({ "accountId": "acc1", "sinceState": task_state }),
    )
    .await
    .expect("Task/changes must succeed");
    let mut destroyed: Vec<&str> = changes["destroyed"]
        .as_array()
        .expect("destroyed array")
        .iter()
        .map(|v| v.as_str().expect("id string"))
        .collect();
    destroyed.sort_unstable();
    assert_eq!(
        destroyed,
        ["t1", "t2"],
        "Task/changes reports cascade: {changes}"
    );
}

// ---------------------------------------------------------------------------
// Test 5c: a failed onDestroyRemoveTasks cascade leaves the TaskList
// Oracle: draft-tasks-06 §3.4 — "If true, any Tasks that were in the
// TaskList will be removed from it, and they will be destroyed." When the
// Tasks cannot be read the cascade cannot run, so the list is not
// destroyed. RFC 8620 §5.3: `notDestroyed` is "A map of the id to a
// SetError for each record that failed to be destroyed"; `serverFail` is
// a SetError type. The stored Task `t2` has a non-string `title`, so the
// reference backend cannot read it. Bead: JMAP-y3f2.17.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_set_destroy_remove_tasks_failure_keeps_list() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));
    backend.seed_object("acc1", "Task", "t1", task_fixture("t1", "tl1", "Buy milk"));
    backend.seed_object(
        "acc1",
        "Task",
        "t2",
        json!({ "id": "t2", "taskListId": "tl1", "title": 42 }),
    );

    let args = json!({
        "accountId": "acc1",
        "destroy": ["tl1"],
        "onDestroyRemoveTasks": true
    });
    let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args)
        .await
        .expect("/set must succeed");
    assert_eq!(
        resp["notDestroyed"]["tl1"]["type"], "serverFail",
        "cascade failure must be reported: {resp}"
    );
    assert!(
        !resp["destroyed"]
            .as_array()
            .is_some_and(|d| d.contains(&json!("tl1"))),
        "list must not be destroyed: {resp}"
    );

    let HandlerOutput { primary: lists, .. } = handle_task_list_get(
        &backend,
        &(),
        json!({ "accountId": "acc1", "ids": ["tl1"] }),
    )
    .await
    .expect("TaskList/get must succeed");
    assert_eq!(lists["list"][0]["id"], "tl1", "list survives: {lists}");

    let HandlerOutput { primary: got, .. } =
        handle_task_get(&backend, &(), json!({ "accountId": "acc1", "ids": ["t1"] }))
            .await
            .expect("Task/get must succeed");
    assert_eq!(got["list"][0]["id"], "t1", "readable Task survives: {got}");
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
    let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args)
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
    let HandlerOutput { primary: resp, .. } = handle_task_get(&backend, &(), args)
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
    let HandlerOutput { primary: resp, .. } = handle_task_query(&backend, &(), args)
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
    let HandlerOutput { primary: resp, .. } = handle_task_changes(&backend, &(), args)
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
    let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args)
        .await
        .expect("/set must succeed");
    let old_state = resp["oldState"].clone();
    let new_state = resp["newState"].clone();
    assert_ne!(old_state, new_state);

    let args = json!({ "accountId": "acc1", "sinceState": old_state });
    let HandlerOutput {
        primary: changes, ..
    } = handle_task_list_changes(&backend, &(), args)
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
    let HandlerOutput { primary: resp, .. } = handle_task_set(&backend, &(), args)
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
// This test exercises Task/set end-to-end. The handler does not pre-check
// isDraft, so the rejection comes from the backend (bd:JMAP-h47t.29).
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
    let HandlerOutput { primary: resp, .. } = handle_task_set(&backend, &(), args)
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

// Oracle: draft-ietf-jmap-tasks-06 §4 (isDraft paragraph) — "This may only
// be set to true upon creation." A Task stored without isDraft is not a
// draft, so a patch to true is invalidProperties from the backend.
#[tokio::test]
async fn task_set_isdraft_true_on_absent_rejected_by_memory_backend() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "Task",
        "t1",
        json!({
            "id": "t1",
            "@type": "Task",
            "uid": "t1-uid",
            "title": "Task without isDraft"
        }),
    );

    let args = json!({
        "accountId": "acc1",
        "update": { "t1": { "isDraft": true } }
    });
    let HandlerOutput { primary: resp, .. } = handle_task_set(&backend, &(), args)
        .await
        .expect("/set must succeed at the top level");

    let err = &resp["notUpdated"]["t1"];
    assert_eq!(err["type"], "invalidProperties", "absent -> true: {resp}");
    assert_eq!(err["properties"], json!(["isDraft"]), "{resp}");
}

// ---------------------------------------------------------------------------
// Test: Task/query against MemoryBackend with a non-trivial filter fails
// loud rather than silently returning all ids.
//
// Oracle: RFC 8620 §5.5 — "A server MUST reject the call with an
// `unsupportedFilter` error if it cannot process the given filter."
// The reference MemoryBackend cannot process this filter, so a query
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
        "filter": { "title": "milk" }
    });
    let err = handle_task_query(&backend, &(), args)
        .await
        .expect_err("Task/query with filter must fail loud on the reference MemoryBackend");
    assert_eq!(
        err.error_type.to_string(),
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
        err.error_type.to_string(),
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
    let HandlerOutput { primary: resp, .. } = handle_task_query(&backend, &(), args)
        .await
        .expect("Task/query with no filter / no sort must succeed");

    let ids = resp["ids"].as_array().expect("ids array present");
    assert_eq!(ids.len(), 2, "two tasks seeded: {resp}");
    assert_eq!(resp["total"], 2);
    // id-lexicographic order; t1 < t2.
    assert_eq!(ids[0], "t1");
    assert_eq!(ids[1], "t2");
}

// ---------------------------------------------------------------------------
// Test: Task/queryChanges honours the `taskListId` filter of the query.
//
// Oracle: RFC 8620 §5.6 — `removed`: "The "id" for every Foo that was in
// the query results in the old state and that is not in the results in
// the new state."; `added`: "The id and index in the query results (in
// the new state) for every Foo that has been added to the results since
// the old state". draft-tasks-06 §4: "taskListId: Id The TaskList id this
// task belongs to." Moving t1 out of tl1 removes it from the tl1 query,
// moving t2 in adds it at index 0 (the only tl1 task), and a task created
// in tl2 is not part of the tl1 results. `removed` also lists t2: "If the
// "filter" or "sort" includes a mutable property, the server MUST include
// all Foos in the current results for which this property may have
// changed", and `added` reinserts it. Beads: JMAP-y3f2.14, JMAP-gev1.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_query_changes_honours_task_list_filter() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "TaskList", "tl1", task_list_fixture("tl1", "Todo"));
    backend.seed_object("acc1", "TaskList", "tl2", task_list_fixture("tl2", "Work"));
    backend.seed_object("acc1", "Task", "t1", task_fixture("t1", "tl1", "Buy milk"));
    backend.seed_object(
        "acc1",
        "Task",
        "t2",
        task_fixture("t2", "tl2", "File report"),
    );

    let filter = json!({ "taskListId": "tl1" });
    let HandlerOutput { primary: query, .. } = handle_task_query(
        &backend,
        &(),
        json!({ "accountId": "acc1", "filter": filter }),
    )
    .await
    .expect("Task/query must succeed");
    assert_eq!(query["ids"], json!(["t1"]), "{query}");

    let HandlerOutput { primary: set, .. } = handle_task_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "update": {
                "t1": { "taskListId": "tl2" },
                "t2": { "taskListId": "tl1" }
            },
            "create": {
                "c3": { "@type": "Task", "uid": "c3-uid", "title": "Call", "taskListId": "tl2" }
            }
        }),
    )
    .await
    .expect("Task/set must succeed");
    assert!(set["notUpdated"].is_null(), "{set}");
    assert!(set["notCreated"].is_null(), "{set}");

    let HandlerOutput {
        primary: changes, ..
    } = handle_task_query_changes(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "filter": filter,
            "sinceQueryState": query["queryState"]
        }),
    )
    .await
    .expect("Task/queryChanges must succeed");
    assert_eq!(changes["removed"], json!(["t1", "t2"]), "{changes}");
    assert_eq!(
        changes["added"],
        json!([{ "id": "t2", "index": 0 }]),
        "{changes}"
    );
}
