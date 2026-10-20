//! Chat binding for the JMAP Tasks method handlers: the Space
//! `taskListId` and Task `chatId` rules of draft-atwood-jmap-chat-tasks-00
//! that apply when a server offers `urn:ietf:params:jmap:chat:tasks`
//! together with `urn:ietf:params:jmap:chat` and
//! `urn:ietf:params:jmap:tasks`.
//!
//! `jmap-tasks-server` and `jmap-chat-server` do not depend on each other.
//! A deployment that offers both implements
//! [`jmap_tasks_server::TasksBackend`] and [`ChatBackend`] and calls these
//! functions from its trait methods:
//!
//! | Where | Call |
//! |---|---|
//! | `ChatBackend::space_extension_properties` | [`space_extension_properties`] |
//! | `ChatBackend::create_object::<Space>` and `ChatBackend::apply_space_metadata_patch`, for a `taskListId` value | [`check_task_list_id`] |
//! | `ChatBackend::create_object::<Space>`, before the insert, for auto-bind on create | [`create_space_task_list`] |
//! | `TasksBackend::destroy_object::<TaskList>`, after the destroy | [`clear_space_task_list_id`] |
//! | `ChatBackend::on_chat_destroyed` | [`clear_task_chat_ids`] |
//!
//! Task destroy needs no call: this crate never destroys a Chat
//! (§Lifecycle: "When a Task is destroyed (via `Task/set` destroy), the
//! referenced Chat is NOT automatically destroyed").
//!
//! Space destroy needs no call: this crate never destroys a TaskList
//! (§Binding and unbinding: "the bound TaskList SHOULD NOT be
//! automatically destroyed unless explicitly requested by the destroying
//! member"). The draft gives the destroying member no way to make that
//! request on `Space/set`, so the TaskList always persists; a member who
//! wants it gone destroys it with `TaskList/set`.

#![forbid(unsafe_code)]

use jmap_chat_server::ChatBackend;
use jmap_chat_types::{
    task_chat_id, ChatTasksCapability, Space, SPACE_TASK_LIST_ID_PROPERTY, TASK_CHAT_ID_PROPERTY,
};
use jmap_tasks_server::{BackendSetError, SetError, SetErrorType, TasksBackend};
use jmap_tasks_types::{Task, TaskList};
use jmap_types::{Id, PatchObject};
use serde_json::Value;

/// The Space extension properties to return from
/// `ChatBackend::space_extension_properties` for an account advertising
/// `capability`: `taskListId` when `mayBindTaskList` is `true`, none
/// otherwise.
///
/// With `mayBindTaskList: false` (draft L103: "`true` if accounts on this
/// server may associate a TaskList with a Space, `false` otherwise") no
/// Space is bound: the `Space/set` handler does not store `taskListId` on create
/// and rejects it in an update patch as `invalidProperties`. With `true`,
/// the handler stores it on create and routes a `taskListId` patch through
/// `apply_space_metadata_patch`, whose `manage_space` gate gives
/// §Binding and unbinding: servers "MUST accept patches to
/// `Space.taskListId` from members holding the `"manage_space"`
/// permission".
pub fn space_extension_properties(capability: &ChatTasksCapability) -> &'static [&'static str] {
    if capability.may_bind_task_list {
        &[SPACE_TASK_LIST_ID_PROPERTY]
    } else {
        &[]
    }
}

/// Check a `taskListId` value from a `Space/set` create or update of a
/// Space on `account_id`.
///
/// - `null` unbinds the Space (§Binding and unbinding: "setting it to
///   `null` unbinds the Space") and passes. The TaskList is not touched:
///   it "may be unbound from the Space without being deleted".
/// - Any other value must be an `Id` naming a TaskList that `tasks`
///   returns for `account_id`: "The bound TaskList MUST belong to the same
///   account that owns the Space" (§taskListId). Otherwise the result is
///   `invalidProperties` naming `taskListId` (§Binding and unbinding: "If
///   the `taskListId` value does not reference an existing TaskList on the
///   same account, the server MUST return a SetError of type
///   `invalidProperties` naming `taskListId`").
///
/// `caller` is the requesting user's Tasks context, so a TaskList the
/// user cannot see is refused like a missing one. Call this after the
/// `manage_space` gate of `apply_space_metadata_patch`; on create, the
/// Space's creator is its first member.
///
/// Returns `Ok(Err(set_error))` when the value must be refused; return
/// that SetError without storing the Space.
///
/// # Errors
///
/// Returns the Tasks backend's error if reading the TaskList fails.
pub async fn check_task_list_id<K: TasksBackend>(
    tasks: &K,
    caller: &K::CallerCtx,
    account_id: &Id,
    value: &Value,
) -> Result<Result<(), SetError>, K::Error> {
    let task_list_id = match value {
        Value::Null => return Ok(Ok(())),
        Value::String(s) => match Id::new_validated(s.as_str()) {
            Ok(id) => id,
            Err(_) => return Ok(Err(invalid("taskListId must be a valid Id"))),
        },
        _ => return Ok(Err(invalid("taskListId must be a String or null"))),
    };
    let ids = std::slice::from_ref(&task_list_id);
    let (found, _) = tasks
        .get_objects::<TaskList>(caller, account_id, Some(ids), None)
        .await?;
    Ok(if found.iter().any(|t| t.id == task_list_id) {
        Ok(())
    } else {
        Err(invalid(
            "taskListId must name a TaskList on the account that owns the Space",
        ))
    })
}

/// Auto-bind on create: create a TaskList for the new `space` on
/// `account_id` and set the Space's `taskListId` to it, for a server
/// advertising `mayBindTaskList: true` that binds every new Space.
///
/// §Binding and unbinding: "Servers MAY automatically create a new
/// TaskList when a Space is created and bind it ... When this is done, the
/// server MUST set `taskListId` to the new TaskList's id at Space creation
/// time." Call it from `ChatBackend::create_object::<Space>` before the
/// insert, so the stored Space and the `Space/set` `created` entry carry
/// the binding.
///
/// A `space` that already names a TaskList (the client supplied a
/// non-null `taskListId`, checked with [`check_task_list_id`]) is left as
/// it is and nothing is created.
///
/// The TaskList is named after the Space, subscribed, without a role, and
/// with every right for the account owner. It is passed to
/// [`TasksBackend::create_object`] with the placeholder id `TaskList/set`
/// also uses; the backend assigns the real id and records the create for
/// `TaskList/changes`. `caller` is a server context.
///
/// Returns the bound TaskList's id.
///
/// # Errors
///
/// Returns the backend's error from `create_object`.
// Same error type as `TasksBackend::create_object`, which it propagates.
#[allow(clippy::result_large_err)]
pub async fn create_space_task_list<K: TasksBackend>(
    tasks: &K,
    caller: &K::CallerCtx,
    account_id: &Id,
    space: &mut Space,
) -> Result<Id, BackendSetError<K::Error>> {
    if let Some(Value::String(existing)) = space.extra.get(SPACE_TASK_LIST_ID_PROPERTY) {
        return Ok(Id::from(existing.as_str()));
    }
    let task_list: TaskList = serde_json::from_value(serde_json::json!({
        "id": "placeholder",
        "name": space.name,
        "isSubscribed": true,
        "myRights": {
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayWriteOwn": true,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "mayAdmin": true,
            "mayDelete": true,
        },
    }))
    .expect("a TaskList with every required field deserializes");
    let (task_list_id, _) = tasks
        .create_object(caller, account_id, "spaceTaskList", task_list)
        .await?;
    space.extra.insert(
        SPACE_TASK_LIST_ID_PROPERTY.to_owned(),
        Value::String(task_list_id.as_ref().to_owned()),
    );
    Ok(task_list_id)
}

/// Clear `taskListId` on every Space of `account_id` bound to the
/// destroyed TaskList `task_list_id`.
///
/// §taskListId: "When a TaskList referenced by `Space.taskListId` is
/// destroyed, the server MUST clear the field (so that it is absent) as
/// part of the destroy operation." Call it from
/// `TasksBackend::destroy_object::<TaskList>` once the destroy succeeds,
/// so it runs for every `TaskList/set` destroy. Each Space is patched
/// with `taskListId: null`, which removes the property (RFC 8620 §5.3),
/// through [`ChatBackend::update_object`], which advances the Space state
/// so `Space/changes` lists it.
///
/// The Space and the TaskList are on the same account (§taskListId: "The
/// bound TaskList MUST belong to the same account that owns the Space"),
/// so only `account_id`'s Spaces are read. `caller` is a server context.
///
/// # Errors
///
/// Returns the Chat backend's error from reading or updating a Space.
/// Spaces cleared before the error stay cleared.
// Same error type as `ChatBackend::update_object`, which it propagates.
#[allow(clippy::result_large_err)]
pub async fn clear_space_task_list_id<C: ChatBackend>(
    chat: &C,
    caller: &C::CallerCtx,
    account_id: &Id,
    task_list_id: &Id,
) -> Result<(), BackendSetError<C::Error>> {
    // ponytail: reads every Space of the account; O(spaces) per TaskList
    // destroy. Upgrade: an index from TaskList id to bound Space.
    let (spaces, _) = chat
        .get_objects::<Space>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    for space in spaces
        .iter()
        .filter(|s| s.task_list_id().ok().flatten().as_ref() == Some(task_list_id))
    {
        chat.update_object::<Space>(
            caller,
            account_id,
            &space.id,
            clear_patch(SPACE_TASK_LIST_ID_PROPERTY),
        )
        .await?;
    }
    Ok(())
}

/// Clear `chatId` on every Task of `account_id` that back-references the
/// destroyed Chat `chat_id`.
///
/// §Lifecycle: "When a Chat referenced by one or more Tasks is destroyed,
/// the affected Tasks' `chatId` fields MUST be cleared (set to absent) by
/// the server as part of the destroy operation." Call it from
/// `ChatBackend::on_chat_destroyed`. Each Task is patched with
/// `chatId: null`, which removes the property (RFC 8620 §5.3), through
/// [`TasksBackend::update_object`], which advances the Task state so
/// `Task/changes` lists it.
///
/// The Chat and the Task are on the same account (§chatId: "The
/// referenced Chat, once it exists, MUST be on the same account as the
/// Task"), so only `account_id`'s Tasks are read. `caller` is a server
/// context for the Tasks backend.
///
/// # Errors
///
/// Returns the Tasks backend's error from reading or updating a Task.
/// Tasks cleared before the error stay cleared.
// Same error type as `TasksBackend::update_object`, which it propagates.
#[allow(clippy::result_large_err)]
pub async fn clear_task_chat_ids<K: TasksBackend>(
    tasks: &K,
    caller: &K::CallerCtx,
    account_id: &Id,
    chat_id: &Id,
) -> Result<(), BackendSetError<K::Error>> {
    // ponytail: reads every Task of the account; O(tasks) per Chat
    // destroy. Upgrade: an index from Chat id to back-referencing Tasks.
    let (all, _) = tasks
        .get_objects::<Task>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    let referencing = all
        .iter()
        .filter(|t| task_chat_id(&t.extra).ok().flatten().as_ref() == Some(chat_id))
        .filter_map(|t| t.id.as_ref());
    for task_id in referencing {
        tasks
            .update_object::<Task>(
                caller,
                account_id,
                task_id,
                clear_patch(TASK_CHAT_ID_PROPERTY),
            )
            .await?;
    }
    Ok(())
}

/// A patch setting `property` to `null`.
fn clear_patch(property: &str) -> PatchObject {
    let mut patch = PatchObject::new();
    patch.as_map_mut().insert(property.to_owned(), Value::Null);
    patch
}

/// An `invalidProperties` SetError naming `taskListId`.
fn invalid(description: &str) -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties([SPACE_TASK_LIST_ID_PROPERTY])
        .with_description(description)
}
