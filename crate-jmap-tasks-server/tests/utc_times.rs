//! `Task/get` `utcStart` / `utcDue` (draft-ietf-jmap-tasks-06 §4 as amended
//! by jmapio/jmap PR #464).
//!
//! Oracle, PR #464 (spec/tasks/task.mdown):
//!
//! > **utcStart**: `UTCDate|null` The task's "start" converted to UTC using
//! > its time zone, or null if "start" is not set.
//!
//! > **utcDue**: `UTCDate|null` The task's "due" converted to UTC using its
//! > time zone, or null if "due" is not set. ... Floating tasks will be
//! > interpreted as per the time zone given as a Task/get argument.
//!
//! and -06 §4: "This is not included by default and must be requested
//! explicitly." The `UserBackend` test zone `Test/Plus2` is a fixed +02:00
//! offset, so 09:00 there is 07:00Z by arithmetic, not by the code under
//! test.

mod common;

use common::{User, UserBackend};
use jmap_server::HandlerOutput;
use jmap_tasks_server::handle_task_get;
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

fn backend_with(task: Value) -> UserBackend {
    let backend = UserBackend::new();
    backend.inner.register_account(&Id::from(ACCOUNT));
    backend
        .inner
        .seed_object(&Id::from(ACCOUNT), "Task", &Id::from("t1"), task);
    backend
}

async fn get(backend: &UserBackend, args: Value) -> Value {
    let HandlerOutput { primary, .. } = handle_task_get(backend, &User::Owner, args, &[])
        .await
        .expect("Task/get succeeds");
    primary["list"][0].clone()
}

fn utc_args(extra: Value) -> Value {
    let mut args = json!({
        "accountId": ACCOUNT,
        "ids": ["t1"],
        "properties": ["id", "utcStart", "utcDue"]
    });
    if let (Some(a), Some(e)) = (args.as_object_mut(), extra.as_object()) {
        a.extend(e.clone());
    }
    args
}

/// A zoned task converts with its own time zone.
#[tokio::test]
async fn zoned_task_converts_with_its_time_zone() {
    let backend = backend_with(json!({
        "id": "t1", "@type": "Task", "uid": "u1", "title": "x", "taskListId": "tl1",
        "timeZone": "Test/Plus2",
        "start": "2026-03-01T09:00:00", "due": "2026-03-01T17:00:00"
    }));
    let task = get(&backend, utc_args(json!({}))).await;
    assert_eq!(
        task,
        json!({ "id": "t1", "utcStart": "2026-03-01T07:00:00Z", "utcDue": "2026-03-01T15:00:00Z" })
    );
}

/// A floating task with no Task/get `timeZone` argument is read in
/// `Etc/UTC`, the CalendarEvent/get default (draft-ietf-jmap-calendars-34
/// §5.7 "timeZone: TimeZoneId (default: "Etc/UTC")").
#[tokio::test]
async fn floating_task_defaults_to_utc() {
    let backend = backend_with(json!({
        "id": "t1", "@type": "Task", "uid": "u1", "title": "x", "taskListId": "tl1",
        "start": "2026-03-01T09:00:00"
    }));
    let task = get(&backend, utc_args(json!({}))).await;
    assert_eq!(task["utcStart"], json!("2026-03-01T09:00:00Z"));
}

/// A task without `start` / `due` gets `null`, not an omitted key.
#[tokio::test]
async fn absent_start_and_due_are_null() {
    let backend = backend_with(json!({
        "id": "t1", "@type": "Task", "uid": "u1", "title": "x", "taskListId": "tl1"
    }));
    let task = get(&backend, utc_args(json!({}))).await;
    assert_eq!(
        task,
        json!({ "id": "t1", "utcStart": null, "utcDue": null })
    );
}

/// Only the absent one is null: `due` set, `start` not.
#[tokio::test]
async fn absent_start_is_null_while_due_converts() {
    let backend = backend_with(json!({
        "id": "t1", "@type": "Task", "uid": "u1", "title": "x", "taskListId": "tl1",
        "timeZone": "Etc/UTC", "due": "2026-03-01T17:00:00"
    }));
    let task = get(&backend, utc_args(json!({}))).await;
    assert_eq!(task["utcStart"], Value::Null);
    assert_eq!(task["utcDue"], json!("2026-03-01T17:00:00Z"));
}

/// -06 §4: "not included by default": a `null` `properties` leaves both
/// keys out.
#[tokio::test]
async fn default_properties_leave_utc_out() {
    let backend = backend_with(json!({
        "id": "t1", "@type": "Task", "uid": "u1", "title": "x", "taskListId": "tl1"
    }));
    let task = get(
        &backend,
        json!({ "accountId": ACCOUNT, "ids": ["t1"], "properties": null }),
    )
    .await;
    assert!(task.get("utcStart").is_none(), "{task}");
    assert!(task.get("utcDue").is_none(), "{task}");
}
