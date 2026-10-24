//! Destroy cascades between the Tasks and Chat layers.
//!
//! Oracle: draft-atwood-jmap-chat-tasks-00.md at jmap-chat-spec be46208
//! ("tasks Lnnn") and RFC 8620:
//!
//! - tasks L119: "When a TaskList referenced by `Space.taskListId` is
//!   destroyed, the server MUST clear the field (so that it is absent) as
//!   part of the destroy operation."
//! - tasks L165: "When a Task is destroyed (via `Task/set` destroy), the
//!   referenced Chat is NOT automatically destroyed. The Chat persists
//!   independently."
//! - tasks L167: "When a Chat referenced by one or more Tasks is
//!   destroyed, the affected Tasks' `chatId` fields MUST be cleared (set
//!   to absent) by the server as part of the destroy operation."
//! - RFC 8620 §5.2 `/changes` `updated`: "An array of ids for records that
//!   have been updated since the old state."
//!
//! Records are hand-written JSON from the drafts' field lists.

use jmap_chat_server::memory::MemoryBackend as ChatMemory;
use jmap_chat_server::{ChatBackend, handle_space_changes, handle_space_get, handle_space_set};
use jmap_chat_tasks_server::{
    clear_space_task_list_id, clear_task_chat_ids, space_extension_properties,
};
use jmap_chat_types::{Chat, ChatTasksCapability};
use jmap_tasks_server::memory::MemoryBackend as TasksMemory;
use jmap_tasks_server::{
    handle_task_changes, handle_task_get, handle_task_list_set, handle_task_set,
};
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "A1";

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

/// A Task in TaskList `TL` with `extra` merged in.
fn task_json(id: &str, extra: Value) -> Value {
    let mut v = json!({
        "id": id, "@type": "Task", "uid": format!("{id}-uid"),
        "title": "Ship it", "taskListId": "TL"
    });
    v.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("object").clone());
    v
}

/// A chat backend storing the Space `taskListId` extension (L103
/// `mayBindTaskList: true`).
fn chat() -> ChatMemory {
    let capability: ChatTasksCapability = serde_json::from_value(json!({
        "mayBindTaskList": true,
        "mayBackReferenceChat": true
    }))
    .expect("capability fixture");
    let chat = ChatMemory::new();
    chat.register_account(&Id::from(ACCOUNT));
    chat.set_space_extension_properties_for_test(space_extension_properties(&capability));
    chat
}

async fn space_set(chat: &ChatMemory, args: Value) -> Value {
    handle_space_set(chat, &(), &std::collections::BTreeMap::new(), args, &[])
        .await
        .expect("Space/set")
        .primary
}

/// Create a Space bound to `task_list_id`; return its id.
async fn create_space(chat: &ChatMemory, task_list_id: &str) -> String {
    let out = space_set(
        chat,
        json!({ "accountId": ACCOUNT,
                "create": { "s": { "name": "Team", "taskListId": task_list_id } } }),
    )
    .await;
    out["created"]["s"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Space not created: {out}"))
        .to_owned()
}

/// `Space/get` of `id`.
async fn space_get(chat: &ChatMemory, id: &str) -> (Value, String) {
    let out = handle_space_get(chat, &(), json!({ "accountId": ACCOUNT, "ids": [id] }))
        .await
        .expect("Space/get")
        .primary;
    (
        out["list"][0].clone(),
        out["state"].as_str().expect("state").to_owned(),
    )
}

/// `Task/get` of every Task: the Tasks by id, and the state.
async fn task_get(tasks: &TasksMemory) -> (serde_json::Map<String, Value>, String) {
    let out = handle_task_get(tasks, &(), json!({ "accountId": ACCOUNT, "ids": null }))
        .await
        .expect("Task/get")
        .primary;
    let by_id = out["list"]
        .as_array()
        .expect("list")
        .iter()
        .map(|t| (t["id"].as_str().expect("id").to_owned(), t.clone()))
        .collect();
    (by_id, out["state"].as_str().expect("state").to_owned())
}

/// Whether `chat` returns Chat `id` on [`ACCOUNT`].
async fn chat_exists<C: ChatBackend<CallerCtx = ()>>(chat: &C, id: &str) -> bool {
    let ids = [Id::from(id)];
    let (found, _) = chat
        .get_objects::<Chat>(&(), &Id::from(ACCOUNT), Some(&ids), None)
        .await
        .expect("Chat read");
    found.iter().any(|c| c.id.as_ref() == id)
}

/// L119: after `TaskList/set` destroys `T1`, the Space bound to it has no
/// `taskListId` ("so that it is absent") and `Space/changes` lists it as
/// updated. A Space bound to another TaskList keeps its binding.
#[tokio::test]
async fn task_list_destroy_clears_space_task_list_id() {
    let tasks = TasksMemory::new().with_account(&Id::from(ACCOUNT));
    tasks.seed_object(
        &Id::from(ACCOUNT),
        "TaskList",
        &Id::from("T1"),
        task_list_json("T1"),
    );
    tasks.seed_object(
        &Id::from(ACCOUNT),
        "TaskList",
        &Id::from("T2"),
        task_list_json("T2"),
    );
    let chat = chat();
    let bound = create_space(&chat, "T1").await;
    let other = create_space(&chat, "T2").await;
    let (_, before) = space_get(&chat, &bound).await;

    let out = handle_task_list_set(
        &tasks,
        &(),
        json!({ "accountId": ACCOUNT, "destroy": ["T1"] }),
        &[],
    )
    .await
    .expect("TaskList/set")
    .primary;
    assert_eq!(out["destroyed"], json!(["T1"]), "{out}");
    clear_space_task_list_id(&chat, &(), &Id::from(ACCOUNT), &Id::from("T1"))
        .await
        .expect("clear taskListId");

    let (space, _) = space_get(&chat, &bound).await;
    assert_eq!(space.get("taskListId"), None, "{space}");
    let (space, _) = space_get(&chat, &other).await;
    assert_eq!(space["taskListId"], json!("T2"));
    let changes = handle_space_changes(
        &chat,
        &(),
        json!({ "accountId": ACCOUNT, "sinceState": before }),
        &[],
    )
    .await
    .expect("Space/changes")
    .primary;
    assert_eq!(changes["updated"], json!([bound]), "{changes}");
}

/// L167: destroying Chat `C1` clears `chatId` ("set to absent") on every
/// Task that references it ("one or more Tasks"), and `Task/changes`
/// lists exactly those Tasks as updated. Tasks referencing another Chat,
/// or none, are unchanged.
#[tokio::test]
async fn chat_destroy_clears_task_chat_ids() {
    let tasks = TasksMemory::new().with_account(&Id::from(ACCOUNT));
    tasks.seed_object(
        &Id::from(ACCOUNT),
        "TaskList",
        &Id::from("TL"),
        task_list_json("TL"),
    );
    tasks.seed_object(
        &Id::from(ACCOUNT),
        "Task",
        &Id::from("t1"),
        task_json("t1", json!({ "chatId": "C1" })),
    );
    tasks.seed_object(
        &Id::from(ACCOUNT),
        "Task",
        &Id::from("t2"),
        task_json("t2", json!({ "chatId": "C1" })),
    );
    tasks.seed_object(
        &Id::from(ACCOUNT),
        "Task",
        &Id::from("t3"),
        task_json("t3", json!({ "chatId": "C2" })),
    );
    tasks.seed_object(
        &Id::from(ACCOUNT),
        "Task",
        &Id::from("t4"),
        task_json("t4", json!({})),
    );
    let (_, before) = task_get(&tasks).await;

    clear_task_chat_ids(&tasks, &(), &Id::from(ACCOUNT), &Id::from("C1"))
        .await
        .expect("clear chatId");

    let (by_id, _) = task_get(&tasks).await;
    assert_eq!(by_id["t1"].get("chatId"), None, "{}", by_id["t1"]);
    assert_eq!(by_id["t2"].get("chatId"), None, "{}", by_id["t2"]);
    assert_eq!(by_id["t3"]["chatId"], json!("C2"));
    assert_eq!(by_id["t4"].get("chatId"), None);
    let changes = handle_task_changes(
        &tasks,
        &(),
        json!({ "accountId": ACCOUNT, "sinceState": before }),
        &[],
    )
    .await
    .expect("Task/changes")
    .primary;
    let mut updated: Vec<&str> = changes["updated"]
        .as_array()
        .expect("updated")
        .iter()
        .map(|v| v.as_str().expect("id"))
        .collect();
    updated.sort_unstable();
    assert_eq!(updated, ["t1", "t2"], "{changes}");
}

/// L165: `Task/set` destroy of a Task whose `chatId` is `C1` leaves Chat
/// `C1` in place ("The Chat persists independently").
#[tokio::test]
async fn task_destroy_leaves_its_chat() {
    let tasks = TasksMemory::new().with_account(&Id::from(ACCOUNT));
    tasks.seed_object(
        &Id::from(ACCOUNT),
        "TaskList",
        &Id::from("TL"),
        task_list_json("TL"),
    );
    tasks.seed_object(
        &Id::from(ACCOUNT),
        "Task",
        &Id::from("t1"),
        task_json("t1", json!({ "chatId": "C1" })),
    );
    let chat = chat();
    chat.insert_object_for_test(
        "Chat",
        ACCOUNT,
        "C1",
        json!({ "id": "C1", "kind": "channel", "createdAt": "2026-01-01T00:00:00Z",
                "unreadCount": 0, "spaceId": "S1" }),
    );

    let out = handle_task_set(
        &tasks,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT, "destroy": ["t1"] }),
        &[],
    )
    .await
    .expect("Task/set")
    .primary;
    assert_eq!(out["destroyed"], json!(["t1"]), "{out}");
    assert!(chat_exists(&chat, "C1").await);
}
