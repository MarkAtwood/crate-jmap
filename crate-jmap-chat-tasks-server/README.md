# jmap-chat-tasks-server

The Chat binding for `jmap-tasks-server`: the Space `taskListId` rules of
draft-atwood-jmap-chat-tasks-00 that apply when one server offers
`urn:ietf:params:jmap:chat:tasks` with `urn:ietf:params:jmap:chat` and
`urn:ietf:params:jmap:tasks`. `jmap-tasks-server` and `jmap-chat-server`
do not depend on each other; this crate depends on the Tasks server and
the Chat types.

## What it is

| Item | Spec |
|---|---|
| `space_extension_properties` | chat-tasks §Account-Level Capability Object `mayBindTaskList`: register `taskListId` as a Space extension property only when binding is allowed |
| `check_task_list_id` | chat-tasks §taskListId, §Binding and unbinding: `null` unbinds; otherwise the value must name a TaskList on the Space's own account (`invalidProperties` naming `taskListId`) |
| `create_space_task_list` | chat-tasks §Binding and unbinding auto-bind on create: create a TaskList and set the Space's `taskListId` before it is stored |

## What it's for

A deployment that implements both `TasksBackend` and `ChatBackend` calls
these functions:

| Where | Call |
|---|---|
| `ChatBackend::space_extension_properties` | `space_extension_properties` |
| `ChatBackend::create_object::<Space>` and `apply_space_metadata_patch`, for a `taskListId` value | `check_task_list_id` |
| `ChatBackend::create_object::<Space>`, before the insert, for auto-bind | `create_space_task_list` |

## How to use

```rust,ignore
// Inside ChatBackend::apply_space_metadata_patch, after the manage_space gate:
if let Some(value) = patch.extra.get(SPACE_TASK_LIST_ID_PROPERTY) {
    check_task_list_id(&self.tasks, &tasks_caller, account_id, value)
        .await
        .map_err(|e| BackendSetError::Other(e.into()))?
        .map_err(BackendSetError::SetError)?;
}
```

## How it works

`taskListId` travels through the `jmap-chat-server` Space extension hook:
it is stored in `Space::extra` on create and patched through
`apply_space_metadata_patch`, whose `manage_space` gate covers the
draft's "members holding the `"manage_space"` permission". This crate only
checks the value against the Tasks backend.

## Gotchas

- Space destroy has no hook here: the TaskList is never destroyed with
  the Space (§Binding and unbinding "SHOULD NOT be automatically
  destroyed"). `Space/set` carries no way to request it; a member destroys
  the TaskList with `TaskList/set`.
- `check_task_list_id` reads with the caller's Tasks context, so a
  TaskList the caller cannot see is refused like a missing one. The read
  is not atomic with the Space write.
- With `mayBindTaskList: false`, `Space/set` create drops a `taskListId`
  rather than refusing it; update refuses it (JMAP-zmqp).
- `create_space_task_list` keeps a client-supplied `taskListId` (check it
  first) and names a new TaskList after the Space.
