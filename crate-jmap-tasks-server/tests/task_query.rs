//! Task/query per jmapio/jmap #456, following CalendarEvent/query.
//!
//! Oracles:
//!
//! - jmapio/jmap #456: "inTaskLists: Id[]; after / before: ..., on due (and
//!   start); progress: String; workflowStatus: String; text / title:
//!   String ... and Task/query expandRecurrences with the same before/after
//!   requirement and tooLarge error as CalendarEvent/query."
//! - draft-ietf-jmap-calendars-34 §5.11.1: "after: LocalDateTime The end of
//!   the event ... must be after this date"; "before: LocalDateTime The
//!   start of the event ... must be before this date". For a task the
//!   span runs from `start` to `due` (TaskFilterCondition docs).
//! - draft-ietf-jmap-calendars-34 §5.11 `expandRecurrences`: "If true, the
//!   filter MUST be just a FilterCondition (not a FilterOperator) and MUST
//!   include both a "before" and "after" property; otherwise, the server
//!   MUST reject the call with an invalidArguments error", and
//!   "expandDurationTooLarge: ... the duration between the "before" and
//!   "after" properties exceeds the maxExpandedQueryDuration limit".
//!
//! Fixtures are hand-written; expected ids follow from the quoted rules.

mod common;

use common::{USER_BACKEND_MAX_EXPAND_SECONDS, User, UserBackend};
use jmap_server::{BackendQueryError, HandlerOutput, QueryParams};
use jmap_tasks_server::{TaskQueryArgs, TasksBackend, handle_task_query};
use jmap_tasks_types::{Filter, TaskFilter};
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

/// A backend holding `tasks`, each `(id, extra properties)`, in list tl1
/// unless the extras name another.
fn backend(tasks: &[(&str, Value)]) -> UserBackend {
    let backend = UserBackend::new();
    backend.inner.register_account(&Id::from(ACCOUNT));
    for (id, extra) in tasks {
        let mut task = json!({
            "id": id, "@type": "Task", "uid": format!("{id}-uid"), "title": id,
            "taskListId": "tl1"
        });
        if let (Some(t), Some(e)) = (task.as_object_mut(), extra.as_object()) {
            t.extend(e.clone());
        }
        backend
            .inner
            .seed_object(&Id::from(ACCOUNT), "Task", &Id::from(*id), task);
    }
    backend
}

async fn query(backend: &UserBackend, extra: Value) -> Result<Value, JmapError> {
    let mut args = json!({ "accountId": ACCOUNT });
    if let (Some(a), Some(e)) = (args.as_object_mut(), extra.as_object()) {
        a.extend(e.clone());
    }
    handle_task_query(backend, &User::Owner, args, &[])
        .await
        .map(|HandlerOutput { primary, .. }| primary["ids"].clone())
}

async fn ids(backend: &UserBackend, filter: Value) -> Value {
    query(backend, json!({ "filter": filter }))
        .await
        .expect("Task/query succeeds")
}

/// `inTaskLists` matches a task in any of the given lists.
#[tokio::test]
async fn in_task_lists_matches_any_listed_list() {
    let b = backend(&[
        ("a", json!({ "taskListId": "tl1" })),
        ("b", json!({ "taskListId": "tl2" })),
        ("c", json!({ "taskListId": "tl3" })),
    ]);
    assert_eq!(
        ids(&b, json!({ "inTaskLists": ["tl1", "tl3"] })).await,
        json!(["a", "c"])
    );
}

/// `workflowStatus` matches the task's `workflowStatus`.
#[tokio::test]
async fn workflow_status_matches_exactly() {
    let b = backend(&[
        ("a", json!({ "workflowStatus": "review" })),
        ("b", json!({ "workflowStatus": "done" })),
        ("c", json!({})),
    ]);
    assert_eq!(
        ids(&b, json!({ "workflowStatus": "review" })).await,
        json!(["a"])
    );
}

/// `progress` matches the task's `progress`.
#[tokio::test]
async fn progress_matches_exactly() {
    let b = backend(&[
        ("a", json!({ "progress": "completed" })),
        ("b", json!({ "progress": "in-process" })),
    ]);
    assert_eq!(
        ids(&b, json!({ "progress": "completed" })).await,
        json!(["a"])
    );
}

/// `after` tests the end of the task, its `due`: due after the bound
/// matches, due before it does not, whatever the `start`.
#[tokio::test]
async fn after_tests_due() {
    let b = backend(&[
        (
            "late",
            json!({ "start": "2026-01-01T09:00:00", "due": "2026-03-10T09:00:00" }),
        ),
        (
            "early",
            json!({ "start": "2026-01-01T09:00:00", "due": "2026-02-10T09:00:00" }),
        ),
    ]);
    assert_eq!(
        ids(&b, json!({ "after": "2026-03-01T00:00:00" })).await,
        json!(["late"])
    );
}

/// `after` uses `start` for a task without `due`.
#[tokio::test]
async fn after_falls_back_to_start() {
    let b = backend(&[
        ("late", json!({ "start": "2026-03-10T09:00:00" })),
        ("early", json!({ "start": "2026-02-10T09:00:00" })),
    ]);
    assert_eq!(
        ids(&b, json!({ "after": "2026-03-01T00:00:00" })).await,
        json!(["late"])
    );
}

/// `before` tests the beginning of the task, its `start`: a task that
/// starts before the bound matches even when it is due after it.
#[tokio::test]
async fn before_tests_start() {
    let b = backend(&[
        (
            "spans",
            json!({ "start": "2026-02-10T09:00:00", "due": "2026-04-10T09:00:00" }),
        ),
        (
            "later",
            json!({ "start": "2026-03-10T09:00:00", "due": "2026-04-10T09:00:00" }),
        ),
    ]);
    assert_eq!(
        ids(&b, json!({ "before": "2026-03-01T00:00:00" })).await,
        json!(["spans"])
    );
}

/// `before` uses `due` for a task without `start`.
#[tokio::test]
async fn before_falls_back_to_due() {
    let b = backend(&[
        ("soon", json!({ "due": "2026-02-10T09:00:00" })),
        ("later", json!({ "due": "2026-03-10T09:00:00" })),
    ]);
    assert_eq!(
        ids(&b, json!({ "before": "2026-03-01T00:00:00" })).await,
        json!(["soon"])
    );
}

/// A task with neither `start` nor `due` has no time to test, so it
/// matches neither `after` nor `before`.
#[tokio::test]
async fn untimed_task_matches_neither_bound() {
    let b = backend(&[("untimed", json!({}))]);
    assert_eq!(
        ids(&b, json!({ "after": "2000-01-01T00:00:00" })).await,
        json!([])
    );
    assert_eq!(
        ids(&b, json!({ "before": "2100-01-01T00:00:00" })).await,
        json!([])
    );
}

/// The Task/query `timeZone` argument reaches the backend: a task in
/// that zone is compared in it.
#[tokio::test]
async fn time_zone_argument_reaches_backend() {
    let b = backend(&[(
        "zoned",
        json!({ "timeZone": "Test/Plus2", "due": "2026-03-10T09:00:00" }),
    )]);
    let got = query(
        &b,
        json!({ "timeZone": "Test/Plus2", "filter": { "after": "2026-03-01T00:00:00" } }),
    )
    .await
    .expect("Task/query succeeds");
    assert_eq!(got, json!(["zoned"]));
}

/// A `timeZone` that is not a string is `invalidArguments`.
#[tokio::test]
async fn time_zone_must_be_a_string() {
    let b = backend(&[]);
    let err = query(&b, json!({ "timeZone": 5 }))
        .await
        .expect_err("non-string timeZone");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}

/// `expandRecurrences` without both bounds is `invalidArguments`.
#[tokio::test]
async fn expand_recurrences_needs_before_and_after() {
    let b = backend(&[]);
    for filter in [
        Value::Null,
        json!({ "after": "2026-03-01T00:00:00" }),
        json!({ "before": "2026-03-01T00:00:00" }),
    ] {
        let err = query(&b, json!({ "expandRecurrences": true, "filter": filter }))
            .await
            .expect_err("expandRecurrences without both bounds");
        assert_eq!(err.error_type.to_string(), "invalidArguments", "{filter}");
    }
}

/// `expandRecurrences` with a FilterOperator is `invalidArguments`.
#[tokio::test]
async fn expand_recurrences_needs_a_filter_condition() {
    let b = backend(&[]);
    let filter = json!({ "operator": "AND", "conditions": [
        { "after": "2026-03-01T00:00:00", "before": "2026-03-01T12:00:00" }
    ] });
    let err = query(&b, json!({ "expandRecurrences": true, "filter": filter }))
        .await
        .expect_err("expandRecurrences with an operator");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}

/// A window longer than the account's `maxExpandedQueryDuration` (the
/// test backend's one day) is `expandDurationTooLarge`.
#[tokio::test]
async fn expand_window_over_cap_is_expand_duration_too_large() {
    assert_eq!(USER_BACKEND_MAX_EXPAND_SECONDS, 86_400);
    let b = backend(&[]);
    let err = query(
        &b,
        json!({ "expandRecurrences": true, "filter": {
            "after": "2026-03-01T00:00:00", "before": "2026-03-03T00:00:00"
        } }),
    )
    .await
    .expect_err("two days over a one-day cap");
    assert_eq!(err.error_type.to_string(), "expandDurationTooLarge");
}

/// A window within the cap is accepted, and a non-recurring task is its
/// own single instance.
#[tokio::test]
async fn expand_window_within_cap_returns_instances() {
    let b = backend(&[("a", json!({ "start": "2026-03-01T09:00:00" }))]);
    let got = query(
        &b,
        json!({ "expandRecurrences": true, "filter": {
            "after": "2026-03-01T00:00:00", "before": "2026-03-01T12:00:00"
        } }),
    )
    .await
    .expect("half a day within a one-day cap");
    assert_eq!(got, json!(["a"]));
}

/// The backend re-checks the cap itself (TasksBackend::query_tasks:
/// "the backend is canonical and MUST check it again"): the memory backend
/// refuses a window over its own limit even when no handler checked it.
#[tokio::test]
async fn backend_rechecks_expand_cap() {
    let b = backend(&[]);
    let cap = b.inner.limits(&(), &Id::from(ACCOUNT));
    let filter: TaskFilter = Filter::Condition(
        serde_json::from_value(json!({
            "after": "2026-01-01T00:00:00", "before": "2028-01-01T00:00:00"
        }))
        .expect("condition"),
    );
    // Two years is more than the default cap, whatever its exact value
    // within one year.
    assert!(cap.max_expanded_query_duration_seconds < 2 * 365 * 86_400);
    let err = b
        .inner
        .query_tasks(
            &(),
            &Id::from(ACCOUNT),
            QueryParams::new().filter(Some(&filter)),
            &TaskQueryArgs::default().with_expand_recurrences(true),
        )
        .await
        .expect_err("over the backend cap");
    match err {
        BackendQueryError::Method(e) => {
            assert_eq!(e.error_type.to_string(), "expandDurationTooLarge");
        }
        other => panic!("expected a method error, got {other:?}"),
    }
}

/// The memory backend has no recurrence engine, so a recurring match is
/// `cannotCalculateOccurrences` (calendars-34 §5.11: "The server cannot
/// expand a recurrence required to return the results for this query").
#[tokio::test]
async fn recurring_match_without_engine_cannot_calculate_occurrences() {
    let b = backend(&[(
        "weekly",
        json!({
            "start": "2026-03-01T09:00:00",
            "recurrenceRules": [{ "@type": "RecurrenceRule", "frequency": "weekly" }]
        }),
    )]);
    let err = query(
        &b,
        json!({ "expandRecurrences": true, "filter": {
            "after": "2026-03-01T00:00:00", "before": "2026-03-01T12:00:00"
        } }),
    )
    .await
    .expect_err("recurring task cannot be expanded");
    assert_eq!(err.error_type.to_string(), "cannotCalculateOccurrences");
}

// ─── Sorting ────────────────────────────────────────────────────────────────
//
// docs/specs/jmap-tasks-local.md Sorting, after draft-ietf-jmap-calendars-34
// §5.11.2 (calendars-34.txt L2372-2391): "The following properties MUST be
// supported for sorting: `id`, `uid`, `start`. The following SHOULD be
// supported: `created`, `updated`. Sorting by `start` orders tasks by the
// absolute time they start, not by the lexical value of `start`; a floating
// task's start is read in the `timeZone` argument." RFC 8620 §5.5:
// "isAscending: Boolean (optional; default: true) If true, sort in
// ascending order.  If false, reverse the comparator's results to sort in
// descending order."

async fn sorted(backend: &UserBackend, sort: Value) -> Value {
    query(backend, json!({ "sort": sort }))
        .await
        .expect("Task/query succeeds")
}

/// Sort by `id`, both directions.
#[tokio::test]
async fn sorts_by_id() {
    let b = backend(&[("b", json!({})), ("c", json!({})), ("a", json!({}))]);
    assert_eq!(
        sorted(&b, json!([{ "property": "id" }])).await,
        json!(["a", "b", "c"])
    );
    assert_eq!(
        sorted(&b, json!([{ "property": "id", "isAscending": false }])).await,
        json!(["c", "b", "a"])
    );
}

/// Sort by `uid`, which here orders the tasks differently from their ids.
#[tokio::test]
async fn sorts_by_uid() {
    let b = backend(&[
        ("a", json!({ "uid": "zebra" })),
        ("b", json!({ "uid": "apple" })),
        ("c", json!({ "uid": "mango" })),
    ]);
    assert_eq!(
        sorted(&b, json!([{ "property": "uid" }])).await,
        json!(["b", "c", "a"])
    );
}

/// Sort by `start` for tasks in one zone; floating tasks are read in the
/// `timeZone` argument, so with it equal to the zoned task's zone all three
/// compare directly.
#[tokio::test]
async fn sorts_by_start() {
    let b = backend(&[
        ("a", json!({ "start": "2026-03-10T09:00:00" })),
        (
            "b",
            json!({ "start": "2026-03-01T09:00:00", "timeZone": "Test/Plus2" }),
        ),
        ("c", json!({ "start": "2026-03-05T09:00:00" })),
    ]);
    let got = query(
        &b,
        json!({ "timeZone": "Test/Plus2", "sort": [{ "property": "start" }] }),
    )
    .await
    .expect("Task/query succeeds");
    assert_eq!(got, json!(["b", "c", "a"]));
}

/// The memory backend has no time-zone database, so it cannot order starts
/// in different zones by absolute time; RFC 8620 §5.5: "A server MUST
/// reject the call with an unsupportedSort error if it cannot sort by the
/// given properties."
#[tokio::test]
async fn start_sort_across_zones_is_unsupported_sort() {
    let b = backend(&[
        ("a", json!({ "start": "2026-03-10T09:00:00" })),
        (
            "b",
            json!({ "start": "2026-03-01T09:00:00", "timeZone": "Test/Plus2" }),
        ),
    ]);
    let err = query(&b, json!({ "sort": [{ "property": "start" }] }))
        .await
        .expect_err("cannot sort across zones");
    assert_eq!(err.error_type.to_string(), "unsupportedSort");
}

/// Sort by `created` and `updated` (UTCDateTime, compared as instants).
#[tokio::test]
async fn sorts_by_created_and_updated() {
    let b = backend(&[
        (
            "a",
            json!({ "created": "2026-01-03T00:00:00Z", "updated": "2026-02-01T00:00:00Z" }),
        ),
        (
            "b",
            json!({ "created": "2026-01-01T00:00:00Z", "updated": "2026-02-01T00:00:00.5Z" }),
        ),
        (
            "c",
            json!({ "created": "2026-01-02T00:00:00Z", "updated": "2026-01-15T00:00:00Z" }),
        ),
    ]);
    assert_eq!(
        sorted(&b, json!([{ "property": "created" }])).await,
        json!(["b", "c", "a"])
    );
    // 00:00:00.5Z is half a second after 00:00:00Z.
    assert_eq!(
        sorted(&b, json!([{ "property": "updated" }])).await,
        json!(["c", "a", "b"])
    );
}

/// A later comparator breaks ties of an earlier one (RFC 8620 §5.5: "If
/// two Foo records have an identical value for the first comparator, the
/// next comparator will be considered, and so on.").
#[tokio::test]
async fn second_comparator_breaks_ties() {
    let b = backend(&[
        ("a", json!({ "uid": "same" })),
        ("b", json!({ "uid": "same" })),
        ("c", json!({ "uid": "first" })),
    ]);
    assert_eq!(
        sorted(
            &b,
            json!([{ "property": "uid" }, { "property": "id", "isAscending": false }])
        )
        .await,
        json!(["c", "b", "a"])
    );
}

/// -26's `recurrenceId`, dropped by -34, and an unlisted property are
/// `unsupportedSort`.
#[tokio::test]
async fn unlisted_sort_properties_are_unsupported_sort() {
    let b = backend(&[("a", json!({}))]);
    for property in ["recurrenceId", "title"] {
        let err = query(&b, json!({ "sort": [{ "property": property }] }))
            .await
            .expect_err("unlisted sort property");
        assert_eq!(err.error_type.to_string(), "unsupportedSort", "{property}");
    }
}
