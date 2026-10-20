//! Space `taskListId` binding rules.
//!
//! Oracle: draft-atwood-jmap-chat-tasks-00.md at jmap-chat-spec be46208
//! ("tasks Lnnn") and RFC 8620:
//!
//! - tasks L103: "Deployments that prohibit Space-TaskList binding ...
//!   MUST set this to `false`."
//! - tasks L114-115: "`taskListId` (String, optional): A JMAP Tasks
//!   `TaskList.id` ... bound to this Space. ... When absent, this Space
//!   has no bound TaskList."
//! - tasks L117: "The bound TaskList MUST belong to the same account that
//!   owns the Space. Cross-account TaskList references are not supported
//!   in this revision".
//! - tasks L125: "Servers that advertise `mayBindTaskList: true` MUST
//!   accept patches to `Space.taskListId` from members holding the
//!   `"manage_space"` permission. Setting `taskListId` to a non-null value
//!   binds the Space to the identified TaskList; setting it to `null`
//!   unbinds the Space. If the `taskListId` value does not reference an
//!   existing TaskList on the same account, the server MUST return a
//!   SetError of type `invalidProperties` naming `taskListId`."
//! - tasks L127: "Servers MAY automatically create a new TaskList when a
//!   Space is created and bind it ... When this is done, the server MUST
//!   set `taskListId` to the new TaskList's id at Space creation time."
//! - tasks L129: "The TaskList persists independently and may be unbound
//!   from the Space without being deleted."
//! - RFC 8620 §5.3 `invalidProperties`: "The record given is invalid in
//!   some way."
//!
//! The `manage_space` gate of L125 is `apply_space_metadata_patch`'s, and
//! is tested in `jmap-chat-server`
//! (`registered_property_patch_without_manage_space_is_forbidden`).
//!
//! Records are hand-written JSON from the drafts' field lists.

use jmap_chat_server::memory::MemoryBackend as ChatMemory;
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_chat_tasks_server::{
    check_task_list_id, create_space_task_list, space_extension_properties,
};
use jmap_chat_types::{ChatTasksCapability, Space};
use jmap_tasks_server::handle_task_list_get;
use jmap_tasks_server::memory::MemoryBackend as TasksMemory;
use jmap_types::Id;
use serde_json::{json, Value};

const ACCOUNT: &str = "A1";
const OTHER_ACCOUNT: &str = "A2";

/// A TaskList with the draft-ietf-jmap-tasks-06 §TaskList field list.
fn task_list_json(id: &str) -> Value {
    json!({
        "id": id, "role": null, "name": "Work", "description": null,
        "color": null, "keywordColors": null, "categoryColors": null,
        "sortOrder": 0, "isSubscribed": true, "timeZone": null,
        "workflowStatuses": null, "shareWith": null,
        "myRights": { "mayReadItems": true, "mayWriteAll": true,
                      "mayWriteOwn": true, "mayUpdatePrivate": true,
                      "mayRSVP": true, "mayAdmin": true, "mayDelete": true }
    })
}

/// TaskList `T1` on `A1`, TaskList `T2` on `A2`.
fn tasks() -> TasksMemory {
    let tasks = TasksMemory::new()
        .with_account(ACCOUNT)
        .with_account(OTHER_ACCOUNT);
    tasks.seed_object(ACCOUNT, "TaskList", "T1", task_list_json("T1"));
    tasks.seed_object(OTHER_ACCOUNT, "TaskList", "T2", task_list_json("T2"));
    tasks
}

/// A Space per the draft-atwood-jmap-chat-00 §Space field list, with
/// `extra` merged in.
fn space(extra: Value) -> Space {
    let mut v = json!({
        "id": "S1", "name": "Team", "roles": [], "members": [],
        "categories": [], "uncategorizedChannelIds": [],
        "createdAt": "2026-01-01T00:00:00Z", "isPublic": false,
        "isPubliclyPreviewable": false, "memberCount": 0
    });
    v.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("object").clone());
    serde_json::from_value(v).expect("Space fixture")
}

fn capability(may_bind_task_list: bool) -> ChatTasksCapability {
    serde_json::from_value(json!({
        "mayBindTaskList": may_bind_task_list,
        "mayBackReferenceChat": false
    }))
    .expect("capability fixture")
}

/// `TaskList/get` of `ids` on `account`: the ids listed and not found.
async fn task_list_get(tasks: &TasksMemory, account: &str, ids: Value) -> (Vec<String>, Value) {
    let out = handle_task_list_get(tasks, &(), json!({ "accountId": account, "ids": ids }))
        .await
        .expect("TaskList/get");
    let listed = out.primary["list"]
        .as_array()
        .expect("list")
        .iter()
        .map(|t| t["id"].as_str().expect("id").to_owned())
        .collect();
    (listed, out.primary["notFound"].clone())
}

async fn check(tasks: &TasksMemory, value: Value) -> Result<(), (Value, Value)> {
    check_task_list_id(tasks, &(), &Id::from(ACCOUNT), &value)
        .await
        .expect("TaskList read")
        .map_err(|e| {
            let wire = serde_json::to_value(&e).expect("SetError serializes");
            (wire["type"].clone(), wire["properties"].clone())
        })
}

/// A chat backend whose Space extension properties come from
/// [`space_extension_properties`] for `mayBindTaskList`.
fn chat(may_bind_task_list: bool) -> ChatMemory {
    let chat = ChatMemory::new();
    chat.register_account(&Id::from(ACCOUNT));
    chat.set_space_extension_properties_for_test(space_extension_properties(&capability(
        may_bind_task_list,
    )));
    chat
}

async fn space_set(chat: &ChatMemory, args: Value) -> Value {
    handle_space_set(chat, &(), args)
        .await
        .expect("Space/set")
        .primary
}

/// Create a Space with `taskListId: "T1"`; return its id and its
/// `taskListId` as `Space/get` returns it.
async fn create_bound_space(chat: &ChatMemory) -> (String, Value) {
    let out = space_set(
        chat,
        json!({ "accountId": ACCOUNT,
                "create": { "s": { "name": "Team", "taskListId": "T1" } } }),
    )
    .await;
    let id = out["created"]["s"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Space not created: {out}"))
        .to_owned();
    let got = handle_space_get(chat, &(), json!({ "accountId": ACCOUNT, "ids": [&id] }))
        .await
        .expect("Space/get")
        .primary;
    let task_list_id = got["list"][0]
        .get("taskListId")
        .cloned()
        .unwrap_or(Value::Null);
    (id, task_list_id)
}

/// L103: with `mayBindTaskList: false` no Space is bound. A create leaves
/// `taskListId` absent ("When absent, this Space has no bound TaskList",
/// L115), and an update patch naming it is `invalidProperties` (RFC 8620
/// §5.3), as `taskListId` is then no Space property.
#[tokio::test]
async fn binding_refused_without_may_bind_task_list() {
    let chat = chat(false);
    let (id, task_list_id) = create_bound_space(&chat).await;
    assert_eq!(task_list_id, Value::Null);
    let out = space_set(
        &chat,
        json!({ "accountId": ACCOUNT, "update": { &id: { "taskListId": "T1" } } }),
    )
    .await;
    assert_eq!(
        out["notUpdated"][&id],
        json!({ "type": "invalidProperties", "properties": ["taskListId"] })
    );
}

/// L125: with `mayBindTaskList: true`, `Space/set` binds on create and
/// accepts a patch to `taskListId` from the owner, who holds
/// `manage_space`.
#[tokio::test]
async fn binding_accepted_with_may_bind_task_list() {
    let chat = chat(true);
    let (id, task_list_id) = create_bound_space(&chat).await;
    assert_eq!(task_list_id, json!("T1"));
    let out = space_set(
        &chat,
        json!({ "accountId": ACCOUNT, "update": { &id: { "taskListId": null } } }),
    )
    .await;
    assert!(out["updated"].get(&id).is_some(), "{out}");
}

/// L117, L125: a TaskList on the Space's own account may be bound.
#[tokio::test]
async fn same_account_task_list_is_accepted() {
    assert_eq!(check(&tasks(), json!("T1")).await, Ok(()));
}

/// L117: "The bound TaskList MUST belong to the same account that owns
/// the Space"; `T2` is on `A2`, so L125's `invalidProperties` naming
/// `taskListId`.
#[tokio::test]
async fn other_account_task_list_is_invalid() {
    assert_eq!(
        check(&tasks(), json!("T2")).await,
        Err((json!("invalidProperties"), json!(["taskListId"])))
    );
}

/// L125: "does not reference an existing TaskList on the same account"
/// is `invalidProperties` naming `taskListId`.
#[tokio::test]
async fn unknown_task_list_is_invalid() {
    assert_eq!(
        check(&tasks(), json!("T9")).await,
        Err((json!("invalidProperties"), json!(["taskListId"])))
    );
}

/// L114: `taskListId` is a String (or `null` per L125).
#[tokio::test]
async fn non_string_task_list_id_is_invalid() {
    for value in [json!(1), json!(["T1"]), json!({ "id": "T1" })] {
        assert_eq!(
            check(&tasks(), value).await,
            Err((json!("invalidProperties"), json!(["taskListId"])))
        );
    }
}

/// L125 ("setting it to `null` unbinds the Space") and L129 (the TaskList
/// "may be unbound from the Space without being deleted"): `null` is
/// accepted and `T1` is still there.
#[tokio::test]
async fn unbind_keeps_the_task_list() {
    let tasks = tasks();
    assert_eq!(check(&tasks, Value::Null).await, Ok(()));
    let (listed, not_found) = task_list_get(&tasks, ACCOUNT, json!(["T1"])).await;
    assert_eq!(listed, ["T1"]);
    assert_eq!(not_found, json!([]));
}

/// L127: auto-bind creates a TaskList on the Space's account and sets
/// `taskListId` to its id before the Space is stored.
#[tokio::test]
async fn auto_bind_sets_task_list_id_to_the_new_task_list() {
    let tasks = tasks();
    let mut space = space(json!({}));
    let id = create_space_task_list(&tasks, &(), &Id::from(ACCOUNT), &mut space)
        .await
        .expect("create TaskList");
    assert_eq!(space.extra["taskListId"], json!(id.as_ref()));
    let (listed, _) = task_list_get(&tasks, ACCOUNT, json!([id.as_ref()])).await;
    assert_eq!(listed, [id.as_ref()]);
}

/// L125: a Space created already bound by the client keeps that binding
/// and no further TaskList is created.
#[tokio::test]
async fn auto_bind_keeps_a_client_binding() {
    let tasks = tasks();
    let mut space = space(json!({ "taskListId": "T1" }));
    let id = create_space_task_list(&tasks, &(), &Id::from(ACCOUNT), &mut space)
        .await
        .expect("create TaskList");
    assert_eq!(id.as_ref(), "T1");
    assert_eq!(space.extra["taskListId"], json!("T1"));
    let (listed, _) = task_list_get(&tasks, ACCOUNT, Value::Null).await;
    assert_eq!(listed, ["T1"]);
}
