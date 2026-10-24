//! `/set` update re-validates the patched object before storing it.
//!
//! Oracle: RFC 8620 §5.3: "If null, set to the default value if specified
//! for this property; otherwise, remove the property from the patched
//! object." Removing a property the draft marks as required yields an
//! invalid object, which RFC 8620 §5.3 rejects with "invalidProperties: The
//! PatchObject or Foo object is invalid in some way". Each test quotes the
//! draft-ietf-jmap-tasks-06 line that makes the property required.

mod common;

use common::MemoryBackend;
use jmap_server::HandlerOutput;
use jmap_tasks_server::{
    handle_task_get, handle_task_list_get, handle_task_list_set, handle_task_set,
};
use jmap_types::Id;
use serde_json::json;

/// draft-ietf-jmap-tasks-06 line 397: "*name*: String The user-visible name
/// of the task list." No default is specified, so `null` removes it.
#[tokio::test]
async fn task_list_null_name_rejected_and_get_still_succeeds() {
    let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
    backend.seed_object(
        &Id::from("acc1"),
        "TaskList",
        &Id::from("tl1"),
        json!({
            "id": "tl1",
            "name": "Chores",
            "sortOrder": 0,
            "isSubscribed": true,
            "myRights": {
                "mayReadItems": true,
                "mayWriteAll": true,
                "mayWriteOwn": true,
                "mayUpdatePrivate": true,
                "mayRSVP": true,
                "mayAdmin": true,
                "mayDelete": true
            }
        }),
    );

    let set_args = json!({ "accountId": "acc1", "update": { "tl1": { "name": null } } });
    let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), set_args, &[])
        .await
        .expect("/set must not return a top-level error");
    assert_eq!(
        resp["notUpdated"]["tl1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["tl1"]["properties"],
        json!(["name"]),
        "{resp}"
    );

    let get_args = json!({ "accountId": "acc1", "ids": ["tl1"] });
    let HandlerOutput { primary: after, .. } = handle_task_list_get(&backend, &(), get_args, &[])
        .await
        .expect("/get must not return a top-level error");
    assert_eq!(after["list"][0]["name"], "Chores", "{after}");
}

/// draft-ietf-jmap-tasks-06 line 901: "*message*: String (mandatory).  The
/// free text value of this comment."
#[tokio::test]
async fn task_null_comment_message_rejected_and_get_still_succeeds() {
    let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
    backend.seed_object(
        &Id::from("acc1"),
        "Task",
        &Id::from("t1"),
        json!({
            "id": "t1",
            "@type": "Task",
            "uid": "t1-uid",
            "title": "Task",
            "comments": { "c1": { "@type": "Comment", "message": "hello" } }
        }),
    );

    let set_args = json!({
        "accountId": "acc1",
        "update": { "t1": { "comments/c1/message": null } }
    });
    let HandlerOutput { primary: resp, .. } = handle_task_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        set_args,
        &[],
    )
    .await
    .expect("/set must not return a top-level error");
    assert_eq!(
        resp["notUpdated"]["t1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["t1"]["properties"],
        json!(["comments"]),
        "{resp}"
    );

    let get_args = json!({ "accountId": "acc1", "ids": ["t1"] });
    let HandlerOutput { primary: after, .. } = handle_task_get(&backend, &(), get_args, &[])
        .await
        .expect("/get must not return a top-level error");
    assert_eq!(
        after["list"][0]["comments"]["c1"]["message"], "hello",
        "{after}"
    );
}

/// draft-ietf-jmap-tasks-06 line 733: "*isDraft*: Boolean ... This may only
/// be set to true upon creation.  Once set to false, the value cannot be
/// updated to true." Clearing it with `null` first must not open a path
/// back to true.
#[tokio::test]
async fn task_is_draft_null_then_true_rejected() {
    let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
    backend.seed_object(
        &Id::from("acc1"),
        "Task",
        &Id::from("t1"),
        json!({
            "id": "t1",
            "@type": "Task",
            "uid": "t1-uid",
            "title": "Published task",
            "isDraft": false
        }),
    );

    let clear = json!({ "accountId": "acc1", "update": { "t1": { "isDraft": null } } });
    let HandlerOutput { primary: resp, .. } = handle_task_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        clear,
        &[],
    )
    .await
    .expect("/set must not return a top-level error");
    assert!(resp["updated"].get("t1").is_some(), "{resp}");

    let revert = json!({ "accountId": "acc1", "update": { "t1": { "isDraft": true } } });
    let HandlerOutput { primary: resp, .. } = handle_task_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        revert,
        &[],
    )
    .await
    .expect("/set must not return a top-level error");
    assert_eq!(
        resp["notUpdated"]["t1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["t1"]["properties"],
        json!(["isDraft"]),
        "{resp}"
    );
}
