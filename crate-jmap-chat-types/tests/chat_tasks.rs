//! draft-atwood-jmap-chat-tasks-00 types: capability, `Space.taskListId`
//! and `Task.chatId` accessors, the task MessageAction metadata, and the
//! `jmap:task:` URI form.
//!
//! Oracle: sentences quoted from
//! /home/mark/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-tasks-00.md
//! (line numbers at HEAD) and hand-written JSON built from the field names
//! they define.

use jmap_chat_types::{
    task_chat_id, ChatTasksCapability, MessageAction, Space, TaskMetadata, JMAP_CHAT_TASKS_URI,
    JMAP_URI_TASK_TYPE, SPACE_TASK_LIST_ID_PROPERTY, TASK_CHAT_ID_PROPERTY,
};
use jmap_types::JmapUri;
use serde_json::{json, Map, Value};

#[test]
fn capability_uri_matches_draft() {
    // L92: "Servers supporting this specification MUST advertise the
    // `urn:ietf:params:jmap:chat:tasks` capability in the JMAP Session
    // object."
    assert_eq!(JMAP_CHAT_TASKS_URI, "urn:ietf:params:jmap:chat:tasks");
}

#[test]
fn account_capability_round_trips_two_booleans() {
    // L100-106: the account-level object has `mayBindTaskList` (Boolean)
    // and `mayBackReferenceChat` (Boolean). L106: "A deployment MAY
    // support TaskList binding (`mayBindTaskList: true`) without
    // supporting Task-Chat back-references".
    let raw = json!({ "mayBindTaskList": true, "mayBackReferenceChat": false });
    let cap: ChatTasksCapability = serde_json::from_value(raw.clone()).expect("deserialize");
    assert!(cap.may_bind_task_list);
    assert!(!cap.may_back_reference_chat);
    assert!(cap.extra.is_empty());
    assert_eq!(serde_json::to_value(&cap).expect("serialize"), raw);
}

#[test]
fn account_capability_requires_each_boolean() {
    // L102-106 list both fields without marking either optional.
    for key in ["mayBindTaskList", "mayBackReferenceChat"] {
        let mut raw = json!({ "mayBindTaskList": false, "mayBackReferenceChat": false });
        raw.as_object_mut().expect("object").remove(key);
        assert!(
            serde_json::from_value::<ChatTasksCapability>(raw).is_err(),
            "{key} missing must not deserialize"
        );
    }
}

#[test]
fn account_capability_preserves_vendor_fields() {
    let raw = json!({
        "mayBindTaskList": true,
        "mayBackReferenceChat": true,
        "acmeCorpTier": "gold"
    });
    let cap: ChatTasksCapability = serde_json::from_value(raw.clone()).expect("deserialize");
    assert_eq!(serde_json::to_value(&cap).expect("serialize"), raw);
}

/// A Space with the draft-atwood-jmap-chat-00 core properties plus `ext`.
fn space(ext: Value) -> Value {
    let mut v = json!({
        "id": "S1",
        "name": "Team",
        "roles": [],
        "members": [],
        "categories": [],
        "uncategorizedChannelIds": [],
        "createdAt": "2026-10-20T00:00:00Z",
        "isPublic": false,
        "isPubliclyPreviewable": false,
        "memberCount": 0
    });
    v.as_object_mut()
        .expect("object")
        .extend(ext.as_object().expect("object").clone());
    v
}

#[test]
fn task_list_id_property_name_matches_draft() {
    // L114: "`taskListId` (String, optional)".
    assert_eq!(SPACE_TASK_LIST_ID_PROPERTY, "taskListId");
}

#[test]
fn task_list_id_present_round_trips() {
    // L115: "A JMAP Tasks `TaskList.id` (per {{JMAP-TASKS}}) bound to this
    // Space."
    let raw = space(json!({ "taskListId": "TL1" }));
    let s: Space = serde_json::from_value(raw.clone()).expect("deserialize");
    assert_eq!(
        s.task_list_id()
            .expect("string")
            .as_ref()
            .map(AsRef::as_ref),
        Some("TL1")
    );
    assert_eq!(serde_json::to_value(&s).expect("serialize"), raw);
}

#[test]
fn task_list_id_absent_or_null_means_unbound() {
    // L115: "When absent, this Space has no bound TaskList."
    // L119: "the server MUST clear the field (set to null)".
    for raw in [space(json!({})), space(json!({ "taskListId": null }))] {
        let s: Space = serde_json::from_value(raw).expect("deserialize");
        assert_eq!(s.task_list_id().expect("absent or null"), None);
    }
}

#[test]
fn task_list_id_non_string_is_an_error() {
    // L114: "`taskListId` (String, optional)".
    let s: Space =
        serde_json::from_value(space(json!({ "taskListId": ["TL1"] }))).expect("deserialize");
    assert!(s.task_list_id().is_err());
}

fn map(v: Value) -> Map<String, Value> {
    v.as_object().expect("object").clone()
}

#[test]
fn chat_id_property_name_matches_draft() {
    // L152: "`chatId` (String, optional)".
    assert_eq!(TASK_CHAT_ID_PROPERTY, "chatId");
}

#[test]
fn chat_id_present_is_read() {
    // L153: "A JMAP Chat `Chat.id` (per {{JMAP-CHAT}}) referencing a Chat
    // used for discussion of this Task."
    let extra = map(json!({ "chatId": "C7" }));
    assert_eq!(
        task_chat_id(&extra)
            .expect("string")
            .as_ref()
            .map(AsRef::as_ref),
        Some("C7")
    );
}

#[test]
fn chat_id_absent_or_null_means_no_thread() {
    // L153: "When absent, this Task has no associated discussion thread."
    for extra in [map(json!({})), map(json!({ "chatId": null }))] {
        assert_eq!(task_chat_id(&extra).expect("absent or null"), None);
    }
}

#[test]
fn chat_id_non_string_is_an_error() {
    // L152: "`chatId` (String, optional)".
    assert!(task_chat_id(&map(json!({ "chatId": 3 }))).is_err());
}

#[test]
fn task_metadata_round_trips_draft_keys() {
    // 33702a5 L183-203: a `urn:ietf:params:jmap:chat:cap:task` MessageAction
    // with `uri` in the "`jmap:task:<accountId>:<taskId>`" form (L191), a
    // `label`, and (L200-203) `metadata` keys `progress` (String), `due`
    // (UTCDate), `assigneeIds` (String[]) and `priority` (Integer).
    let action = json!({
        "type": "urn:ietf:params:jmap:chat:cap:task",
        "uri": "jmap:task:A1:T9",
        "label": "Ship the release notes",
        "metadata": {
            "progress": "in-process",
            "due": "2026-10-23T17:00:00Z",
            "assigneeIds": ["CC1", "A1"],
            "priority": 3
        }
    });
    let a: MessageAction = serde_json::from_value(action.clone()).expect("action");
    let m: TaskMetadata =
        serde_json::from_value(a.metadata.clone().expect("metadata")).expect("metadata shape");
    assert_eq!(m.progress.as_deref(), Some("in-process"));
    assert_eq!(
        m.due.as_ref().map(AsRef::as_ref),
        Some("2026-10-23T17:00:00Z")
    );
    let assignees: Vec<&str> = m
        .assignee_ids
        .as_deref()
        .expect("assigneeIds")
        .iter()
        .map(AsRef::as_ref)
        .collect();
    assert_eq!(assignees, ["CC1", "A1"]);
    assert_eq!(m.priority, Some(3));
    assert!(m.extra.is_empty());
    assert_eq!(
        serde_json::to_value(&m).expect("serialize"),
        action["metadata"]
    );
    assert_eq!(serde_json::to_value(&a).expect("serialize"), action);
}

#[test]
fn task_metadata_keys_are_optional() {
    // L194: `metadata` "MAY include the following type-specific keys".
    let m: TaskMetadata = serde_json::from_value(json!({})).expect("empty");
    assert_eq!(m, TaskMetadata::default());
    assert_eq!(serde_json::to_value(&m).expect("serialize"), json!({}));
}

#[test]
fn task_metadata_keeps_unknown_keys() {
    // 6bcd9d8 L204: "Servers MAY include other type-specific keys; clients
    // ignore unknown keys per the MessageAction definition".
    let raw = json!({ "progress": "completed", "acmeStoryPoints": 5 });
    let m: TaskMetadata = serde_json::from_value(raw.clone()).expect("deserialize");
    assert_eq!(m.extra.get("acmeStoryPoints"), Some(&json!(5)));
    assert_eq!(serde_json::to_value(&m).expect("serialize"), raw);
}

#[test]
fn task_metadata_priority_is_integer() {
    // L199: "`priority` (Integer, optional)".
    assert!(serde_json::from_value::<TaskMetadata>(json!({ "priority": 1.5 })).is_err());
    assert!(serde_json::from_value::<TaskMetadata>(json!({ "priority": "high" })).is_err());
}

#[test]
fn task_uri_names_one_task() {
    // L191: "A JMAP object URI ({{JMAP-OBJECT-URI}}) of type `task`, in
    // the form `jmap:task:<accountId>:<taskId>`, referencing a Task on the
    // named account."
    let uri: JmapUri = "jmap:task:A1:T9".parse().expect("parse");
    assert_eq!(uri.data_type(), JMAP_URI_TASK_TYPE);
    assert_eq!(uri.account_id().as_ref(), "A1");
    assert_eq!(uri.object_id().as_ref(), "T9");
}

#[test]
fn task_uri_with_task_list_segment_rejected() {
    // L191 carries no taskListId; draft-atwood-jmap-object-uri-00 L96: "A
    // receiver treats a `jmap` URI that matches no form it implements as
    // unresolvable."
    assert!("jmap:task:A1:TL1:T9".parse::<JmapUri>().is_err());
}
