//! A sharee whose only write right is `mayUpdatePrivate`.
//!
//! Oracle, jmapio/jmap PR #469 (spec/tasks/list.mdown):
//!
//! > **mayUpdatePrivate**: `Boolean` The user may modify the per-user
//! > properties (see the "Per-user properties" section) on all tasks in the
//! > task list, even if they would not otherwise have permission to modify
//! > that task.
//!
//! and draft-ietf-jmap-tasks-06 §4.5.1, the per-user properties: "keywords,
//! color, freeBusyStatus, useDefaultAlerts, alerts". The sharee here holds
//! neither `mayWriteAll` nor `mayWriteOwn`, so anything else is
//! `forbidden` (RFC 8620 §5.3).

mod common;

use common::{User, UserBackend};
use jmap_server::HandlerOutput;
use jmap_tasks_server::handle_task_set;
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

fn backend() -> UserBackend {
    let backend = UserBackend::new();
    backend.inner.register_account(&Id::from(ACCOUNT));
    backend.inner.seed_object(
        &Id::from(ACCOUNT),
        "Task",
        &Id::from("t1"),
        json!({ "id": "t1", "@type": "Task", "uid": "u1", "title": "x", "taskListId": "tl1",
                "keywords": {} }),
    );
    backend
}

async fn update_as_sharee(backend: &UserBackend, patch: Value) -> Value {
    let args = json!({ "accountId": ACCOUNT, "update": { "t1": patch } });
    let HandlerOutput { primary, .. } = handle_task_set(
        backend,
        &User::PrivateOnlySharee,
        &Default::default(),
        args,
        &[],
    )
    .await
    .expect("Task/set succeeds at method level");
    primary
}

/// Each §4.5.1 per-user property may be patched by the sharee.
#[tokio::test]
async fn sharee_may_patch_each_per_user_property() {
    let patches = [
        json!({ "keywords": { "starred": true } }),
        json!({ "keywords/starred": true }),
        json!({ "color": "red" }),
        json!({ "freeBusyStatus": "free" }),
        json!({ "useDefaultAlerts": true }),
        json!({ "alerts": {} }),
    ];
    for patch in patches {
        let backend = backend();
        let resp = update_as_sharee(&backend, patch.clone()).await;
        assert!(
            resp["updated"].get("t1").is_some(),
            "per-user patch {patch} must be allowed: {resp}"
        );
    }
}

/// A shared property is not the sharee's to change.
#[tokio::test]
async fn sharee_may_not_patch_a_shared_property() {
    let backend = backend();
    let resp = update_as_sharee(&backend, json!({ "title": "renamed" })).await;
    assert_eq!(resp["notUpdated"]["t1"]["type"], "forbidden", "{resp}");
}

/// A per-user change does not carry a shared one with it.
#[tokio::test]
async fn sharee_may_not_patch_per_user_and_shared_together() {
    let backend = backend();
    let resp = update_as_sharee(&backend, json!({ "color": "red", "title": "renamed" })).await;
    assert_eq!(resp["notUpdated"]["t1"]["type"], "forbidden", "{resp}");
}
