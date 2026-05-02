# jmap-tasks-server — Implementation Plan

Backend-agnostic JMAP Tasks method handlers.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-tasks-06.txt`

## Crate Family Position

```
jmap-types
    └── jmap-tasks-types
            └── jmap-tasks-server  ← this crate
```

## What This Crate Is

Method handlers for JMAP Tasks, plugged into `jmap-server::Dispatcher` via
`register_tasks_handlers(dispatcher, backend)`.

## Methods (draft-ietf-jmap-tasks-06)

- `TaskList/get`, `TaskList/set`, `TaskList/changes`, `TaskList/query`
- `Task/get`, `Task/set`, `Task/changes`, `Task/query`, `Task/queryChanges`
- `TaskNotification/get`, `TaskNotification/changes`, `TaskNotification/query`

## Backend Trait

```rust
pub trait TasksBackend: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    async fn get_objects<O>(...) -> Result<...>;
    async fn set_objects<O>(...) -> Result<...>;
    // etc.
}
```

## Module Layout

```
src/
  lib.rs            TasksBackend trait, register_tasks_handlers
  tasklist.rs       TaskList/get, set, changes, query
  task.rs           Task/get, set, changes, query, queryChanges
  notification.rs   TaskNotification/get, changes, query
  backend.rs        TasksBackend trait, error types
  helpers.rs        shared utilities
```

## Pattern to Follow

`~/PROJECT/JMAP/crate-jmap-mail-server/` — identical handler structure.
`~/PROJECT/JMAP/crate-jmap-calendars-server/` — very close structural analog.

## Caveat

draft-ietf-jmap-tasks-06 is an early draft. The API surface may change before publication.
Read the spec carefully before implementing — do not assume it mirrors Calendars exactly.
