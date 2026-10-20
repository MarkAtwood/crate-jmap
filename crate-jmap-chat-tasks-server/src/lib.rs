//! Chat binding for the JMAP Tasks method handlers: the Space
//! `taskListId` rules of draft-atwood-jmap-chat-tasks-00 that apply when a
//! server offers `urn:ietf:params:jmap:chat:tasks` together with
//! `urn:ietf:params:jmap:chat` and `urn:ietf:params:jmap:tasks`.
//!
//! `jmap-tasks-server` and `jmap-chat-server` do not depend on each other.
//! A deployment that offers both implements
//! [`jmap_tasks_server::TasksBackend`] and `ChatBackend` and calls these
//! functions from its `ChatBackend` impl:
//!
//! | Where | Call |
//! |---|---|
//! | `ChatBackend::space_extension_properties` | [`space_extension_properties`] |
//! | `ChatBackend::create_object::<Space>` and `ChatBackend::apply_space_metadata_patch`, for a `taskListId` value | [`check_task_list_id`] |
//! | `ChatBackend::create_object::<Space>`, before the insert, for auto-bind on create | [`create_space_task_list`] |
//!
//! Space destroy needs no call: this crate never destroys a TaskList
//! (§Binding and unbinding: "the bound TaskList SHOULD NOT be
//! automatically destroyed unless explicitly requested by the destroying
//! member"). The draft gives the destroying member no way to make that
//! request on `Space/set`, so the TaskList always persists; a member who
//! wants it gone destroys it with `TaskList/set`.

#![forbid(unsafe_code)]

use jmap_chat_types::{ChatTasksCapability, Space, SPACE_TASK_LIST_ID_PROPERTY};
use jmap_tasks_server::{BackendSetError, SetError, SetErrorType, TasksBackend};
use jmap_tasks_types::TaskList;
use jmap_types::Id;
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

/// An `invalidProperties` SetError naming `taskListId`.
fn invalid(description: &str) -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties([SPACE_TASK_LIST_ID_PROPERTY])
        .with_description(description)
}
