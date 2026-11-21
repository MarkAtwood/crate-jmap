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
//! | Session object, before advertising `urn:ietf:params:jmap:chat:tasks` on an account | [`may_advertise`] |
//! | `ChatSpaces::space_extension_properties` | [`space_extension_properties`] |
//! | `ChatStore::create_object::<Space>` and `ChatSpaces::apply_space_metadata_patch`, for a `taskListId` value | [`check_task_list_id`] |
//! | `ChatStore::create_object::<Space>`, before the insert, for auto-bind on create | [`create_space_task_list`] |
//! | `TasksBackend::destroy_object::<TaskList>`, after the destroy | [`clear_space_task_list_id`] |
//! | `ChatEvents::on_chat_destroyed` | [`clear_task_chat_ids`] |
//! | `TasksBackend::create_object::<Task>` and `TasksBackend::update_object::<Task>`, for a `chatId` value | [`check_task_chat_id`] |
//! | `JmapReads::get_objects::<Task>`, on the Tasks returned to the reader | [`redact_task_chat_ids`] |
//!
//! Task destroy needs no call: this crate never destroys a Chat
//! (§Lifecycle: "When a Task is destroyed (via `Task/set` destroy), the
//! referenced Chat is NOT automatically destroyed").
//!
//! Space destroy needs no call: this crate never destroys a [`TaskList`]
//! (§Binding and unbinding: "the bound `TaskList` SHOULD NOT be
//! automatically destroyed unless explicitly requested by the destroying
//! member"). The draft gives the destroying member no way to make that
//! request on `Space/set`, so the `TaskList` always persists; a member who
//! wants it gone destroys it with `TaskList/set`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

use jmap_chat_server::ChatBackend;
use jmap_chat_types::{
    ChatTasksCapability, JMAP_CHAT_URI, SPACE_TASK_LIST_ID_PROPERTY, Space, TASK_CHAT_ID_PROPERTY,
    task_chat_id,
};
use jmap_server::{BackendSetError, SetError, SetErrorType};
use jmap_tasks_server::TasksBackend;
use jmap_tasks_types::{JMAP_TASKS_URI, Task, TaskList};
use jmap_types::{Id, PatchObject};
use serde_json::Value;

/// Whether an account may advertise `urn:ietf:params:jmap:chat:tasks`,
/// given the capability URIs it already advertises.
///
/// draft-atwood-jmap-chat-tasks-00 §Capability: "This capability is
/// meaningful only when both `urn:ietf:params:jmap:chat` and
/// `urn:ietf:params:jmap:tasks` are also advertised on the same account."
/// §Relationship to JMAP Tasks: "A deployment that supports JMAP Chat but
/// not JMAP Tasks MUST NOT advertise the capability defined here."
/// Returns `true` only when both URIs are present.
pub fn may_advertise<'a>(account_capabilities: impl IntoIterator<Item = &'a str>) -> bool {
    let (mut chat, mut tasks) = (false, false);
    for uri in account_capabilities {
        chat |= uri == JMAP_CHAT_URI;
        tasks |= uri == JMAP_TASKS_URI;
    }
    chat && tasks
}

/// The Space extension properties to return from
/// `ChatSpaces::space_extension_properties` for an account advertising
/// `capability`: `taskListId` when `mayBindTaskList` is `true`, none
/// otherwise.
///
/// With `mayBindTaskList: false` (draft L103: "`true` if accounts on this
/// server may associate a [`TaskList`] with a Space, `false` otherwise") no
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
///   `null` unbinds the Space") and passes. The [`TaskList`] is not touched:
///   it "may be unbound from the Space without being deleted".
/// - Any other value must be an `Id` naming a `TaskList` that `tasks`
///   returns for `account_id`: "The bound `TaskList` MUST belong to the same
///   account that owns the Space" (§taskListId). Otherwise the result is
///   `invalidProperties` naming `taskListId` (§Binding and unbinding: "If
///   the `taskListId` value does not reference an existing `TaskList` on the
///   same account, the server MUST return a [`SetError`] of type
///   `invalidProperties` naming `taskListId`").
///
/// `caller` is the requesting user's Tasks context, so a `TaskList` the
/// user cannot see is refused like a missing one. Call this after the
/// `manage_space` gate of `apply_space_metadata_patch`; on create, the
/// Space's creator is its first member.
///
/// Returns `Ok(Err(set_error))` when the value must be refused; return
/// that `SetError` without storing the Space.
///
/// # Errors
///
/// Returns the Tasks backend's error if reading the `TaskList` fails.
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
            Err(_) => {
                return Ok(Err(invalid(
                    SPACE_TASK_LIST_ID_PROPERTY,
                    "taskListId must be a valid Id",
                )));
            }
        },
        _ => {
            return Ok(Err(invalid(
                SPACE_TASK_LIST_ID_PROPERTY,
                "taskListId must be a String or null",
            )));
        }
    };
    let ids = std::slice::from_ref(&task_list_id);
    let (found, _) = tasks
        .get_objects::<TaskList>(caller, account_id, Some(ids), None)
        .await?;
    Ok(if found.iter().any(|t| t.id == task_list_id) {
        Ok(())
    } else {
        Err(invalid(
            SPACE_TASK_LIST_ID_PROPERTY,
            "taskListId must name a TaskList on the account that owns the Space",
        ))
    })
}

/// Auto-bind on create: create a [`TaskList`] for the new `space` on
/// `account_id` and set the Space's `taskListId` to it, for a server
/// advertising `mayBindTaskList: true` that binds every new Space.
///
/// §Binding and unbinding: "Servers MAY automatically create a new
/// `TaskList` when a Space is created and bind it ... When this is done, the
/// server MUST set `taskListId` to the new `TaskList`'s id at Space creation
/// time." Call it from `ChatStore::create_object::<Space>` before the
/// insert, so the stored Space and the `Space/set` `created` entry carry
/// the binding.
///
/// A `space` that already names a `TaskList` (the client supplied a
/// non-null `taskListId`, checked with [`check_task_list_id`]) is left as
/// it is and nothing is created.
///
/// The `TaskList` is named after the Space, subscribed, without a role, and
/// with every right for the account owner. It is built by the
/// [`TasksBackend::create_object`] builder for the id the backend assigns,
/// and the backend records the create for `TaskList/changes`. `caller` is
/// a server context.
///
/// Returns the bound `TaskList`'s id.
///
/// # Errors
///
/// Returns the backend's error from `create_object`, or, before anything is
/// stored, [`BackendSetError::SetError`] with an `invalidProperties`
/// [`SetError`] naming `name` if the `TaskList` cannot be built from the
/// Space's name.
// Same error type as `TasksBackend::create_object`, which it propagates.
pub async fn create_space_task_list<K: TasksBackend>(
    tasks: &K,
    caller: &K::CallerCtx,
    account_id: &Id,
    space: &mut Space,
) -> Result<Id, BackendSetError<K::Error>> {
    if let Some(Value::String(existing)) = space.extra.get(SPACE_TASK_LIST_ID_PROPERTY) {
        // Id::from: this crate stored the id when it created the record.
        return Ok(Id::from(existing.as_str()));
    }
    let build = space_task_list_builder(&space.name)?;
    let (task_list_id, _) = tasks
        .create_object(caller, account_id, "spaceTaskList", build)
        .await?;
    space.extra.insert(
        SPACE_TASK_LIST_ID_PROPERTY.to_owned(),
        Value::String(task_list_id.as_ref().to_owned()),
    );
    Ok(task_list_id)
}

/// The [`TasksBackend::create_object`] builder for the `TaskList` that
/// [`create_space_task_list`] binds to a Space named `name`.
///
/// The `TaskList` is deserialized once, with a placeholder id, before the
/// backend is called; the builder then sets the id the backend assigns
/// directly, as `TaskList/set` create does. An id the backend built
/// unchecked with `Id::from` is stored as given, where deserializing it
/// would fail the RFC 8620 Id check.
fn space_task_list_builder(
    name: &str,
) -> Result<impl FnOnce(&Id) -> TaskList + Send + use<>, Box<SetError>> {
    let mut task_list: TaskList = serde_json::from_value(serde_json::json!({
        "id": "spaceTaskList",
        "name": name,
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
    .map_err(|e| {
        Box::new(
            SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["name"])
                .with_description(format!("the Space's TaskList cannot be built: {e}")),
        )
    })?;
    Ok(move |id: &Id| {
        task_list.id = id.clone();
        task_list
    })
}

/// Clear `taskListId` on every Space of `account_id` bound to the
/// destroyed [`TaskList`] `task_list_id`.
///
/// §taskListId: "When a `TaskList` referenced by `Space.taskListId` is
/// destroyed, the server MUST clear the field (so that it is absent) as
/// part of the destroy operation." Call it from
/// `TasksBackend::destroy_object::<TaskList>` once the destroy succeeds,
/// so it runs for every `TaskList/set` destroy. Each Space is patched
/// with `taskListId: null`, which removes the property (RFC 8620 §5.3),
/// through [`ChatStore::update_object`](jmap_chat_server::ChatStore::update_object), which advances the Space state
/// so `Space/changes` lists it.
///
/// The Space and the `TaskList` are on the same account (§taskListId: "The
/// bound `TaskList` MUST belong to the same account that owns the Space"),
/// so only `account_id`'s Spaces are read. `caller` is a server context.
///
/// # Errors
///
/// Returns the Chat backend's error from reading or updating a Space.
/// Spaces cleared before the error stay cleared.
// Same error type as `ChatStore::update_object`, which it propagates.
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
/// `ChatEvents::on_chat_destroyed`. Each Task is patched with
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

/// Check a `chatId` value from a `Task/set` create or update of a Task,
/// for an account advertising `capability`.
///
/// - With `mayBackReferenceChat: false` every value is refused: the flag
///   is "`true` if tasks may carry a `chatId` back-reference to a
///   discussion Chat ..., `false` otherwise" (§Capability).
/// - `null` removes the back-reference (§chatId: "When absent, this Task
///   has no associated discussion thread") and passes.
/// - Any other value must be an `Id`. It is not looked up: "The server
///   MUST NOT validate `chatId` existence at write time, allowing the
///   two-step creation pattern" (§chatId), in which the Task may be
///   written before its Chat exists (§Operations: "The two operations MAY
///   be performed in either order").
///
/// The value is a bare `Id`, read on the Task's own account, so it cannot
/// name a Chat on another account: "The referenced Chat, once it exists,
/// MUST be on the same account as the Task" holds by construction.
///
/// # Errors
///
/// Returns an `invalidProperties` [`SetError`] naming `chatId` when the value
/// must be refused; return it without storing the Task.
// The SetError is the refusal itself, returned verbatim in `notCreated`
// or `notUpdated`.
pub fn check_task_chat_id(
    capability: &ChatTasksCapability,
    value: &Value,
) -> Result<(), Box<SetError>> {
    if !capability.may_back_reference_chat {
        return Err(Box::new(invalid(
            TASK_CHAT_ID_PROPERTY,
            "this server does not support Task chatId (mayBackReferenceChat is false)",
        )));
    }
    match value {
        Value::Null => Ok(()),
        Value::String(s) if Id::new_validated(s.as_str()).is_ok() => Ok(()),
        Value::String(_) => Err(Box::new(invalid(
            TASK_CHAT_ID_PROPERTY,
            "chatId must be a valid Id",
        ))),
        _ => Err(Box::new(invalid(
            TASK_CHAT_ID_PROPERTY,
            "chatId must be a String or null",
        ))),
    }
}

/// Remove `chatId` from each of `tasks` whose Chat the reader `caller`
/// may not view, as [`ChatAccessPolicy::may_view_chat`](jmap_chat_server::ChatAccessPolicy::may_view_chat) decides.
///
/// §Chat back-reference leakage: "Deployments MAY hide the `chatId` field
/// from Task readers who lack access to the referenced Chat. The wire
/// contract permits this (it is equivalent to the Task not exposing the
/// field to that reader)". Call it on the Tasks of `account_id` that
/// `JmapReads::get_objects::<Task>` returns to a reader, with that
/// reader's Chat context. Do not call it on Tasks the backend writes back
/// or on reads made with a server context, such as the one
/// [`clear_task_chat_ids`] uses, or the redacted `chatId` is lost.
///
/// `may_view_chat` defaults to `Ok(true)` (fail-open, bd:JMAP-bghk), so a
/// Chat backend that does not override it hides nothing. The hook is
/// asked once per distinct `chatId`. Its answer for a Chat that does not
/// exist yet (§Operations: "a Task with `chatId` set to a Chat that does
/// not yet exist on this server is a brief inconsistency") is the
/// backend's to give.
///
/// # Errors
///
/// Returns the Chat backend's error from `may_view_chat`. `tasks` is then
/// left unchanged.
pub async fn redact_task_chat_ids<C: ChatBackend>(
    chat: &C,
    caller: &C::CallerCtx,
    account_id: &Id,
    tasks: &mut [Task],
) -> Result<(), C::Error> {
    let mut viewable: HashMap<Id, bool> = HashMap::new();
    for chat_id in tasks
        .iter()
        .filter_map(|t| task_chat_id(&t.extra).ok().flatten())
    {
        if let Entry::Vacant(slot) = viewable.entry(chat_id) {
            let may_view = chat.may_view_chat(caller, account_id, slot.key()).await?;
            slot.insert(may_view);
        }
    }
    for task in tasks.iter_mut() {
        if task_chat_id(&task.extra)
            .ok()
            .flatten()
            .is_some_and(|id| viewable.get(&id) == Some(&false))
        {
            task.extra.remove(TASK_CHAT_ID_PROPERTY);
        }
    }
    Ok(())
}

/// For each of `tasks` (Tasks of `account_id`) whose `chatId` the reader
/// `caller` may not view, as [`ChatAccessPolicy::may_view_chat`](jmap_chat_server::ChatAccessPolicy::may_view_chat) decides, the
/// Task's id mapped to `["chatId"]`.
///
/// This is the draft-ietf-jmap-conditional-02 §9.1 counterpart of
/// [`redact_task_chat_ids`], with the same decision: a deployment that
/// hides `chatId` from such readers returns this from its
/// `ConditionalBackend::hidden_properties` for `"Task"`, so an
/// `ifUnchangedBy` precondition on `chatId` is answered `forbidden` without
/// comparing the value (otherwise its answer would reveal the hidden
/// `chatId`). Read `tasks` with a server context, unredacted, so their
/// `chatId` is there to check.
///
/// `may_view_chat` is asked once per distinct `chatId`; Tasks without one,
/// or without an id, are left out.
///
/// # Errors
///
/// Returns the Chat backend's error from `may_view_chat`.
pub async fn task_hidden_properties<C: ChatBackend>(
    chat: &C,
    caller: &C::CallerCtx,
    account_id: &Id,
    tasks: &[Task],
) -> Result<BTreeMap<Id, Vec<String>>, C::Error> {
    let mut viewable: HashMap<Id, bool> = HashMap::new();
    let mut hidden = BTreeMap::new();
    for task in tasks {
        let (Some(id), Ok(Some(chat_id))) = (&task.id, task_chat_id(&task.extra)) else {
            continue;
        };
        let may_view = match viewable.entry(chat_id) {
            Entry::Occupied(known) => *known.get(),
            Entry::Vacant(slot) => {
                let may_view = chat.may_view_chat(caller, account_id, slot.key()).await?;
                *slot.insert(may_view)
            }
        };
        if !may_view {
            hidden.insert(id.clone(), vec![TASK_CHAT_ID_PROPERTY.to_owned()]);
        }
    }
    Ok(hidden)
}

/// A patch setting `property` to `null`.
fn clear_patch(property: &str) -> PatchObject {
    let mut patch = PatchObject::new();
    patch.as_map_mut().insert(property.to_owned(), Value::Null);
    patch
}

/// An `invalidProperties` [`SetError`] naming `property`.
fn invalid(property: &str, description: &str) -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties([property])
        .with_description(description)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The auto-bind builder sets the id the backend assigns without
    /// deserializing it, so an id built unchecked with `Id::from` that is
    /// no RFC 8620 Id (§1.2: 1-255 octets of `A-Za-z0-9_-`) is kept as given
    /// instead of panicking. The rest of the `TaskList` is unchanged.
    #[test]
    fn space_task_list_builder_takes_an_unchecked_id() {
        let build = space_task_list_builder("Team").expect("a TaskList for any name");
        let task_list = build(&Id::from("not an id!"));
        assert_eq!(task_list.id.as_ref(), "not an id!");
        let wire = serde_json::to_value(&task_list).expect("TaskList serializes");
        assert_eq!(wire["name"], "Team");
        assert_eq!(wire["isSubscribed"], true);
        assert_eq!(wire["myRights"]["mayAdmin"], true);
    }
}
