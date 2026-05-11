# jmap-tasks-server

JMAP Tasks ([draft-ietf-jmap-tasks-06]) method handlers for Rust. Plugs into
[`jmap-server`]'s `Dispatcher`. Implements all 14 method names from the Tasks extension.
Storage-agnostic — consumers implement the `TasksBackend` trait for their own data layer.

## Usage

```rust
use std::sync::Arc;
use jmap_tasks_server::{TasksBackend, register_tasks_handlers};
use jmap_server::Dispatcher;

// 1. Implement TasksBackend for your storage layer (see trait section below).
struct MyBackend { /* db pool, etc. */ }
impl TasksBackend for MyBackend { /* ... */ }

// 2. Wire all 14 Tasks methods into a Dispatcher in one call.
let mut dispatcher: Dispatcher<()> = Dispatcher::new();
register_tasks_handlers(&mut dispatcher, Arc::new(MyBackend { /* ... */ }));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, (), session_state).await;
```

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

```rust
pub trait TasksBackend: JmapBackend {
    // --- Write operations ---

    /// Create a new object (TaskList or Task).
    /// Returns (assigned_id, created_object).
    fn create_object<O: SetObject + Send + Sync>(
        &self, account_id: &Id, create_id: &str, obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    /// Apply a partial update (patch) to an existing object.
    /// Returns Some(updated_object) if the backend modified server-set fields beyond
    /// the patch (RFC 8620 §5.3 echo); None if the patch was applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self, account_id: &Id, id: &Id, patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self, account_id: &Id, id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Copy a Task from another account into the given account (Task/copy).
    fn copy_task(
        &self,
        from_account_id: &Id,
        to_account_id: &Id,
        task: Task,
    ) -> impl Future<Output = Result<(Id, Task), BackendSetError<Self::Error>>> + Send;

    /// Returns true if the given task list contains at least one task.
    ///
    /// Called by TaskList/set destroy when onDestroyRemoveTasks is false.
    /// If this returns true, the destroy is rejected with taskListHasTasks.
    fn task_list_has_tasks(
        &self, account_id: &Id, task_list_id: &Id,
    ) -> impl Future<Output = bool> + Send;

    // --- Optional overrides (have default implementations) ---

    /// Returns true if `prop` is a per-user Task property (draft-tasks-06 §4.5.1).
    ///
    /// Per-user properties: "keywords", "color", "freeBusyStatus",
    /// "useDefaultAlerts", "alerts". The default implementation matches exactly
    /// these five names.
    fn is_per_user_property(prop: &str) -> bool { /* ... */ }

    /// Apply a patch containing only per-user Task properties (§4.5.1).
    ///
    /// When only per-user properties are patched, the shared `updated` timestamp
    /// MUST NOT change (§4.5.1). The default implementation delegates to
    /// update_object, which is correct for single-user scenarios. Backends
    /// serving multiple users SHOULD override this to route to a user-scoped
    /// patch path.
    fn update_task_per_user(
        &self, account_id: &Id, id: &Id, patch: serde_json::Value,
    ) -> impl Future<Output = Result<Option<Task>, BackendSetError<Self::Error>>> + Send {
        self.update_object::<Task>(account_id, id, patch)
    }

    /// Compute utcStart and utcDue for a Task by converting local-time fields
    /// and time zone into UTC (draft-tasks-06 §4, lines 739-772).
    ///
    /// Returns (utc_start, utc_due) as RFC 3339 strings, or None for each
    /// if the field is absent or the time zone is unknown.
    ///
    /// The default implementation returns (None, None). Backends with full
    /// time-zone support should override this.
    fn compute_task_utc_times(
        &self, task: &Task, tz_hint: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        (None, None)
    }
}
```

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, `taskListHasTasks`, etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a `serverFail` response

## How it works

### Registration

`register_tasks_handlers` uses `ClosureHandler` (provided by
`jmap-server`) to wrap each handler function and `Arc<B>` into a
`JmapHandler<C>` and registers it with the dispatcher. The dispatcher's
`CallerCtx` value is forwarded into each closure as `_ctx`; the standard
`handle_*` handler bodies receive `(Arc<B>, call_id, args)` only. One
`Arc::clone` per method name; no heap allocation per request.

### Task/set — isDraft immutability

Once a Task's `isDraft` is set to `false`, it cannot be reverted to `true`. The
`Task/set` handler enforces this: when the patch contains `isDraft: true`, it first
fetches the current task via `get_objects` and rejects the patch with
`invalidProperties: ["isDraft"]` if the current value is already `false`. This costs
one extra `get_objects` call per updated task that includes `isDraft: true` in the patch.
Transitions from `isDraft: true` to `isDraft: false` (publishing a draft) are always allowed.

### Task/set — per-user property routing

`keywords`, `color`, `freeBusyStatus`, `useDefaultAlerts`, and `alerts` are per-user
properties (§4.5.1). When every non-null key in a patch is in this set, the handler routes
the update to `update_task_per_user` instead of `update_object`. This allows backends to
store per-user data separately without changing the shared `updated` timestamp.

### Task/get — utcStart and utcDue

`utcStart` and `utcDue` are computed on-demand via `compute_task_utc_times`. They are only
included in the response when the client explicitly requests them in the `properties` list.
The default implementation returns `(None, None)`, so both fields are absent unless the
backend overrides this method.

### TaskList/set — onDestroyRemoveTasks cascade

When `onDestroyRemoveTasks: false` (the default) and the task list being destroyed contains
tasks, `task_list_has_tasks` is called. If it returns `true`, the destroy is rejected with
`taskListHasTasks`. When `onDestroyRemoveTasks: true`, the handler cascades the destroy to
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

## Known Limitations

- `compute_task_utc_times` default returns `(None, None)` — `utcStart`/`utcDue` will be
  absent from all `Task/get` responses unless the backend overrides this method.
- `isDraft` immutability check requires one extra `get_objects` call per updated task where
  the patch includes `isDraft: true`. Backends that enforce this invariant atomically in
  `update_object` should return an `invalidProperties` SetError there instead.
- No storage backend ships with this crate.

## References

- **[draft-ietf-jmap-tasks-06]** — JMAP Tasks (normative for all method semantics)
- **[RFC 8984]** — JSCalendar (Task property definitions)
- **[RFC 8620]** — JMAP Core (request format, SetError, ResultReference, `/set` response shape)

[draft-ietf-jmap-tasks-06]: https://www.ietf.org/archive/id/draft-ietf-jmap-tasks-06.txt
[RFC 8984]: https://www.rfc-editor.org/rfc/rfc8984
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-server`]: ../crate-jmap-server

## License

MIT OR Apache-2.0
