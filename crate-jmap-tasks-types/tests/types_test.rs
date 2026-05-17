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
    assert_eq!(
        task.updated.as_ref().map(AsRef::as_ref),
        Some("2020-01-09T14:32:01Z")
    );
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

/// `recurrenceOverrides` round-trips as a `HashMap<String, PatchObject>`
/// without altering wire shape — proves `PatchObject`'s
/// `#[serde(transparent)]` attribute is in effect.
///
/// Oracle: hand-written JSON modelled on RFC 8984 §4.3.3 recurrenceOverrides
/// (LocalDateTime keys → PatchObject values).
///
/// Uses a single outer key so the outer `HashMap`'s iteration order is
/// irrelevant. The inner `PatchObject` wraps a `serde_json::Map`, which —
/// without the `preserve_order` feature on `serde_json` — is a `BTreeMap`
/// and emits keys in alphabetical order. The fixture is therefore written
/// with alphabetically-sorted inner keys so a byte-equal round-trip is
/// deterministic.
#[test]
fn task_recurrence_overrides_patch_object_transparent() {
    let json = r#"{"recurrenceOverrides":{"2024-03-15T09:00:00":{"start":"2024-03-15T10:00:00","title":"Rescheduled"}}}"#;
    let task: Task = serde_json::from_str(json).expect("deserialize");

    // Deserialized into typed PatchObject, accessible via .as_map().
    let overrides = task
        .recurrence_overrides
        .as_ref()
        .expect("recurrenceOverrides present");
    let patch = overrides
        .get("2024-03-15T09:00:00")
        .expect("override key present");
    assert_eq!(
        patch.as_map().get("title").and_then(|v| v.as_str()),
        Some("Rescheduled")
    );
    assert_eq!(
        patch.as_map().get("start").and_then(|v| v.as_str()),
        Some("2024-03-15T10:00:00")
    );

    // Re-serialize and compare to input byte-for-byte. This proves
    // #[serde(transparent)] is doing its job — PatchObject does not
    // introduce any wrapper key on the wire.
    let serialized = serde_json::to_string(&task).expect("serialize");
    assert_eq!(serialized, json);
}

/// `localizations` round-trips as a `HashMap<String, PatchObject>`.
///
/// Oracle: hand-written JSON modelled on RFC 8984 §4.6.1 localizations
/// (BCP 47 language tag keys → PatchObject values). Inner keys are written
/// in alphabetical order so the byte-equal round-trip is deterministic
/// (see `task_recurrence_overrides_patch_object_transparent` for the
/// underlying reason).
#[test]
fn task_localizations_patch_object_transparent() {
    let json = r#"{"localizations":{"de":{"description":"Beschreibung","title":"Aufgabe"}}}"#;
    let task: Task = serde_json::from_str(json).expect("deserialize");

    let locs = task.localizations.as_ref().expect("localizations present");
    let de = locs.get("de").expect("de locale present");
    assert_eq!(
        de.as_map().get("title").and_then(|v| v.as_str()),
        Some("Aufgabe")
    );
    assert_eq!(
        de.as_map().get("description").and_then(|v| v.as_str()),
        Some("Beschreibung")
    );

    let serialized = serde_json::to_string(&task).expect("serialize");
    assert_eq!(serialized, json);
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
    assert_eq!(
        cond.after.as_ref().map(AsRef::as_ref),
        Some("2024-01-01T00:00:00Z")
    );
    assert_eq!(
        cond.before.as_ref().map(AsRef::as_ref),
        Some("2025-01-01T00:00:00Z")
    );
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

/// Structural round-trip of the `notification.json` fixture: every typed
/// field deserializes and re-serializes back into an Eq-equal value.
/// Mirrors `task_roundtrip` and `tasklist_roundtrip`.
///
/// Note: the fixture carries `"comment": null` (an explicit null on an
/// `Option<String>` field with `skip_serializing_if`). On deserialize
/// the field becomes `None`; on serialize the key is omitted. A
/// byte-equal round-trip is therefore NOT possible against the fixture
/// — but a structural round-trip is, because the second deserialize
/// also collapses the absent key into `None`. The Eq compare is the
/// right oracle for catching deserialize regressions without coupling
/// the test to the explicit-null-vs-absent asymmetry documented in
/// workspace AGENTS.md extras-preservation policy.
///
/// See bd:JMAP-ky8g.5.
#[test]
fn notification_roundtrip() {
    let json = include_str!("fixtures/notification.json");
    let original: TaskNotification = serde_json::from_str(json).expect("first deserialize");
    let serialized = serde_json::to_string(&original).expect("serialize");
    let recovered: TaskNotification =
        serde_json::from_str(&serialized).expect("second deserialize");
    assert_eq!(original, recovered);
}

/// `task_patch` round-trips as a `PatchObject` without altering wire shape.
///
/// Oracle: hand-written JSON modelled on draft-tasks-06 §5.1 (taskPatch is a
/// PatchObject per RFC 8620 §5.3). Single top-level patch key keeps the
/// serialization deterministic.
#[test]
fn notification_task_patch_patch_object_transparent() {
    let json = r#"{"id":"notif002","created":"2020-01-10T08:00:00Z","changedBy":{"@type":"Person","name":"Alice"},"type":"updated","taskId":"taskid001","taskPatch":{"title":"New title"}}"#;
    let notif: TaskNotification = serde_json::from_str(json).expect("notification");

    let patch = notif.task_patch.as_ref().expect("taskPatch present");
    assert_eq!(
        patch.as_map().get("title").and_then(|v| v.as_str()),
        Some("New title")
    );

    // Byte-for-byte round-trip: PatchObject's #[serde(transparent)] must
    // emit the inner Map directly, with no wrapper key.
    let serialized = serde_json::to_string(&notif).expect("serialize");
    assert_eq!(serialized, json);
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

// ─── JSCalendar sloppy-Value round-trip tests (JMAP-yfpq.2) ──────────────────
//
// These tests prove that each of the 11 `Option<...serde_json::Value>` sloppy
// fields on `Task` / `TaskList` carries JSON that round-trips through the
// matching typed sub-type in `jmap-jscalendar-types`.
//
// Pattern per field:
//   1. Build minimal Task / TaskList JSON containing the field, populated
//      with one or more RFC 8984 spec-shaped sub-objects.
//   2. Deserialize as Task / TaskList — proves the wire shape is accepted.
//   3. Extract the field's Value and `serde_json::from_value` it into the
//      typed jscalendar sub-type — the new contract this test proves.
//   4. Assert key fields on the typed sub-type to confirm data really
//      landed in the right places.
//   5. Re-serialize the typed sub-type and check the round-tripped JSON
//      matches the input shape.
//
// Oracle source: RFC 8984 (the JSCalendar specification) — sections cited
// per test. NEVER derived from code under test.
//
// Note on `time_zones`: RFC 8984 §4.7.2 defines `timeZones` as a map of
// TimeZoneId → TimeZone object, but `jmap-jscalendar-types` does not yet
// model the `TimeZone` typed sub-type. The `time_zones` test below
// therefore only verifies Value-level round-trip; see JMAP-x014 for the
// follow-up to add `TimeZone` and upgrade this test.
mod jscalendar_roundtrip {
    use jmap_jscalendar_types::{
        Alert, AlertTrigger, Link, Location, Participant, RecurrenceRule, Relation, VirtualLocation,
    };
    use jmap_tasks_types::{Task, TaskList};

    /// Build a syntactically valid minimal `Task` JSON object with a single
    /// extra key/value injected at the top level. `Task` has no
    /// strictly-required fields (every field is `Option`), so the helper
    /// emits just the essentials plus the extra key under test.
    fn task_with(extra_key: &str, extra_value: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "id": "T1",
            "taskListId": "L1",
            "@type": "Task",
            "uid": "task-uid-yfpq2",
            "title": "fixture task",
            extra_key: extra_value,
        })
    }

    /// RFC 8984 §4.1.3 — `relatedTo` is `String[Relation]`.
    ///
    /// A Relation object has `@type: "Relation"` and an optional map
    /// `relation: String[Boolean]` (RFC 8984 §1.4.10).
    #[test]
    fn task_related_to_roundtrips_as_relation() {
        let related_to_value = serde_json::json!({
            "task-other-uid": {
                "@type": "Relation",
                "relation": { "parent": true }
            }
        });
        let raw = task_with("relatedTo", related_to_value.clone());

        let task: Task = serde_json::from_value(raw).expect("Task deserialize");
        let map = task.related_to.as_ref().expect("relatedTo present");
        let entry = map.get("task-other-uid").expect("entry");

        let rel: Relation = serde_json::from_value(entry.clone()).expect("decode Relation");
        assert_eq!(rel.at_type, "Relation");
        assert_eq!(
            rel.relation.as_ref().and_then(|m| m.get("parent")).copied(),
            Some(true)
        );

        let round_tripped = serde_json::to_value(&rel).expect("serialize Relation");
        assert_eq!(round_tripped, entry.clone());
    }

    /// RFC 8984 §4.2.5 — `locations` is `Id[Location]`. Example shape
    /// adapted from RFC 8984 §6.8 (Event with Multiple Locations).
    #[test]
    fn task_locations_roundtrips_as_location() {
        let locations_value = serde_json::json!({
            "loc-1": {
                "@type": "Location",
                "name": "The Music Bowl",
                "description": "Music Bowl, Central Park, New York",
                "coordinates": "geo:40.7829,-73.9654"
            }
        });
        let raw = task_with("locations", locations_value);

        let task: Task = serde_json::from_value(raw).expect("Task deserialize");
        let map = task.locations.as_ref().expect("locations present");
        let entry = map.values().next().expect("at least one location");

        let loc: Location = serde_json::from_value(entry.clone()).expect("decode Location");
        assert_eq!(loc.at_type, "Location");
        assert_eq!(loc.name.as_deref(), Some("The Music Bowl"));
        assert_eq!(loc.coordinates.as_deref(), Some("geo:40.7829,-73.9654"));

        let round_tripped = serde_json::to_value(&loc).expect("serialize Location");
        assert_eq!(round_tripped, entry.clone());
    }

    /// RFC 8984 §4.2.6 — `virtualLocations` is `Id[VirtualLocation]`.
    /// `uri` is mandatory per spec. Example shape from §6.8.
    #[test]
    fn task_virtual_locations_roundtrips_as_virtual_location() {
        let vloc_value = serde_json::json!({
            "vloc1": {
                "@type": "VirtualLocation",
                "name": "Free live Stream from Music Bowl",
                "uri": "https://stream.example.com/the_band_2020"
            }
        });
        let raw = task_with("virtualLocations", vloc_value);

        let task: Task = serde_json::from_value(raw).expect("Task deserialize");
        let map = task
            .virtual_locations
            .as_ref()
            .expect("virtualLocations present");
        let entry = map.values().next().expect("at least one vloc");

        let vloc: VirtualLocation =
            serde_json::from_value(entry.clone()).expect("decode VirtualLocation");
        assert_eq!(vloc.at_type, "VirtualLocation");
        assert_eq!(vloc.uri, "https://stream.example.com/the_band_2020");
        assert_eq!(
            vloc.name.as_deref(),
            Some("Free live Stream from Music Bowl")
        );

        let round_tripped = serde_json::to_value(&vloc).expect("serialize VirtualLocation");
        assert_eq!(round_tripped, entry.clone());
    }

    /// RFC 8984 §4.2.7 / §1.4.11 — `links` is `Id[Link]`.
    /// `href` is the only meaningful identifier when `blobId` is absent.
    #[test]
    fn task_links_roundtrips_as_link() {
        let links_value = serde_json::json!({
            "link-1": {
                "@type": "Link",
                "href": "https://example.com/attach/report.pdf",
                "contentType": "application/pdf",
                "size": 123456_u64,
                "rel": "enclosure"
            }
        });
        let raw = task_with("links", links_value);

        let task: Task = serde_json::from_value(raw).expect("Task deserialize");
        let map = task.links.as_ref().expect("links present");
        let entry = map.values().next().expect("at least one link");

        let link: Link = serde_json::from_value(entry.clone()).expect("decode Link");
        assert_eq!(link.at_type, "Link");
        assert_eq!(
            link.href.as_deref(),
            Some("https://example.com/attach/report.pdf")
        );
        assert_eq!(link.content_type.as_deref(), Some("application/pdf"));
        assert_eq!(link.size, Some(123_456));
        assert_eq!(link.rel.as_deref(), Some("enclosure"));

        let round_tripped = serde_json::to_value(&link).expect("serialize Link");
        assert_eq!(round_tripped, entry.clone());
    }

    /// RFC 8984 §4.3.3 — `recurrenceRules` is a list of RecurrenceRule.
    /// Example shape adapted from §6.9 (Recurring Event with Overrides).
    #[test]
    fn task_recurrence_rules_roundtrips_as_recurrence_rule() {
        let rules_value = serde_json::json!([
            {
                "@type": "RecurrenceRule",
                "frequency": "weekly",
                "until": "2020-06-24T09:00:00"
            }
        ]);
        let raw = task_with("recurrenceRules", rules_value);

        let task: Task = serde_json::from_value(raw).expect("Task deserialize");
        let rules = task
            .recurrence_rules
            .as_ref()
            .expect("recurrenceRules present");
        let entry = rules.first().expect("at least one rule");

        let rule: RecurrenceRule =
            serde_json::from_value(entry.clone()).expect("decode RecurrenceRule");
        assert_eq!(rule.at_type, "RecurrenceRule");
        assert_eq!(rule.frequency, "weekly");
        assert_eq!(
            rule.until.as_ref().map(AsRef::as_ref),
            Some("2020-06-24T09:00:00")
        );

        let round_tripped = serde_json::to_value(&rule).expect("serialize RecurrenceRule");
        assert_eq!(round_tripped, entry.clone());
    }

    /// RFC 8984 §4.3.4 — `excludedRecurrenceRules` shares the RecurrenceRule
    /// shape with §4.3.3. Verifying both fields independently catches a
    /// regression where only one of the two `Vec<Value>` fields is wired up.
    #[test]
    fn task_excluded_recurrence_rules_roundtrips_as_recurrence_rule() {
        let rules_value = serde_json::json!([
            {
                "@type": "RecurrenceRule",
                "frequency": "daily",
                "byMonth": ["12"]
            }
        ]);
        let raw = task_with("excludedRecurrenceRules", rules_value);

        let task: Task = serde_json::from_value(raw).expect("Task deserialize");
        let rules = task
            .excluded_recurrence_rules
            .as_ref()
            .expect("excludedRecurrenceRules present");
        let entry = rules.first().expect("at least one rule");

        let rule: RecurrenceRule =
            serde_json::from_value(entry.clone()).expect("decode RecurrenceRule");
        assert_eq!(rule.at_type, "RecurrenceRule");
        assert_eq!(rule.frequency, "daily");
        assert_eq!(rule.by_month.as_deref(), Some(&["12".to_string()][..]));

        let round_tripped = serde_json::to_value(&rule).expect("serialize RecurrenceRule");
        assert_eq!(round_tripped, entry.clone());
    }

    /// RFC 8984 §4.4.6 — `participants` is `Id[Participant]`. Example
    /// shape adapted from §6.10 (Recurring Event with Participants).
    #[test]
    fn task_participants_roundtrips_as_participant() {
        let participants_value = serde_json::json!({
            "p-tom": {
                "@type": "Participant",
                "name": "Tom Tool",
                "email": "tom@foobar.example.com",
                "sendTo": {
                    "imip": "mailto:tom@calendar.example.com"
                },
                "participationStatus": "accepted",
                "roles": { "attendee": true }
            }
        });
        let raw = task_with("participants", participants_value);

        let task: Task = serde_json::from_value(raw).expect("Task deserialize");
        let map = task.participants.as_ref().expect("participants present");
        let entry = map.values().next().expect("at least one participant");

        let p: Participant = serde_json::from_value(entry.clone()).expect("decode Participant");
        assert_eq!(p.at_type, "Participant");
        assert_eq!(p.name.as_deref(), Some("Tom Tool"));
        assert_eq!(p.email.as_deref(), Some("tom@foobar.example.com"));
        assert_eq!(
            p.send_to
                .as_ref()
                .and_then(|m| m.get("imip"))
                .map(String::as_str),
            Some("mailto:tom@calendar.example.com")
        );

        let round_tripped = serde_json::to_value(&p).expect("serialize Participant");
        assert_eq!(round_tripped, entry.clone());
    }

    /// RFC 8984 §4.5.2 — `alerts` is `Id[Alert]` with an OffsetTrigger
    /// (example: `"-PT15M"` to fire 15 minutes before start).
    #[test]
    fn task_alerts_roundtrips_as_alert_offset_trigger() {
        let alerts_value = serde_json::json!({
            "alarm-1": {
                "@type": "Alert",
                "trigger": {
                    "@type": "OffsetTrigger",
                    "offset": "-PT15M"
                },
                "action": "display"
            }
        });
        let raw = task_with("alerts", alerts_value);

        let task: Task = serde_json::from_value(raw).expect("Task deserialize");
        let map = task.alerts.as_ref().expect("alerts present");
        let entry = map.values().next().expect("at least one alert");

        let alert: Alert = serde_json::from_value(entry.clone()).expect("decode Alert");
        assert_eq!(alert.at_type, "Alert");
        assert_eq!(alert.action.as_deref(), Some("display"));
        match &alert.trigger {
            AlertTrigger::OffsetTrigger(t) => {
                assert_eq!(t.offset.as_ref(), "-PT15M");
            }
            other => panic!("expected OffsetTrigger, got {other:?}"),
        }

        let round_tripped = serde_json::to_value(&alert).expect("serialize Alert");
        assert_eq!(round_tripped, entry.clone());
    }

    /// RFC 8984 §4.7.2 — `timeZones` is `Map<TimeZoneId, TimeZone>`.
    ///
    /// `Task.time_zones` is the sloppy-Value field
    /// (`Option<HashMap<String, serde_json::Value>>`) for wire-shape
    /// preservation. Each value entry can be typed-decoded into a
    /// `jmap_jscalendar_types::TimeZone` (re-exported from this crate as
    /// `TimeZone`). This test verifies both:
    ///
    ///   1. The Value round-trip survives unchanged.
    ///   2. A typed `TimeZone` decoded from the Value carries the
    ///      expected `tzId`, `@type`, and STANDARD rule fields.
    #[test]
    fn task_time_zones_value_and_typed_roundtrip() {
        use jmap_tasks_types::TimeZone;

        let tz_value = serde_json::json!({
            "/example/custom/UTC+05:30:00": {
                "@type": "TimeZone",
                "tzId": "/example/custom/UTC+05:30:00",
                "updated": "2024-01-01T00:00:00Z",
                "standard": [
                    {
                        "@type": "TimeZoneRule",
                        "start": "1970-01-01T00:00:00",
                        "offsetFrom": "+0530",
                        "offsetTo": "+0530"
                    }
                ]
            }
        });
        let raw = task_with("timeZones", tz_value.clone());

        let task: Task = serde_json::from_value(raw).expect("Task deserialize");
        let map = task.time_zones.as_ref().expect("timeZones present");
        let entry = map
            .get("/example/custom/UTC+05:30:00")
            .expect("custom tz entry");

        // Value-only round-trip: the field stores the wire shape opaquely
        // and emits it back unchanged.
        let round_tripped = serde_json::to_value(entry).expect("serialize Value");
        assert_eq!(
            round_tripped,
            tz_value["/example/custom/UTC+05:30:00"].clone()
        );

        // Typed decode: the Value can be parsed into a typed
        // `TimeZone` via `serde_json::from_value`.
        let tz: TimeZone = serde_json::from_value(entry.clone()).expect("typed TimeZone decode");
        assert_eq!(tz.at_type, "TimeZone");
        assert_eq!(tz.tz_id, "/example/custom/UTC+05:30:00");
        let standard = tz.standard.as_ref().expect("standard rule present");
        assert_eq!(standard.len(), 1);
        assert_eq!(standard[0].offset_from.as_ref(), "+0530");
        assert_eq!(standard[0].offset_to.as_ref(), "+0530");
        assert_eq!(standard[0].at_type, "TimeZoneRule");

        // Typed re-encode matches the original Value.
        let typed_back = serde_json::to_value(&tz).expect("typed TimeZone encode");
        assert_eq!(typed_back, tz_value["/example/custom/UTC+05:30:00"].clone());
    }

    /// Helper for TaskList tests: minimal TaskList JSON with one
    /// extra top-level key.
    fn task_list_with(extra_key: &str, extra_value: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "id": "L1",
            "name": "fixture list",
            "sortOrder": 0,
            "isSubscribed": false,
            "myRights": {
                "mayReadItems": true,
                "mayWriteAll": false,
                "mayWriteOwn": false,
                "mayUpdatePrivate": false,
                "mayRSVP": false,
                "mayAdmin": false,
                "mayDelete": false
            },
            extra_key: extra_value,
        })
    }

    /// draft-ietf-jmap-tasks-06 §3 / RFC 8984 §4.5.2 — TaskList carries
    /// `defaultAlertsWithTime` whose values are JSCalendar `Alert` objects.
    /// Verify with an OffsetTrigger, matching what a client would set for
    /// "remind me 15 minutes before any timed task on this list".
    #[test]
    fn task_list_default_alerts_with_time_roundtrips_as_alert() {
        let alerts_value = serde_json::json!({
            "default-15m": {
                "@type": "Alert",
                "trigger": {
                    "@type": "OffsetTrigger",
                    "offset": "-PT15M"
                }
            }
        });
        let raw = task_list_with("defaultAlertsWithTime", alerts_value);

        let tl: TaskList = serde_json::from_value(raw).expect("TaskList deserialize");
        let map = tl
            .default_alerts_with_time
            .as_ref()
            .expect("defaultAlertsWithTime present");
        let entry = map.values().next().expect("at least one default alert");

        let alert: Alert = serde_json::from_value(entry.clone()).expect("decode Alert");
        assert_eq!(alert.at_type, "Alert");
        match &alert.trigger {
            AlertTrigger::OffsetTrigger(t) => {
                assert_eq!(t.offset.as_ref(), "-PT15M");
            }
            other => panic!("expected OffsetTrigger, got {other:?}"),
        }

        let round_tripped = serde_json::to_value(&alert).expect("serialize Alert");
        assert_eq!(round_tripped, entry.clone());
    }

    /// draft-ietf-jmap-tasks-06 §3 — `defaultAlertsWithoutTime` parallels
    /// `defaultAlertsWithTime` but for tasks without a specific time of
    /// day (e.g. all-day tasks). Same Alert shape; we use an
    /// AbsoluteTrigger here to also exercise the second AlertTrigger
    /// variant.
    #[test]
    fn task_list_default_alerts_without_time_roundtrips_as_alert() {
        let alerts_value = serde_json::json!({
            "default-noon": {
                "@type": "Alert",
                "trigger": {
                    "@type": "AbsoluteTrigger",
                    "when": "2024-06-15T08:45:00Z"
                },
                "action": "email"
            }
        });
        let raw = task_list_with("defaultAlertsWithoutTime", alerts_value);

        let tl: TaskList = serde_json::from_value(raw).expect("TaskList deserialize");
        let map = tl
            .default_alerts_without_time
            .as_ref()
            .expect("defaultAlertsWithoutTime present");
        let entry = map.values().next().expect("at least one default alert");

        let alert: Alert = serde_json::from_value(entry.clone()).expect("decode Alert");
        assert_eq!(alert.at_type, "Alert");
        assert_eq!(alert.action.as_deref(), Some("email"));
        match &alert.trigger {
            AlertTrigger::AbsoluteTrigger(t) => {
                assert_eq!(t.when.as_ref(), "2024-06-15T08:45:00Z");
            }
            other => panic!("expected AbsoluteTrigger, got {other:?}"),
        }

        let round_tripped = serde_json::to_value(&alert).expect("serialize Alert");
        assert_eq!(round_tripped, entry.clone());
    }
}

// ── Extras-preservation policy tests (JMAP-lbdy.6) ──────────────────────────
//
// One round-trip preservation test per migrated type. Each asserts that
// an unknown vendor / site / private-extension field survives
// deserialize/serialize unchanged. Per workspace AGENTS.md
// "Extras-preservation policy for vendor/site fields".

use jmap_tasks_types::{CheckItem, Checklist, Comment, Person};

/// `Person.extra` captures vendor fields and preserves them.
#[test]
fn person_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "@type": "Person",
        "name": "Alice",
        "acmeCorpEmployeeId": "emp-42"
    });
    let p: Person = serde_json::from_value(raw).unwrap();
    assert_eq!(
        p.extra.get("acmeCorpEmployeeId").and_then(|v| v.as_str()),
        Some("emp-42")
    );
    let back = serde_json::to_value(&p).unwrap();
    assert_eq!(back["acmeCorpEmployeeId"], "emp-42");
}

/// `CheckItem.extra` captures vendor fields and preserves them.
#[test]
fn check_item_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "@type": "CheckItem",
        "title": "Buy milk",
        "isComplete": false,
        "acmeCorpPriority": "high"
    });
    let c: CheckItem = serde_json::from_value(raw).unwrap();
    assert_eq!(
        c.extra.get("acmeCorpPriority").and_then(|v| v.as_str()),
        Some("high")
    );
    let back = serde_json::to_value(&c).unwrap();
    assert_eq!(back["acmeCorpPriority"], "high");
}

/// `Checklist.extra` captures vendor fields and preserves them.
#[test]
fn checklist_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "@type": "Checklist",
        "title": "Shopping",
        "acmeCorpListColor": "#abcdef"
    });
    let c: Checklist = serde_json::from_value(raw).unwrap();
    assert_eq!(
        c.extra.get("acmeCorpListColor").and_then(|v| v.as_str()),
        Some("#abcdef")
    );
    let back = serde_json::to_value(&c).unwrap();
    assert_eq!(back["acmeCorpListColor"], "#abcdef");
}

/// `Comment.extra` captures vendor fields and preserves them.
#[test]
fn comment_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "@type": "Comment",
        "message": "lgtm",
        "acmeCorpCommentChannel": "review"
    });
    let c: Comment = serde_json::from_value(raw).unwrap();
    assert_eq!(
        c.extra
            .get("acmeCorpCommentChannel")
            .and_then(|v| v.as_str()),
        Some("review")
    );
    let back = serde_json::to_value(&c).unwrap();
    assert_eq!(back["acmeCorpCommentChannel"], "review");
}

/// `Task.extra` captures vendor fields and preserves them.
#[test]
fn task_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "uid": "t-1",
        "title": "Write report",
        "acmeCorpExternalRef": "JIRA-42"
    });
    let t: Task = serde_json::from_value(raw).unwrap();
    assert_eq!(
        t.extra.get("acmeCorpExternalRef").and_then(|v| v.as_str()),
        Some("JIRA-42")
    );
    let back = serde_json::to_value(&t).unwrap();
    assert_eq!(back["acmeCorpExternalRef"], "JIRA-42");
}

/// `TaskRights.extra` captures vendor fields and preserves them.
#[test]
fn task_rights_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "mayReadItems": true,
        "mayWriteAll": false,
        "mayWriteOwn": true,
        "mayUpdatePrivate": true,
        "mayRSVP": true,
        "mayAdmin": false,
        "mayDelete": false,
        "acmeCorpMayBulkAssign": true
    });
    let r: TaskRights = serde_json::from_value(raw).unwrap();
    assert_eq!(
        r.extra
            .get("acmeCorpMayBulkAssign")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    let back = serde_json::to_value(&r).unwrap();
    assert_eq!(back["acmeCorpMayBulkAssign"], true);
}

/// `TaskList.extra` captures vendor fields and preserves them.
#[test]
fn task_list_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "id": "tl1",
        "name": "Personal",
        "sortOrder": 0,
        "isSubscribed": true,
        "myRights": {
            "mayReadItems": true, "mayWriteAll": true, "mayWriteOwn": true,
            "mayUpdatePrivate": true, "mayRSVP": true, "mayAdmin": true,
            "mayDelete": true
        },
        "acmeCorpDepartment": "ops"
    });
    let tl: TaskList = serde_json::from_value(raw).unwrap();
    assert_eq!(
        tl.extra.get("acmeCorpDepartment").and_then(|v| v.as_str()),
        Some("ops")
    );
    let back = serde_json::to_value(&tl).unwrap();
    assert_eq!(back["acmeCorpDepartment"], "ops");
}

/// `TaskNotification.extra` captures vendor fields and preserves them.
#[test]
fn task_notification_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "id": "tn1",
        "created": "2024-06-01T00:00:00Z",
        "changedBy": {
            "@type": "Person",
            "name": "Bob"
        },
        "type": "created",
        "taskId": "t1",
        "acmeCorpNotificationChannel": "email"
    });
    let n: TaskNotification = serde_json::from_value(raw).unwrap();
    assert_eq!(
        n.extra
            .get("acmeCorpNotificationChannel")
            .and_then(|v| v.as_str()),
        Some("email")
    );
    let back = serde_json::to_value(&n).unwrap();
    assert_eq!(back["acmeCorpNotificationChannel"], "email");
}

// ── @type-default regression tests (bd:JMAP-ky8g.1) ─────────────────────
//
// Person / CheckItem / Checklist / Comment declare `@type` as a bare
// `String` with a serde-default function returning the type-mandated
// literal. Deserialize must succeed when `@type` is absent (spec-
// violating producer input or partial fixture), populating the field
// with the literal. Serialize must always emit the field.
//
// Independent oracle: hand-written JSON shaped against draft-tasks-06
// §4.2.3 / §4.2.4 with `@type` omitted, plus the produced
// serialize-back JSON checked against the same draft's mandated string.

/// `Person` deserialize succeeds when `@type` is absent and defaults
/// to `"Person"`. Re-serialize emits the field with the default value.
#[test]
fn person_at_type_defaults_when_absent() {
    let raw = serde_json::json!({ "name": "Alice" });
    let p: Person = serde_json::from_value(raw).unwrap();
    assert_eq!(p.at_type, "Person");
    let back = serde_json::to_value(&p).unwrap();
    assert_eq!(back["@type"], "Person");
}

/// `CheckItem` deserialize succeeds when `@type` is absent and defaults
/// to `"CheckItem"`. Re-serialize emits the field with the default value.
#[test]
fn check_item_at_type_defaults_when_absent() {
    let raw = serde_json::json!({
        "title": "Buy milk",
        "isComplete": false
    });
    let c: CheckItem = serde_json::from_value(raw).unwrap();
    assert_eq!(c.at_type, "CheckItem");
    let back = serde_json::to_value(&c).unwrap();
    assert_eq!(back["@type"], "CheckItem");
}

/// `Checklist` deserialize succeeds when `@type` is absent and defaults
/// to `"Checklist"`. Re-serialize emits the field with the default value.
#[test]
fn checklist_at_type_defaults_when_absent() {
    let raw = serde_json::json!({ "title": "Shopping" });
    let c: Checklist = serde_json::from_value(raw).unwrap();
    assert_eq!(c.at_type, "Checklist");
    let back = serde_json::to_value(&c).unwrap();
    assert_eq!(back["@type"], "Checklist");
}

/// `Comment` deserialize succeeds when `@type` is absent and defaults
/// to `"Comment"`. Re-serialize emits the field with the default value.
#[test]
fn comment_at_type_defaults_when_absent() {
    let raw = serde_json::json!({ "message": "lgtm" });
    let c: Comment = serde_json::from_value(raw).unwrap();
    assert_eq!(c.at_type, "Comment");
    let back = serde_json::to_value(&c).unwrap();
    assert_eq!(back["@type"], "Comment");
}

/// Explicit non-default `@type` values still round-trip verbatim
/// (the serde-default does NOT overwrite an explicit wire value).
/// Locks in the contract that a vendor shipping a non-conformant string
/// is preserved end-to-end rather than silently normalised.
#[test]
fn person_at_type_explicit_value_round_trips_verbatim() {
    let raw = serde_json::json!({ "@type": "AcmeCorpPerson", "name": "Alice" });
    let p: Person = serde_json::from_value(raw).unwrap();
    assert_eq!(p.at_type, "AcmeCorpPerson");
    let back = serde_json::to_value(&p).unwrap();
    assert_eq!(back["@type"], "AcmeCorpPerson");
}

/// A parent object (Task -> checklists -> CheckItem) deserializes
/// successfully when nested CheckItems omit `@type`. This is the
/// concrete failure-mode the bead identifies: a server response
/// missing `@type` on a sub-object would previously fail the whole
/// Task deserialize.
#[test]
fn task_with_check_items_missing_at_type_deserializes() {
    let raw = serde_json::json!({
        "id": "t1",
        "title": "Plan release",
        "checklists": {
            "cl1": {
                "title": "Release tasks",
                "checkItems": [
                    { "title": "Tag commit", "isComplete": false },
                    { "title": "Publish crate", "isComplete": false }
                ]
            }
        }
    });
    let t: Task = serde_json::from_value(raw).unwrap();
    let checklists = t.checklists.expect("checklists present");
    let cl = checklists
        .get(&jmap_types::Id::from("cl1"))
        .expect("cl1 present");
    assert_eq!(cl.at_type, "Checklist");
    let items = cl.check_items.as_ref().expect("checkItems present");
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i.at_type == "CheckItem"));
}
