//! Integration tests for jmap-tasks-types.
//!
//! Test oracles are hand-written JSON fixtures grounded directly in:
//!   - RFC 8984 §6.2 (Simple Task) and §6.5 (Task with a Due Date)
//!   - draft-ietf-jmap-tasks-06 §3 (TaskList), §4 (Task), §5 (TaskNotification)
//!
//! No expected values are derived from the code under test.

use jmap_tasks_types::{
    NotificationType, PrincipalTasksCapability, Task, TaskFilterCondition, TaskList, TaskListRole,
    TaskNotification, TaskNotificationFilterCondition, TaskProgress, TaskRights,
    TasksAccountCapability, TasksAlertsAccountCapability, TasksAlertsCapability,
    TasksAssigneesAccountCapability, TasksAssigneesCapability, TasksCapability,
    TasksCustomTimeZonesAccountCapability, TasksCustomTimeZonesCapability,
    TasksMultilingualAccountCapability, TasksMultilingualCapability,
    TasksRecurrencesAccountCapability, TasksRecurrencesCapability,
};

// ─── TaskList ────────────────────────────────────────────────────────────────

/// Deserialize a full TaskList from hand-written JSON and verify key fields.
#[test]
fn tasklist_deserialize_full() {
    let json = include_str!("fixtures/tasklist_full.json");
    let tl: TaskList = serde_json::from_str(json).expect("TaskList deserialize");

    assert_eq!(tl.id.as_ref(), "abc123");
    assert_eq!(tl.name, "My Tasks");
    assert_eq!(tl.description.as_deref(), Some("Personal task list"));
    assert_eq!(tl.color.as_deref(), Some("#4287f5"));
    assert!(tl.is_subscribed);
    assert_eq!(tl.sort_order, 0);
    assert_eq!(tl.time_zone.as_deref(), Some("America/New_York"));

    let kc = tl.keyword_colors.as_ref().expect("keywordColors");
    assert_eq!(kc.get("urgent").map(String::as_str), Some("#ff0000"));

    let rights = &tl.my_rights;
    assert!(rights.may_read_items);
    assert!(rights.may_write_all);
    assert!(rights.may_admin);
    assert!(rights.may_delete);

    let share = tl.share_with.as_ref().expect("shareWith");
    let sharee_rights = share.values().next().expect("sharee");
    assert!(sharee_rights.may_read_items);
    assert!(!sharee_rights.may_write_all);

    let wf = tl.workflow_statuses.as_ref().expect("workflowStatuses");
    assert!(wf.contains(&"completed".to_string()));
    assert!(wf.contains(&"pending".to_string()));
}

/// Round-trip: serialize then re-deserialize and compare to original.
#[test]
fn tasklist_roundtrip() {
    let json = include_str!("fixtures/tasklist_full.json");
    let original: TaskList = serde_json::from_str(json).expect("first deserialize");
    let serialized = serde_json::to_string(&original).expect("serialize");
    let recovered: TaskList = serde_json::from_str(&serialized).expect("second deserialize");
    assert_eq!(original, recovered);
}

/// A minimal TaskList (only required fields) deserializes without error.
#[test]
fn tasklist_minimal() {
    let json = r#"{
        "id": "minid",
        "name": "Minimal List",
        "sortOrder": 0,
        "isSubscribed": false,
        "myRights": {
            "mayReadItems": false,
            "mayWriteAll": false,
            "mayWriteOwn": false,
            "mayUpdatePrivate": false,
            "mayRSVP": false,
            "mayAdmin": false,
            "mayDelete": false
        }
    }"#;
    let tl: TaskList = serde_json::from_str(json).expect("minimal TaskList");
    assert_eq!(tl.name, "Minimal List");
    assert!(tl.role.is_none());
    assert!(tl.description.is_none());
    assert!(tl.share_with.is_none());
}

// ─── TaskListRole ────────────────────────────────────────────────────────────

/// Known role values deserialize to their typed variants.
#[test]
fn task_list_role_known_values() {
    let inbox: TaskListRole = serde_json::from_str(r#""inbox""#).expect("inbox");
    let trash: TaskListRole = serde_json::from_str(r#""trash""#).expect("trash");

    assert_eq!(inbox, TaskListRole::Inbox);
    assert_eq!(trash, TaskListRole::Trash);
}

/// An unknown role string maps to `Other(String)` and round-trips.
#[test]
fn task_list_role_other_roundtrip() {
    let other: TaskListRole = serde_json::from_str(r#""archive""#).expect("other role");
    assert_eq!(other, TaskListRole::Other("archive".to_string()));

    let serialized = serde_json::to_string(&other).expect("serialize");
    assert_eq!(serialized, r#""archive""#);
}

/// Known roles serialize to their spec-mandated wire strings.
#[test]
fn task_list_role_wire_strings() {
    assert_eq!(
        serde_json::to_string(&TaskListRole::Inbox).unwrap(),
        r#""inbox""#
    );
    assert_eq!(
        serde_json::to_string(&TaskListRole::Trash).unwrap(),
        r#""trash""#
    );
}

// ─── TaskRights ──────────────────────────────────────────────────────────────

/// `mayRSVP` round-trips with the correct camelCase wire name.
#[test]
fn task_rights_may_rsvp_wire_name() {
    let json = r#"{
        "mayReadItems": false,
        "mayWriteAll": false,
        "mayWriteOwn": false,
        "mayUpdatePrivate": false,
        "mayRSVP": true,
        "mayAdmin": false,
        "mayDelete": false
    }"#;
    let rights: TaskRights = serde_json::from_str(json).expect("TaskRights");
    assert!(rights.may_rsvp);

    let out = serde_json::to_string(&rights).expect("serialize");
    assert!(
        out.contains("\"mayRSVP\":true"),
        "wire name must be mayRSVP, got: {out}"
    );
}

// ─── Task ─────────────────────────────────────────────────────────────────────

/// RFC 8984 §6.2: Simple Task.  Verbatim from the spec.
#[test]
fn task_simple_rfc8984_example() {
    // RFC 8984 §6.2 example (trimmed to only spec-defined fields):
    let json = r#"{
        "@type": "Task",
        "uid": "2a358cee-6489-4f14-a57f-c104db4dc2f2",
        "updated": "2020-01-09T14:32:01Z",
        "title": "Do something"
    }"#;
    let task: Task = serde_json::from_str(json).expect("simple Task");
    assert_eq!(task.at_type.as_deref(), Some("Task"));
    assert_eq!(
        task.uid.as_deref(),
        Some("2a358cee-6489-4f14-a57f-c104db4dc2f2")
    );
    assert_eq!(task.updated.as_deref(), Some("2020-01-09T14:32:01Z"));
    assert_eq!(task.title.as_deref(), Some("Do something"));
}

/// RFC 8984 §6.5: Task with a Due Date.  Verbatim from the spec.
#[test]
fn task_due_date_rfc8984_example() {
    let json = include_str!("fixtures/task_due_date.json");
    let task: Task = serde_json::from_str(json).expect("due-date Task");
    assert_eq!(task.title.as_deref(), Some("Buy groceries"));
    assert_eq!(task.due.as_deref(), Some("2020-01-19T18:00:00"));
    assert_eq!(task.time_zone.as_deref(), Some("Europe/Vienna"));
    assert_eq!(task.estimated_duration.as_deref(), Some("PT1H"));
    assert_eq!(task.progress, Some(TaskProgress::NeedsAction));
}

/// Task with JMAP-specific fields (draft-tasks-06 §4).
#[test]
fn task_jmap_fields() {
    let json = include_str!("fixtures/task_simple.json");
    let task: Task = serde_json::from_str(json).expect("JMAP Task");
    assert_eq!(task.id.as_ref().map(|id| id.as_ref()), Some("taskid001"));
    assert_eq!(
        task.task_list_id.as_ref().map(|id| id.as_ref()),
        Some("abc123")
    );
}

/// Default-constructed Task is all `None`; serializes to `{}`.
#[test]
fn task_default_is_empty() {
    let task = Task::default();
    let json = serde_json::to_string(&task).expect("serialize");
    assert_eq!(json, "{}");
}

/// Task with `keywords` map deserializes correctly.
#[test]
fn task_keywords() {
    let json = r#"{
        "keywords": {
            "work": true,
            "urgent": true
        }
    }"#;
    let task: Task = serde_json::from_str(json).expect("Task with keywords");
    let kw = task.keywords.as_ref().expect("keywords");
    assert_eq!(kw.get("work"), Some(&true));
    assert_eq!(kw.get("urgent"), Some(&true));
}

/// Task round-trip.
#[test]
fn task_roundtrip() {
    let json = include_str!("fixtures/task_due_date.json");
    let original: Task = serde_json::from_str(json).expect("first deserialize");
    let serialized = serde_json::to_string(&original).expect("serialize");
    let recovered: Task = serde_json::from_str(&serialized).expect("second deserialize");
    assert_eq!(original, recovered);
}

// ─── TaskProgress ────────────────────────────────────────────────────────────

/// All RFC 8984 §5.2.5 progress values deserialize correctly.
#[test]
fn task_progress_all_values() {
    let cases = [
        (r#""needs-action""#, TaskProgress::NeedsAction),
        (r#""in-process""#, TaskProgress::InProcess),
        (r#""completed""#, TaskProgress::Completed),
        (r#""failed""#, TaskProgress::Failed),
        (r#""cancelled""#, TaskProgress::Cancelled),
    ];
    for (json, expected) in &cases {
        let got: TaskProgress = serde_json::from_str(json).unwrap();
        assert_eq!(&got, expected, "parsing {json}");
    }
}

/// An unknown progress string maps to `Other` and round-trips.
#[test]
fn task_progress_other_roundtrip() {
    let other: TaskProgress = serde_json::from_str(r#""in-review""#).expect("other progress");
    assert_eq!(other, TaskProgress::Other("in-review".to_string()));
    let out = serde_json::to_string(&other).unwrap();
    assert_eq!(out, r#""in-review""#);
}

// ─── TaskFilterCondition ─────────────────────────────────────────────────────

/// An empty filter condition deserializes from `{}` and serializes back to `{}`.
#[test]
fn task_filter_condition_empty_defaults() {
    let json = "{}";
    let cond: TaskFilterCondition = serde_json::from_str(json).expect("empty filter");
    assert!(cond.task_list_id.is_none());
    assert!(cond.uid.is_none());
    assert!(cond.has_keyword.is_none());
    assert!(cond.not_keyword.is_none());
    assert!(cond.text.is_none());
    assert!(cond.before.is_none());
    assert!(cond.after.is_none());
    assert!(cond.is_draft.is_none());
    assert!(cond.progress.is_none());
    // Serializing back should produce `{}`
    let out = serde_json::to_string(&cond).expect("serialize");
    assert_eq!(out, "{}");
}

/// A populated filter condition round-trips correctly.
#[test]
fn task_filter_condition_populated() {
    let json = r#"{
        "taskListId": "list001",
        "isDraft": false,
        "progress": "needs-action",
        "after": "2024-01-01T00:00:00Z",
        "before": "2025-01-01T00:00:00Z"
    }"#;
    let cond: TaskFilterCondition = serde_json::from_str(json).expect("filter");
    assert_eq!(
        cond.task_list_id.as_ref().map(|id| id.as_ref()),
        Some("list001")
    );
    assert_eq!(cond.is_draft, Some(false));
    assert_eq!(cond.progress.as_deref(), Some("needs-action"));
    assert_eq!(cond.after.as_deref(), Some("2024-01-01T00:00:00Z"));
    assert_eq!(cond.before.as_deref(), Some("2025-01-01T00:00:00Z"));
}

// ─── TaskNotification ────────────────────────────────────────────────────────

/// Deserialize a TaskNotification from hand-written JSON.
#[test]
fn notification_deserialize() {
    let json = include_str!("fixtures/notification.json");
    let notif: TaskNotification = serde_json::from_str(json).expect("notification");
    assert_eq!(notif.id.as_ref(), "notif001");
    assert_eq!(notif.created, "2020-01-10T08:00:00Z");
    assert_eq!(notif.changed_by.at_type, "Person");
    assert_eq!(notif.changed_by.name.as_deref(), Some("Alice"));
    assert_eq!(notif.notification_type, NotificationType::Updated);
    assert_eq!(notif.task_id.as_ref(), "taskid001");
    assert_eq!(notif.is_draft, Some(false));
    assert!(notif.task.is_some());
    assert!(notif.task_patch.is_none());
}

/// `NotificationType` known values deserialize correctly.
#[test]
fn notification_type_known_values() {
    let cases = [
        (r#""created""#, NotificationType::Created),
        (r#""updated""#, NotificationType::Updated),
        (r#""destroyed""#, NotificationType::Destroyed),
    ];
    for (json, expected) in &cases {
        let got: NotificationType = serde_json::from_str(json).unwrap();
        assert_eq!(&got, expected, "parsing {json}");
    }
}

/// `NotificationType` unknown value maps to `Other` and round-trips.
#[test]
fn notification_type_other_roundtrip() {
    let other: NotificationType = serde_json::from_str(r#""transferred""#).expect("other type");
    assert_eq!(other, NotificationType::Other("transferred".to_string()));
    let out = serde_json::to_string(&other).unwrap();
    assert_eq!(out, r#""transferred""#);
}

/// The wire field name for `notification_type` is `"type"`.
#[test]
fn notification_type_wire_field_name() {
    // Build a TaskNotification via JSON so we don't need a struct literal
    // (the struct is #[non_exhaustive] and Id doesn't implement FromStr).
    let json = r#"{
        "id": "n1",
        "created": "2024-01-01T00:00:00Z",
        "changedBy": { "@type": "Person" },
        "type": "created",
        "taskId": "t1"
    }"#;
    let notif: TaskNotification = serde_json::from_str(json).expect("parse");
    let out = serde_json::to_string(&notif).expect("serialize");
    assert!(
        out.contains("\"type\":\"created\""),
        "wire field should be 'type', got: {out}"
    );
}

/// `TaskNotificationFilterCondition` empty defaults.
#[test]
fn notification_filter_empty_defaults() {
    let cond = TaskNotificationFilterCondition::default();
    assert!(cond.after.is_none());
    assert!(cond.before.is_none());
    assert!(cond.notification_type.is_none());
    assert!(cond.task_ids.is_none());
    let out = serde_json::to_string(&cond).expect("serialize");
    assert_eq!(out, "{}");
}

// ─── Capability ──────────────────────────────────────────────────────────────

/// `TasksCapability` serializes to an empty object and round-trips.
#[test]
fn tasks_capability_empty_object() {
    let cap = TasksCapability::default();
    let json = serde_json::to_string(&cap).expect("serialize");
    assert_eq!(json, "{}");
    let recovered: TasksCapability = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(cap, recovered);
}

/// `TasksAccountCapability` round-trips correctly.
#[test]
fn tasks_account_capability_roundtrip() {
    let json = include_str!("fixtures/capability.json");
    let cap: TasksAccountCapability = serde_json::from_str(json).expect("account capability");
    assert_eq!(cap.min_date_time, "1970-01-01T00:00:00");
    assert_eq!(cap.max_date_time, "2100-12-31T23:59:59");
    assert!(cap.may_create_task_list);

    let out = serde_json::to_string(&cap).expect("serialize");
    let recovered: TasksAccountCapability = serde_json::from_str(&out).expect("re-deserialize");
    assert_eq!(cap, recovered);
}

/// Capability URI constants have the expected values.
#[test]
fn capability_uri_constants() {
    assert_eq!(
        jmap_tasks_types::JMAP_TASKS_URI,
        "urn:ietf:params:jmap:tasks"
    );
    assert_eq!(
        jmap_tasks_types::JMAP_TASKS_RECURRENCES_URI,
        "urn:ietf:params:jmap:tasks:recurrences"
    );
    assert_eq!(
        jmap_tasks_types::JMAP_TASKS_ASSIGNEES_URI,
        "urn:ietf:params:jmap:tasks:assignees"
    );
    assert_eq!(
        jmap_tasks_types::JMAP_TASKS_ALERTS_URI,
        "urn:ietf:params:jmap:tasks:alerts"
    );
    assert_eq!(
        jmap_tasks_types::JMAP_TASKS_MULTILINGUAL_URI,
        "urn:ietf:params:jmap:tasks:multilingual"
    );
    assert_eq!(
        jmap_tasks_types::JMAP_TASKS_CUSTOMTIMEZONES_URI,
        "urn:ietf:params:jmap:tasks:customtimezones"
    );
}

// ─── Account-level extension capability structs (Task 1: JMAP-d8e7.1) ────────

/// `TasksAlertsAccountCapability` serializes to `{}` and deserializes from `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.4 — account capability is an empty object.
#[test]
fn tasks_alerts_account_capability_empty_object() {
    let cap = TasksAlertsAccountCapability::default();
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert_eq!(out, serde_json::json!({}), "must be empty JSON object");
    let _: TasksAlertsAccountCapability =
        serde_json::from_str("{}").expect("must deserialize from empty object");
}

/// `TasksMultilingualAccountCapability` serializes to `{}` and deserializes from `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.5 — account capability is an empty object.
#[test]
fn tasks_multilingual_account_capability_empty_object() {
    let cap = TasksMultilingualAccountCapability::default();
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert_eq!(out, serde_json::json!({}), "must be empty JSON object");
    let _: TasksMultilingualAccountCapability =
        serde_json::from_str("{}").expect("must deserialize from empty object");
}

/// `TasksCustomTimeZonesAccountCapability` serializes to `{}` and deserializes from `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.6 — account capability is an empty object.
#[test]
fn tasks_custom_time_zones_account_capability_empty_object() {
    let cap = TasksCustomTimeZonesAccountCapability::default();
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert_eq!(out, serde_json::json!({}), "must be empty JSON object");
    let _: TasksCustomTimeZonesAccountCapability =
        serde_json::from_str("{}").expect("must deserialize from empty object");
}

// ─── PrincipalTasksCapability (Task 2: JMAP-d8e7.2) ──────────────────────────

/// `account_id: None` serializes as `"accountId": null` (required-nullable).
///
/// Oracle: draft-ietf-jmap-tasks-06 §2.1 — accountId is Id|null, always present.
#[test]
fn principal_tasks_capability_account_id_null_serializes() {
    // Construct via deserialization (struct is #[non_exhaustive]).
    let cap: PrincipalTasksCapability =
        serde_json::from_str(r#"{"accountId":null,"mayShareWith":false}"#)
            .expect("must deserialize");
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert!(
        out["accountId"].is_null(),
        "accountId must be null when None, got: {out}"
    );
}

/// `send_to: None` must NOT produce a `"sendTo"` key in the output.
///
/// Oracle: draft-ietf-jmap-tasks-06 §2.1 — sendTo is optional (null or absent).
#[test]
fn principal_tasks_capability_send_to_absent_when_none() {
    // Construct via deserialization (struct is #[non_exhaustive]).
    let cap: PrincipalTasksCapability =
        serde_json::from_str(r#"{"accountId":null,"mayShareWith":true}"#)
            .expect("must deserialize");
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert!(
        out.get("sendTo").is_none(),
        "sendTo must be absent when None, got: {out}"
    );
}

/// Full round-trip: all three fields present, serialized and re-deserialized.
///
/// Oracle: draft-ietf-jmap-tasks-06 §2.1 field definitions.
#[test]
fn principal_tasks_capability_roundtrip_full() {
    let json = r#"{
        "accountId": "acc001",
        "mayShareWith": true,
        "sendTo": {
            "imip": "mailto:alice@example.com"
        }
    }"#;
    let cap: PrincipalTasksCapability =
        serde_json::from_str(json).expect("must deserialize full capability");
    assert_eq!(
        cap.account_id.as_ref().map(|id| id.as_ref()),
        Some("acc001")
    );
    assert!(cap.may_share_with);
    let send_to = cap.send_to.as_ref().expect("sendTo must be present");
    assert_eq!(
        send_to.get("imip").map(String::as_str),
        Some("mailto:alice@example.com")
    );

    // Re-serialize and re-deserialize; must round-trip.
    let out = serde_json::to_string(&cap).expect("must serialize");
    let recovered: PrincipalTasksCapability =
        serde_json::from_str(&out).expect("must re-deserialize");
    assert_eq!(cap, recovered);
}

// ─── Session-level empty capability structs ───────────────────────────────────

/// `TasksAlertsCapability` (session-level) serializes to `{}` and deserializes from `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.4 — session capability is an empty object.
#[test]
fn tasks_alerts_capability_empty_object_serialize() {
    let cap = TasksAlertsCapability::default();
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert_eq!(out, serde_json::json!({}), "must be empty JSON object");
}

/// `TasksAlertsCapability` deserializes from `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.4 — session capability is an empty object.
#[test]
fn tasks_alerts_capability_empty_object_deserialize() {
    let _: TasksAlertsCapability =
        serde_json::from_str("{}").expect("must deserialize from empty object");
}

/// `TasksMultilingualCapability` (session-level) serializes to `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.5 — session capability is an empty object.
#[test]
fn tasks_multilingual_capability_empty_object_serialize() {
    let cap = TasksMultilingualCapability::default();
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert_eq!(out, serde_json::json!({}), "must be empty JSON object");
}

/// `TasksMultilingualCapability` deserializes from `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.5 — session capability is an empty object.
#[test]
fn tasks_multilingual_capability_empty_object_deserialize() {
    let _: TasksMultilingualCapability =
        serde_json::from_str("{}").expect("must deserialize from empty object");
}

/// `TasksCustomTimeZonesCapability` (session-level) serializes to `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.6 — session capability is an empty object.
#[test]
fn tasks_custom_time_zones_capability_empty_object_serialize() {
    let cap = TasksCustomTimeZonesCapability::default();
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert_eq!(out, serde_json::json!({}), "must be empty JSON object");
}

/// `TasksCustomTimeZonesCapability` deserializes from `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.6 — session capability is an empty object.
#[test]
fn tasks_custom_time_zones_capability_empty_object_deserialize() {
    let _: TasksCustomTimeZonesCapability =
        serde_json::from_str("{}").expect("must deserialize from empty object");
}

/// `TasksRecurrencesCapability` (session-level) serializes to `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.2 — session capability is an empty object.
#[test]
fn tasks_recurrences_capability_empty_object_serialize() {
    let cap = TasksRecurrencesCapability::default();
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert_eq!(out, serde_json::json!({}), "must be empty JSON object");
}

/// `TasksRecurrencesCapability` deserializes from `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.2 — session capability is an empty object.
#[test]
fn tasks_recurrences_capability_empty_object_deserialize() {
    let _: TasksRecurrencesCapability =
        serde_json::from_str("{}").expect("must deserialize from empty object");
}

/// `TasksAssigneesCapability` (session-level) serializes to `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.3 — session capability is an empty object.
#[test]
fn tasks_assignees_capability_empty_object_serialize() {
    let cap = TasksAssigneesCapability::default();
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert_eq!(out, serde_json::json!({}), "must be empty JSON object");
}

/// `TasksAssigneesCapability` deserializes from `{}`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.3 — session capability is an empty object.
#[test]
fn tasks_assignees_capability_empty_object_deserialize() {
    let _: TasksAssigneesCapability =
        serde_json::from_str("{}").expect("must deserialize from empty object");
}

// ─── Account-level non-empty extension capability structs ─────────────────────

/// `TasksRecurrencesAccountCapability` serializes with `maxExpandedQueryDuration`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.2 — account capability has one required field.
#[test]
fn tasks_recurrences_account_capability_serialize() {
    let cap: TasksRecurrencesAccountCapability =
        serde_json::from_str(r#"{"maxExpandedQueryDuration":"P1Y"}"#).expect("must deserialize");
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert_eq!(out["maxExpandedQueryDuration"], "P1Y");
}

/// `TasksRecurrencesAccountCapability` deserializes and round-trips.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.2 — maxExpandedQueryDuration is an ISO 8601 Duration.
#[test]
fn tasks_recurrences_account_capability_roundtrip() {
    // Hand-written fixture: P365D is a valid ISO 8601 duration.
    let json = r#"{"maxExpandedQueryDuration":"P365D"}"#;
    let cap: TasksRecurrencesAccountCapability =
        serde_json::from_str(json).expect("must deserialize");
    assert_eq!(cap.max_expanded_query_duration, "P365D");
    let out = serde_json::to_string(&cap).expect("must serialize");
    let recovered: TasksRecurrencesAccountCapability =
        serde_json::from_str(&out).expect("must re-deserialize");
    assert_eq!(
        cap.max_expanded_query_duration,
        recovered.max_expanded_query_duration
    );
}

/// `TasksAssigneesAccountCapability` serializes `maxParticipantsPerTask` as `null` when `None`.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.3 — maxParticipantsPerTask is UnsignedInt|null.
#[test]
fn tasks_assignees_account_capability_null_participants_serialize() {
    let cap: TasksAssigneesAccountCapability =
        serde_json::from_str(r#"{"maxParticipantsPerTask":null}"#).expect("must deserialize");
    assert!(cap.max_participants_per_task.is_none());
    let out = serde_json::to_value(&cap).expect("must serialize");
    assert!(
        out["maxParticipantsPerTask"].is_null(),
        "maxParticipantsPerTask must serialize as null when None, got: {out}"
    );
}

/// `TasksAssigneesAccountCapability` deserializes and round-trips with a concrete value.
///
/// Oracle: draft-ietf-jmap-tasks-06 §1.6.3 — maxParticipantsPerTask is UnsignedInt|null.
#[test]
fn tasks_assignees_account_capability_roundtrip() {
    // Hand-written fixture: server allows up to 50 participants per task.
    let json = r#"{"maxParticipantsPerTask":50}"#;
    let cap: TasksAssigneesAccountCapability =
        serde_json::from_str(json).expect("must deserialize");
    assert_eq!(cap.max_participants_per_task, Some(50));
    let out = serde_json::to_string(&cap).expect("must serialize");
    let recovered: TasksAssigneesAccountCapability =
        serde_json::from_str(&out).expect("must re-deserialize");
    assert_eq!(
        cap.max_participants_per_task,
        recovered.max_participants_per_task
    );
}
