//! Filter conditions reject keys they do not define.
//!
//! Oracle: RFC 8620 §5.5 — "unsupportedFilter: The "filter" is
//! syntactically valid, but the server cannot process it." A key the
//! condition type does not define must fail the decode instead of becoming
//! an all-`None` condition that matches every record; servers map the
//! failure to `unsupportedFilter`. An operator object is not a
//! FilterCondition, so a bare condition type must reject it too. Inputs are
//! hand-written JSON; control keys are the property names the cited spec
//! defines.

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn assert_strict<T: DeserializeOwned>(known: Value, typo: Value) {
    if let Err(e) = serde_json::from_value::<T>(known.clone()) {
        panic!("{known} must decode: {e}");
    }
    for bad in [
        typo,
        json!({ "bogus": 1 }),
        json!({ "operator": "AND", "conditions": [] }),
    ] {
        assert!(
            serde_json::from_value::<T>(bad.clone()).is_err(),
            "{bad} must be rejected"
        );
    }
}

/// Task property `taskListId` (draft-ietf-jmap-tasks-06 §4).
#[test]
fn task_filter_condition_is_strict() {
    assert_strict::<jmap_tasks_types::TaskFilterCondition>(
        json!({ "taskListId": "l1" }),
        json!({ "task_list_id": "l1" }),
    );
}

/// draft-ietf-jmap-tasks-06 §5.5 TaskNotification/query FilterCondition:
/// `type`, `taskIds`.
#[test]
fn task_notification_filter_condition_is_strict() {
    assert_strict::<jmap_tasks_types::TaskNotificationFilterCondition>(
        json!({ "type": "created", "taskIds": ["t1"] }),
        json!({ "task_ids": ["t1"] }),
    );
}
