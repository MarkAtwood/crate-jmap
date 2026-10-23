# jmap-tasks-server

## What it is

JMAP Tasks ([draft-ietf-jmap-tasks-06]) method handlers and the `TasksBackend`
trait. Plugs into [`jmap-server`]'s `Dispatcher`. Implements all 14 method names
from the Tasks extension. Storage-agnostic — consumers implement the
`TasksBackend` trait for their own data layer.

## What it's for

Implements draft-ietf-jmap-tasks-06 — TaskList, Task, and TaskNotification
objects with `Task/copy`, per-user property routing, and `isDraft`
immutability — so consumers can wire JMAP Tasks method dispatch into their
HTTP transport. Sibling to `jmap-mail-server` (the canonical extension-server
template) and the other `jmap-*-server` crates; all of them inter-depend
through the workspace `JmapHandler` trait in `jmap-server`. The consumer
supplies a `TasksBackend` impl (storage), a `CallerCtx` type (auth identity),
and wires the dispatcher into HTTP / SSE / WebSocket transport themselves.

## How to use

```rust,ignore
use std::sync::Arc;
use jmap_tasks_server::{TasksBackend, register_tasks_handlers};
use jmap_server::Dispatcher;

// 1. Implement TasksBackend for your storage layer. The impl must also
//    satisfy the JmapBackend supertrait (account_exists, get_objects,
//    get_state, get_changes, query_objects, query_changes) and declare
//    `type Error` and `type CallerCtx`. See the TasksBackend trait
//    section below for the full surface.

// 2. Construct a dispatcher parameterised on your CallerCtx type, then
//    register all 14 Tasks methods in one call.
let mut dispatcher: Dispatcher<MyCallerCtx> = Dispatcher::new();
register_tasks_handlers(&mut dispatcher, Arc::new(my_backend));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, caller_ctx, session_state).await;
```

`Dispatcher<C>` requires `B::CallerCtx = C`; `register_tasks_handlers`
enforces the match at the type level. For a single-user dev server use
`Dispatcher<()>` with a backend whose `CallerCtx = ()`. For
multi-tenant production, define a typed caller context (e.g. a
principal-id newtype) and parameterise both sides on that type.

For a runnable, type-checked variant of this example, see the
[crate-level documentation](https://docs.rs/jmap-tasks-server) — the
doctest in `lib.rs` is verified by `cargo test --doc`.

After `register_tasks_handlers` returns, the dispatcher handles every method name listed in
the [Registered methods](#registered-methods) section below. The same `Arc<MyBackend>` can
be shared with other parts of your application.

## Registered methods

All 14 method names from draft-ietf-jmap-tasks-06 are registered:

| Object | Methods |
|---|---|
| `TaskList` | `get`, `changes`, `set` |
| `Task` | `get`, `changes`, `set`, `copy`, `query`, `queryChanges` |
| `TaskNotification` | `get`, `changes`, `set`, `query`, `queryChanges` |

## TasksBackend trait

Implement this trait to connect the handlers to your storage system. The read-side methods
(`get_objects`, `get_state`, `get_changes`, `query_objects`, `query_changes`) are defined on
the `JmapBackend` supertrait (from `jmap-server`). `TasksBackend` adds write operations and
Tasks-specific operations.

```rust,ignore
pub trait TasksBackend: JmapBackend {
    // --- Write operations ---

    /// Create a new object (TaskList or Task).
    /// `build` receives the backend-assigned id and returns the object
    /// to store. Returns (assigned_id, created_object). The implementor MUST
    /// authorise against `caller` before the mutation; see the rustdoc
    /// on the trait itself for the workspace-canonical contract.
    fn create_object<O, F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send;

    /// Apply a partial update (patch) to an existing object.
    /// Returns Some(updated_object) if the backend modified server-set fields beyond
    /// the patch (RFC 8620 §5.3 echo); None if the patch was applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self, caller: &Self::CallerCtx, account_id: &Id, id: &Id, patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self, caller: &Self::CallerCtx, account_id: &Id, id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns `true` if this backend supports JMAP object type `O` for
    /// `account_id`, as seen by `caller`.
    fn supports_type<O: JmapObject>(&self, caller: &Self::CallerCtx, account_id: &Id) -> bool;

    /// Returns true if the given task list contains at least one task.
    ///
    /// Called by TaskList/set destroy when onDestroyRemoveTasks is false.
    /// If this returns true, the destroy is rejected with taskListHasTask.
    fn task_list_has_tasks(
        &self, caller: &Self::CallerCtx, account_id: &Id, task_list_id: &Id,
    ) -> impl Future<Output = bool> + Send;

    /// Apply a patch containing only per-user Task properties (§4.5.1).
    ///
    /// When only per-user properties are patched, the shared `updated` timestamp
    /// MUST NOT change (§4.5.1). No default: delegating to update_object is
    /// correct only for a single-user backend whose update_object leaves
    /// `updated` alone. Backends serving multiple users store the patch in a
    /// user-scoped path.
    fn update_task_per_user(
        &self, caller: &Self::CallerCtx, account_id: &Id, id: &Id, patch: PatchObject,
    ) -> impl Future<Output = Result<Option<Task>, BackendSetError<Self::Error>>> + Send;

    // --- Optional overrides (have default implementations) ---

    /// Compute utcStart and utcDue for a Task by converting local-time fields
    /// and time zone into UTC (draft-tasks-06 §4, utcStart/utcDue paragraphs).
    ///
    /// Returns (utc_start, utc_due) as `UTCDate` values, or None for each
    /// if the field is absent or the time zone is unknown.
    ///
    /// The default implementation returns (None, None). Backends with full
    /// time-zone support should override this.
    fn compute_utc_times(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        task: &Task,
        tz_hint: Option<&str>,
    ) -> impl Future<Output = (Option<UTCDate>, Option<UTCDate>)> + Send {
        async { (None, None) }
    }
}
```

Every mutating method takes `caller: &Self::CallerCtx`. The workspace
"Caller identity (foundation seam)" contract — `JmapBackend::principal_id`
identifies the principal; the backend MUST authorise against it before
mutating — is documented per-method on the trait itself (`backend.rs`).

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, `taskListHasTask`, etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a `serverFail` response

## Permission enforcement responsibilities

The handlers in this crate do **no** permission checking. Per the
workspace AGENTS.md "Caller identity (foundation seam)" rule, backends
are canonical for authorization: the handler computes the candidate
mutation and the backend's `/set` impl is the canonical point of
enforcement. Caller identity is read via
`JmapBackend::principal_id` on the `caller: &Self::CallerCtx`
parameter every backend method receives.

The reference `memory::MemoryBackend` does **not** implement any of
the checks below — it returns `None` from `principal_id` and stores
whatever the handler hands it. Production backends MUST implement each
of the following or they will silently accept JMAP requests that
draft-ietf-jmap-tasks-06 mandates the server to reject. ACL denials
are `forbidden` SetErrors. The rights are the `TaskRights` of the
TaskList (§3).

### TaskList/set (draft-ietf-jmap-tasks-06 §1.6.1, §3, §3.4)

1. **Create requires the account's `mayCreateTaskList`** (§1.6.1).

2. **`shareWith` write requires `mayAdmin`**, and "the user cannot
   give a right to a principal if the principal did not already have
   that right and the user making the change also does not have that
   right" (§3.4).

3. **Destroy requires `mayDelete`**, which "MUST be false if the
   account to which this task list belongs has the _isReadOnly_
   property set to true" (§3).

4. **`isSubscribed` policy hook (optional).** "The server MAY forbid
   users from subscribing to certain task lists even though they have
   permission to see them" (§3.4).

5. **Per-user TaskList properties.** `name`, `color`, `sortOrder`,
   `timeZone`, `defaultAlertsWithoutTime` and `defaultAlertsWithTime`
   "may be set by anyone who is subscribed to the task list, and are
   all stored per-user" (§3.4), keyed on `principal_id`.

### Task/set and Task/copy (§3, §4.5.1)

6. **Create, update and destroy require `mayWriteAll`, or
   `mayWriteOwn` when the caller is an owner of the task or it has no
   owner** (§3). Moving a task between lists needs the right on both.
   `Task/copy` writes through `create_object` in the destination
   account and treats a source the caller cannot read as `notFound`.

7. **Per-user Task properties.** `keywords`, `color`,
   `freeBusyStatus`, `useDefaultAlerts` and `alerts` "MUST be stored
   per-user" (§4.5.1); `mayUpdatePrivate` lets the user modify them
   "even if they would not otherwise have permission to modify that
   task" (§3). The handler routes such patches to
   `update_task_per_user`.

8. **`mayRSVP` participant updates.** Without write access the caller
   may change only `participationStatus`, `participationComment` and
   `expectReply` on its own Participant, and may add participants only
   as `mayInviteSelf` / `mayInviteOthers` allow (§3).

### Reads (§3)

9. **Visibility.** "The user may fetch the task if they have the
   mayReadItems right on any task list the task is in" (§3).
   `get_objects` and `query_objects` leave out the others.

TaskNotification create and update are rejected with `forbidden` by
the handler itself (§5.4); the backend sees only destroys.

### Caller-identity-less backends

The reference `MemoryBackend` returns `None` from `principal_id` and
is intentionally permissive (fail-open, bd:JMAP-bghk) — it is suitable
for single-user dev servers, fixtures, and the workspace's own
integration tests, but NOT for any deployment where more than one
principal can reach the JMAP dispatcher. A deployment that needs ACL
enforcement MUST override `JmapBackend::principal_id` and implement
each of the checks above; the workspace ships no production-grade
reference impl.

## How it works

### Registration

`register_tasks_handlers` uses `ClosureHandler` (provided by
`jmap-server`) to wrap each handler function and `Arc<B>` into a
`JmapHandler<C>` and registers it with the dispatcher. The dispatcher's
`CallerCtx` value is forwarded into each closure as `_ctx`; the standard
`handle_*` handler bodies receive `(Arc<B>, call_id, args)` only. One
`Arc::clone` per method name; no heap allocation per request.

### Task/set — isDraft immutability

Once a Task's `isDraft` is not `true`, it cannot be set to `true` (draft-tasks-06
§4: "This may only be set to true upon creation"). The backend enforces this in
`update_object`, atomically with the write, and rejects the patch with
`invalidProperties: ["isDraft"]`; the `Task/set` handler does not pre-check it (the
workspace-canonical posture — see AGENTS.md "Permission enforcement: backend
canonical"). Transitions from `isDraft: true` to `isDraft: false` (publishing a draft)
are always allowed. The reference `MemoryBackend` does this.

### Task/set — per-user property routing

`keywords`, `color`, `freeBusyStatus`, `useDefaultAlerts`, and `alerts` are per-user
properties (§4.5.1), listed in `jmap_tasks_types::PER_USER_TASK_PROPERTIES` and
classified by `jmap_tasks_types::is_per_user_task_property` (fixed by the spec, not a
backend hook). When every key in a patch is in this set, the handler routes
the update to `update_task_per_user` instead of `update_object`. This allows backends to
store per-user data separately without changing the shared `updated` timestamp.

### Task/get — utcStart and utcDue

`utcStart` and `utcDue` are computed on-demand via `compute_utc_times`. They are only
included in the response when the client explicitly requests them in the `properties` list.
The default implementation returns `(None, None)`, so both fields are absent unless the
backend overrides this method.

### TaskList/set — onDestroyRemoveTasks cascade

When `onDestroyRemoveTasks: false` (the default) and the task list being destroyed contains
tasks, `task_list_has_tasks` is called. If it returns `true`, the destroy is rejected with
`taskListHasTask`. When `onDestroyRemoveTasks: true`, the handler cascades the destroy to
all tasks in the list before destroying the list itself.

## CallerCtx

`register_tasks_handlers` registers each method as a `ClosureHandler` that
forwards the dispatcher's `CallerCtx` value into the closure as `_ctx`. The standard
`handle_*` handler bodies ignore `_ctx` and receive only `(Arc<B>, call_id, args)`;
the value is still available for backends that register handlers individually via
`ClosureHandler`.

If you need per-request context — auth identity, tenant id, rate-limit token —
inside one of the standard `handle_*` functions, implement `JmapHandler<C>` directly
and register with `dispatcher.register(method_name, Arc::new(your_handler))`.

## Capability URIs

Include these in your Session object's `capabilities` map:

```rust
// Re-exported from jmap-tasks-types:
pub const JMAP_TASKS_URI: &str = "urn:ietf:params:jmap:tasks";
// Sub-extension URIs (also from jmap-tasks-types):
// JMAP_TASKS_RECURRENCES_URI, JMAP_TASKS_ASSIGNEES_URI,
// JMAP_TASKS_ALERTS_URI, JMAP_TASKS_MULTILINGUAL_URI,
// JMAP_TASKS_CUSTOMTIMEZONES_URI
```

## Crate family

```
jmap-types
    ├── jmap-server          Dispatcher this plugs into
    └── jmap-tasks-types     domain types (TaskList, Task, TaskNotification, etc.)
            └── jmap-tasks-server  ← this crate
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will remain that way
until the family is published to crates.io.

## Cargo features

- `memory` — enables the reference in-memory `MemoryBackend` (gated behind
  `#[cfg(feature = "memory")]` in `src/memory.rs`). Off by default; enable
  only for tests and demonstrations. Not production-grade. Server-assigned
  ids are uuid v4, as in `jmap-mail-server`.

## Gotchas

- `compute_utc_times` default returns `(None, None)` — `utcStart`/`utcDue` will be
  absent from all `Task/get` responses unless the backend overrides this method.
- `isDraft` immutability is the backend's job: `update_object` MUST reject a patch that
  sets `isDraft: true` on a Task whose stored value is not `true`; the handler does not
  check it.
- **`memory::MemoryBackend` is a reference implementation, not production.**
  Gated behind `feature = "memory"` (off by default). State is held in
  `HashMap`s behind a `std::sync::Mutex`; persistence is not implemented; a
  number of draft-ietf-jmap-tasks-06 edge cases are simplified. Intended for integration
  tests, smoke tests, examples, and as source-readable study material for
  downstream contributors writing a real (database-backed) backend. See the
  module docs in `src/memory.rs` for the explicit non-production caveats.
  Server-assigned ids are uuid v4, as in `jmap-mail-server`.

## References

- **[draft-ietf-jmap-tasks-06]** — JMAP Tasks (normative for all method semantics)
- **[RFC 8984]** — JSCalendar (Task property definitions)
- **[RFC 8620]** — JMAP Core (request format, SetError, ResultReference, `/set` response shape)

[draft-ietf-jmap-tasks-06]: https://datatracker.ietf.org/doc/draft-ietf-jmap-tasks/
[RFC 8984]: https://www.rfc-editor.org/rfc/rfc8984
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-server`]: ../crate-jmap-server
