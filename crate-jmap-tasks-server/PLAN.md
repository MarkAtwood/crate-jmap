# jmap-tasks-server — Implementation Plan

Backend-agnostic JMAP Tasks method handlers (draft-ietf-jmap-tasks-06). Plugs
into `jmap-server`'s `Dispatcher`. Defines a `TasksBackend` trait; consumers
provide the implementation.

## Crate Family Position

```
jmap-types
    ├── jmap-server          dispatcher
    └── jmap-tasks-types     data types
            └── jmap-tasks-server  ← this crate
```

## What This Crate Is

Method handler implementations for every JMAP Tasks method: TaskList, Task,
TaskNotification.

Defines a `TasksBackend` trait that the application implements. The crate
handles all JMAP protocol semantics (ordering, partial success, ResultReference
threading, error type mapping, TaskNotification generation rules). The backend
handles storage.

## What This Crate Is Not

- Not a full JMAP server
- Not coupled to any specific storage (SQLite, PostgreSQL, in-memory)
- Not handling auth — caller's responsibility before `Dispatcher::dispatch()`
- Not a CalDAV bridge — iCalendar VTODO parsing is outside scope
- Not axum-specific — any `http`-based framework works

## Source Material

### Normative

`~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-tasks-06.txt` — read the
relevant section before implementing each handler. Wire field names and error
types come from the spec, not from memory.

`~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base protocol method
semantics (§5.1 get, §5.2 changes, §5.3 set, §5.4 copy, §5.5 query, §5.6
queryChanges).

### Backend trait pattern — copy this

`~/PROJECT/crate-jmap/crate-jmap-mail-server/src/backend.rs`

`JmapBackend`, `BackendChangesError`, `BackendSetError`, `ChangesResult`,
`QueryResult`, `QueryChangesResult` are the exact pattern to follow for
`TasksBackend`. The read-side operations (`get_objects`, `get_state`,
`get_changes`, `query_objects`, `query_changes`) should be inherited from
`JmapBackend` via supertrait, exactly as `MailBackend` does. Only write
operations and tasks-specific operations belong directly on `TasksBackend`.

### Handler logic reference — read, do not copy

There is no existing production JMAP Tasks server in Rust to reference.
The JMAP Calendars implementation in `~/GIT/stalwart-jmap-server/` covers
a structurally very similar object hierarchy (Calendar, CalendarEvent,
CalendarEventNotification) and is the best available analog.

**License: AGPL-3.0-only.** Do not copy or translate code. Read to understand
the logic; implement independently from the spec.

The closest structural analogs in Stalwart:
- `main/crates/jmap/src/calendar/` — Calendar object handlers (≈ TaskList)
- `main/crates/jmap/src/email_event_notification/` — notification handling (≈ TaskNotification)

## Capability URI

Core capability: `urn:ietf:params:jmap:tasks` (draft-tasks-06 §7.1)

Optional extension capabilities:
- `urn:ietf:params:jmap:tasks:recurrences` — recurrence rules on Task (§7.2)
- `urn:ietf:params:jmap:tasks:assignees` — Participant/assignee on Task (§7.3)
- `urn:ietf:params:jmap:tasks:alerts` — Alert objects on Task and TaskList (§7.4)
- `urn:ietf:params:jmap:tasks:multilingual` — localizations on Task (§7.5)
- `urn:ietf:params:jmap:tasks:customtimezones` — custom TimeZone objects (§7.6)

## RFC Method Coverage

| Object | Methods | Draft § | Notes |
|---|---|---|---|
| TaskList | get | §3.2 | standard /get; ids=null fetches all |
| TaskList | changes | §3.3 | standard /changes |
| TaskList | set | §3.4 | standard /set + onDestroyRemoveTasks |
| Task | get | §4.9 | standard /get; utcStart/utcDue computed on fetch |
| Task | changes | §4.10 | standard /changes |
| Task | set | §4.11 | standard /set; isDraft restrictions |
| Task | copy | §4.12 | standard /copy (RFC 8620 §5.4) |
| Task | query | §4.13 | standard /query |
| Task | queryChanges | §4.14 | standard /queryChanges |
| TaskNotification | get | §5.2 | standard /get |
| TaskNotification | changes | §5.3 | standard /changes |
| TaskNotification | set | §5.4 | destroy only; create/update → forbidden |
| TaskNotification | query | §5.5 | with FilterCondition and sorting |
| TaskNotification | queryChanges | §5.6 | standard /queryChanges |

Total: 14 method registrations.

## TasksBackend Trait

Follows the exact AFIT pattern of `MailBackend` (see `crate-jmap-mail-server/src/backend.rs`).
Not object-safe (generic methods). Always monomorphized at compile time.

```rust
/// Storage backend for JMAP Tasks method handlers.
///
/// Read-side operations (get_objects, get_state, get_changes, query_objects,
/// query_changes) are inherited from JmapBackend. Only write operations and
/// tasks-specific operations belong here.
///
/// Implementor invariants (same as MailBackend / StorageBackend):
/// 1. State monotonicity: get_state returns a different token after every
///    successful mutation. Token does not change on failure.
/// 2. Initial state: "0" is always the valid initial state sentinel.
/// 3. Partial set success: per-object failures do not roll back other objects
///    in the same /set call (RFC 8620 §5.3).
/// 4. TaskNotification generation: the backend generates notifications when
///    a Task is mutated by a principal other than the current user. The
///    handler does not generate notifications; it is the backend's
///    responsibility.
#[allow(async_fn_in_trait)]
pub trait TasksBackend: JmapBackend {
    // ── Write operations ────────────────────────────────────────────────────

    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    // ── Tasks-specific ──────────────────────────────────────────────────────
    //
    // Task/copy (RFC 8620 §5.4) is implemented in the handler using the
    // generic `get_objects::<Task>` (fetch source) + `create_object::<Task>`
    // (create in destination) pattern from the canonical extension-server
    // template. There is intentionally no `copy_task` trait method —
    // backends provide source-read and destination-write via the standard
    // JmapBackend methods, and the handler does the merge.

    /// Whether this backend supports operations on the given object type.
    ///
    /// Return false for types gated on extensions the backend does not
    /// implement (e.g. return false for a recurrence-related type if the
    /// backend does not support the recurrences extension).
    fn supports_type<O: JmapObject>(&self) -> bool;
}
```

## Key Design Decisions

### 1. TasksBackend follows MailBackend exactly for generic CRUD

Same AFIT pattern, same `BackendChangesError` / `BackendSetError` error types,
same `ChangesResult` / `QueryResult` structs. Implementors who have built a
`MailBackend` will find the contract familiar. The supertrait `JmapBackend`
provides all read-side methods.

### 2. TasksBackend is independent from any future CalendarsBackend

The Tasks spec reuses JSCalendar Task types but keeps TaskList, Task, and
TaskNotification separate from Calendar, CalendarEvent, and
CalendarEventNotification. The two domains must not share a backend trait:
a server can implement tasks without calendars and vice versa. The type crates
are also separate (`jmap-tasks-types` vs. a future `jmap-calendars-types`).

If a server implements both, it will have two independent backend traits on the
same struct — one for `MailBackend` and one for `TasksBackend`. This is the
intended pattern.

### 3. TaskList/set: onDestroyRemoveTasks handled in the handler layer

Draft-tasks-06 §3.4: when `onDestroyRemoveTasks` is true, any tasks in the
TaskList must be destroyed before the TaskList is destroyed. When false (the
default), a destroy attempt on a non-empty TaskList must fail with a
`taskListHasTask` SetError.

The handler calls `query_objects<Task>` (filter by taskListId) to check for
tasks. If `onDestroyRemoveTasks` is true it calls `destroy_object<Task>` for
each, then `destroy_object<TaskList>`. The backend has no concept of this flag.

### 4. TaskNotification/set: destroy only — handler enforces

Draft-tasks-06 §5.4: "Only destroy is supported; any attempt to create/update
MUST be rejected with a forbidden SetError."

The handler rejects any create or update attempt before touching the backend.
Destroy is routed to `destroy_object<TaskNotification>` normally.

### 5. TaskNotification generation is the backend's responsibility

TaskNotifications are created when another principal modifies a Task in a shared
TaskList. The handler does not generate notifications — it has no knowledge of
who else is subscribed or what changed. The backend generates them atomically
with the mutation that caused them.

This mirrors the CalendarEventNotification pattern described in
draft-ietf-jmap-calendars.

### 6. utcStart and utcDue are computed on fetch, not stored

Draft-tasks-06 §4 specifies that `utcStart` and `utcDue` are "calculated at
fetch time by the server" and "not included by default." They must be requested
explicitly via the `properties` argument to `Task/get`.

The handler computes these by calling a helper that converts `start`/`due` +
`timeZone` to UTC. The backend stores `start`, `due`, and `timeZone`; it never
stores `utcStart` or `utcDue`. The helper is in `src/helpers.rs`.

### 7. Task/set: isDraft restrictions enforced in the handler

Draft-tasks-06 §4: "This may only be set to true upon creation. Once set to
false, the value cannot be updated to true." The handler validates this rule
when processing update patches. The backend stores whatever the handler passes
through.

### 8. Per-user properties in shared TaskLists

Draft-tasks-06 §4.5.1: `keywords`, `color`, `freeBusyStatus`,
`useDefaultAlerts`, `alerts` are per-user in shared task lists. The backend
must store them per-user. The handler does not need to know the storage layout
— it calls `get_objects<Task>` / `update_object<Task>` and the backend returns
the correct per-user view.

### 9. register_tasks_handlers is the entry point

One function registers all 14 method handlers with the caller's
`jmap-server::Dispatcher<C>`. The backend is wrapped in `Arc<B>` and cloned
into each handler closure — same pattern as `register_mail_handlers`.

```rust
pub fn register_tasks_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: TasksBackend + 'static,
    C: Clone + Send + 'static;
```

## Module Layout

```
src/
  lib.rs              re-exports; register_tasks_handlers
  backend.rs          TasksBackend trait; re-exports from jmap_server::{
                        JmapBackend, BackendChangesError, BackendSetError,
                        ChangesResult, QueryResult, QueryChangesResult, AddedItem }
  task_list.rs        TaskList/get, /changes, /set
                      (includes onDestroyRemoveTasks logic)
  task.rs             Task/get, /changes, /set, /copy, /query, /queryChanges
                      (includes isDraft validation, utcStart/utcDue computation)
  notification.rs     TaskNotification/get, /changes, /set (destroy only),
                      /query, /queryChanges
  helpers.rs          LocalDateTime → UTCDate conversion; shared query utilities
```

## Test Strategy

A `MemoryBackend` in `tests/common/mod.rs` provides an in-memory `HashMap`
implementation of `TasksBackend`. This serves as both the test harness and the
canonical example for implementors.

```
tests/
  common/
    mod.rs              MemoryBackend implementation
  task_list_tests.rs
  task_tests.rs
  notification_tests.rs
```

Test oracles come from hand-written JSON fixtures constructed from the field
descriptions in draft-tasks-06 and RFC 8984 examples. The spec does not include
full request/response example pairs (the draft is incomplete), so fixtures must
be constructed by hand from the field-level descriptions.

Each test calls `register_tasks_handlers` with the `MemoryBackend`, constructs a
`JmapRequest`, calls `Dispatcher::dispatch`, and asserts the response.

### Non-trivial test cases to include

- TaskList/set: `onDestroyRemoveTasks: true` destroys tasks before TaskList
- TaskList/set: destroy with tasks and `onDestroyRemoveTasks: false` → `taskListHasTask`
- TaskList/set: `shareWith` update rejected for user without `mayAdmin`
- Task/set: create with `isDraft: true`; subsequent update to `isDraft: true` rejected
- Task/set: create with `isDraft: false`; update to `isDraft: true` rejected
- Task/get: `utcStart` / `utcDue` not returned unless explicitly in `properties`
- Task/get: `utcStart` / `utcDue` computed correctly from `start` + `timeZone`
- Task/copy: task appears in destination account with new uid and id
- TaskNotification/set: create attempt → `forbidden`; update attempt → `forbidden`
- TaskNotification/set: destroy removes the notification
- TaskNotification/query: filter by `after`, `before`, `type`, `taskIds`
- queryChanges: `cannotCalculateChanges` when backend returns TooManyChanges

## Dependencies

```toml
jmap-types       = { path = "../crate-jmap-types" }
jmap-tasks-types = { path = "../crate-jmap-tasks-types" }
jmap-server      = { path = "../crate-jmap-server" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror  = "2"
tokio      = { version = "1", features = ["rt"] }
# No MIME libraries, no HTTP client, no database drivers
```
