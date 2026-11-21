//! The Task `workflowStatus` and `progress` rule (docs/design/DECISIONS.md
//! decision 42; jmapio/jmap #465 against draft-ietf-jmap-tasks-06 §4).
//!
//! -06 says only "Must be one of the `TaskList`'s `workflowStatuses`. If set,
//! `progress` MUST null", which leaves a task completed through
//! `workflowStatus` looking unfinished to a JSCalendar or iCalendar reader.
//! Decision 42 replaces it:
//!
//! - A `workflowStatus` that is one of the RFC 8984 §5.2.5 progress values
//!   is mirrored into `progress` by the server; any other value (`pending`,
//!   a list-defined one) leaves `progress` `null`. A changed `progress`,
//!   including one the mirror clears, also sets `progressUpdated` (RFC
//!   8984 §5.2.6) to the current time, unless the client set it.
//! - Setting `progress` to anything else while `workflowStatus` is set is
//!   `invalidProperties` naming `progress`.
//! - A `workflowStatus` not in the task list's `workflowStatuses` is
//!   `invalidProperties` naming `workflowStatus`, on create, when the
//!   update sets it, and when the update moves the task to another list.
//! - Changing a list's `workflowStatuses` does not touch its tasks: an
//!   update that neither sets `workflowStatus` nor moves the task is not
//!   checked against the list.
//!
//! [`apply_workflow_status`] is the whole rule, on the Task as it would be
//! stored. A [`TasksBackend`](crate::TasksBackend) runs it in the same
//! critical section as the write, so a concurrent change of the list's
//! `workflowStatuses` cannot slip a value past it.

use jmap_types::{SetError, SetErrorType};
use serde_json::{Map, Value};

/// draft-ietf-jmap-tasks-06 §3: a `TaskList`'s `workflowStatuses` default,
/// used when the list has none.
pub const DEFAULT_WORKFLOW_STATUSES: &[&str] = &[
    "completed",
    "failed",
    "in-process",
    "needs-action",
    "cancelled",
    "pending",
];

/// The RFC 8984 §5.2.5 `progress` values that a `workflowStatus` mirrors.
pub const PROGRESS_VALUES: &[&str] = &[
    "needs-action",
    "in-process",
    "completed",
    "failed",
    "cancelled",
];

fn invalid(property: &str, description: &str) -> Box<SetError> {
    Box::new(
        SetError::new(SetErrorType::InvalidProperties)
            .with_properties([property])
            .with_description(description),
    )
}

/// How a write reached [`apply_workflow_status`].
#[derive(Debug, Clone, Copy)]
pub enum WorkflowWrite<'a> {
    /// A create (or a `Task/copy` into the account): every rule applies.
    Create,
    /// An update of `before`, whose top-level patched properties are
    /// `written`.
    Update {
        /// The stored Task.
        before: &'a Map<String, Value>,
        /// The top-level properties the patch names.
        written: &'a [String],
    },
}

/// Apply decision 42 to `task`, the Task as it would be stored, in a list
/// whose `workflowStatuses` is `list_statuses` (`None`: the list has none,
/// or none is stored, so [`DEFAULT_WORKFLOW_STATUSES`]). `now` is the
/// `UTCDate` a server-changed `progress` gives `progressUpdated`.
///
/// Returns whether the server changed `task` (`progress` or
/// `progressUpdated`), so an update can echo it (RFC 8620 §5.3).
///
/// # Errors
///
/// `invalidProperties` naming `workflowStatus` or `progress`, as listed in
/// the module docs.
pub fn apply_workflow_status(
    task: &mut Map<String, Value>,
    write: WorkflowWrite<'_>,
    list_statuses: Option<&[String]>,
    now: &str,
) -> Result<bool, Box<SetError>> {
    let wrote_progress_updated = match write {
        WorkflowWrite::Create => task_has(task, "progressUpdated"),
        WorkflowWrite::Update { written, .. } => written.iter().any(|w| w == "progressUpdated"),
    };
    let (sets_status, sets_progress, moves) = match write {
        WorkflowWrite::Create => (true, task_has(task, "progress"), false),
        WorkflowWrite::Update { before, written } => (
            written.iter().any(|w| w == "workflowStatus"),
            written.iter().any(|w| w == "progress"),
            before.get("taskListId") != task.get("taskListId"),
        ),
    };
    let Some(status) = task
        .get("workflowStatus")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return Ok(false);
    };
    if sets_status || moves {
        let allowed = match list_statuses {
            Some(list) => list.contains(&status),
            None => DEFAULT_WORKFLOW_STATUSES.contains(&status.as_str()),
        };
        if !allowed {
            return Err(invalid(
                "workflowStatus",
                "workflowStatus is not one of the task list's workflowStatuses",
            ));
        }
    }
    if !sets_status && !sets_progress {
        return Ok(false);
    }
    let mirrored = PROGRESS_VALUES
        .contains(&status.as_str())
        .then(|| Value::String(status.clone()));
    let given = task.get("progress").filter(|v| !v.is_null()).cloned();
    if sets_progress && given != mirrored {
        return Err(invalid(
            "progress",
            "progress must match workflowStatus: the workflowStatus when it is a progress value, otherwise null",
        ));
    }
    let previous = match write {
        WorkflowWrite::Create => None,
        WorkflowWrite::Update { before, .. } => before.get("progress").filter(|v| !v.is_null()),
    };
    let mut changed = given != mirrored;
    match &mirrored {
        Some(v) => {
            task.insert("progress".to_owned(), v.clone());
        }
        None => {
            task.remove("progress");
        }
    }
    // "Whenever the server changes `progress` this way" (local spec
    // {#workflow} rule 1; decision 42 "as for any `progress` change"):
    // clearing it, e.g. `completed` to `pending`, is such a change too.
    if previous != mirrored.as_ref() && !wrote_progress_updated {
        task.insert("progressUpdated".to_owned(), Value::String(now.to_owned()));
        changed = true;
    }
    Ok(changed)
}

fn task_has(task: &Map<String, Value>, k: &str) -> bool {
    task.get(k).is_some_and(|v| !v.is_null())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: &str = "2026-11-19T12:00:00Z";

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    /// Decision 42: a progress value mirrors; `progressUpdated` is set.
    #[test]
    fn a_progress_value_mirrors() {
        let mut t = map(json!({ "taskListId": "L", "workflowStatus": "completed" }));
        assert!(apply_workflow_status(&mut t, WorkflowWrite::Create, None, NOW).unwrap());
        assert_eq!(t["progress"], "completed");
        assert_eq!(t["progressUpdated"], NOW);
    }

    /// "pending" is a default workflowStatus but not an RFC 8984 progress
    /// value: `progress` stays null.
    #[test]
    fn pending_leaves_progress_null() {
        let mut t = map(json!({ "taskListId": "L", "workflowStatus": "pending" }));
        assert!(!apply_workflow_status(&mut t, WorkflowWrite::Create, None, NOW).unwrap());
        assert!(t.get("progress").is_none());
    }

    /// The same value given for both is not a conflict.
    #[test]
    fn matching_progress_is_accepted() {
        let mut t = map(json!({ "workflowStatus": "in-process", "progress": "in-process" }));
        apply_workflow_status(&mut t, WorkflowWrite::Create, None, NOW).unwrap();
    }

    /// An update that touches neither workflowStatus nor taskListId is
    /// not checked against the list (its statuses may have changed).
    #[test]
    fn an_unrelated_update_skips_the_list_check() {
        let before = map(json!({ "taskListId": "L", "workflowStatus": "in-review" }));
        let mut t = map(json!({ "taskListId": "L", "workflowStatus": "in-review", "title": "x" }));
        let written = vec!["title".to_owned()];
        let w = WorkflowWrite::Update {
            before: &before,
            written: &written,
        };
        assert!(!apply_workflow_status(&mut t, w, Some(&[]), NOW).unwrap());
    }
}
