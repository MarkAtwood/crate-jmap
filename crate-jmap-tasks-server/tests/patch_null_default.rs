//! `TaskList/set` update with a `null` PatchObject value resets the
//! property to its spec default.
//!
//! Oracle: RFC 8620 §5.3: "If null, set to the default value if specified
//! for this property; otherwise, remove the property from the patched
//! object." The test quotes the draft-ietf-jmap-tasks-06 line that
//! specifies the default it expects.

mod common;

use common::MemoryBackend;
use jmap_server::HandlerOutput;
use jmap_tasks_server::{handle_task_list_get, handle_task_list_set};
use jmap_types::Id;
use serde_json::json;

/// draft-ietf-jmap-tasks-06 line 429:
/// "*sortOrder*: UnsignedInt (default: 0)".
#[tokio::test]
async fn task_list_sort_order_null_resets_to_zero() {
    let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
    backend.seed_object(
        &Id::from("acc1"),
        "TaskList",
        &Id::from("tl1"),
        json!({
            "id": "tl1",
            "name": "Chores",
            "sortOrder": 7,
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
    let get_args = json!({ "accountId": "acc1", "ids": ["tl1"] });
    let HandlerOutput {
        primary: before, ..
    } = handle_task_list_get(&backend, &(), get_args.clone(), &[])
        .await
        .expect("/get must not return a top-level error");
    assert_eq!(before["list"][0]["sortOrder"], 7, "{before}");

    let set_args = json!({ "accountId": "acc1", "update": { "tl1": { "sortOrder": null } } });
    let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), set_args, &[])
        .await
        .expect("/set must not return a top-level error");
    assert!(resp["updated"].get("tl1").is_some(), "{resp}");

    let HandlerOutput { primary: after, .. } = handle_task_list_get(&backend, &(), get_args, &[])
        .await
        .expect("/get must not return a top-level error");
    assert_eq!(after["list"][0]["sortOrder"], 0, "{after}");
}
