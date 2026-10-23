//! RFC 8620 §1.2 wire-id validation on `jmap-tasks-server` `/set` handlers
//! (bd JMAP-k9va), mirroring the canonical
//! `crate-jmap-mail-server/tests/integration.rs`
//! `mailbox_set_and_query_reject_malformed_ids`.
//!
//! Oracle: RFC 8620 §1.2, an Id "MUST only contain characters from the
//! 'URL and Filename Safe' base64 alphabet". `"bad id"` contains a space,
//! so a `/set` `update` key or `destroy` element naming it fails that
//! entry with `invalidProperties` naming `id`, written out by hand below.
//! A Task create or patch whose `taskListId` (an Id, draft-tasks-06 §4)
//! is `"bad id"` fails with `invalidProperties` naming `taskListId`.

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_tasks_server::{handle_task_list_set, handle_task_set};
use jmap_types::Id;
use serde_json::{Value, json};

/// Asserts the hand-written RFC 8620 §5.3 SetError for a malformed id.
fn assert_invalid_id(entry: &Value, resp: &Value) {
    assert_invalid_property(entry, resp, "id");
}

fn assert_invalid_property(entry: &Value, resp: &Value, property: &str) {
    assert_eq!(entry["type"], "invalidProperties", "{resp}");
    assert_eq!(entry["properties"], json!([property]), "{resp}");
}

#[tokio::test]
async fn task_list_set_rejects_malformed_update_and_destroy_ids() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    backend.register_account(&account_id);
    let args_with_account = |mut args: Value| {
        args["accountId"] = json!(account_id.as_ref());
        args
    };
    let args = json!({
        "update": { "bad id": { "name": "x" } },
        "destroy": ["bad id"]
    });
    let resp = handle_task_list_set(&backend, &(), args_with_account(args), &[])
        .await
        .expect("/set must not fail as a method")
        .primary;
    assert_invalid_id(&resp["notUpdated"]["bad id"], &resp);
    assert_invalid_id(&resp["notDestroyed"]["bad id"], &resp);
}

#[tokio::test]
async fn task_set_rejects_malformed_task_list_id() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    backend.register_account(&account_id);
    let args = json!({
        "accountId": account_id.as_ref(),
        "create": { "c1": { "title": "x", "taskListId": "bad id" } },
        "update": { "t1": { "taskListId": "bad id" } }
    });
    let resp = handle_task_set(&backend, &(), &Default::default(), args, &[])
        .await
        .expect("/set must not fail as a method")
        .primary;
    assert_invalid_property(&resp["notCreated"]["c1"], &resp, "taskListId");
    assert_invalid_property(&resp["notUpdated"]["t1"], &resp, "taskListId");
}
