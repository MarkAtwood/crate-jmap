# jmap-tasks-client — Implementation Plan

JMAP Tasks method implementations on top of `jmap-base-client`
(draft-ietf-jmap-tasks-06).

## Crate Family Position

```
jmap-types
    ├── jmap-tasks-types
    │       └── (types used here)
    └── jmap-base-client
            └── jmap-tasks-client  ← this crate
```

## What This Crate Is

An extension layer over `jmap-base-client` that adds typed methods for every
JMAP Tasks operation: `TaskList/get`, `TaskList/set`, `TaskList/changes`,
`Task/get`, `Task/set`, `Task/changes`, `Task/query`, `Task/queryChanges`,
`Task/copy`, `TaskNotification/get`, `TaskNotification/changes`,
`TaskNotification/set` (destroy), `TaskNotification/query`,
`TaskNotification/queryChanges`.

Consumers call `jmap-base-client::JmapClient::call()` directly or use the
typed helpers defined here. No new HTTP machinery — all network operations go
through `jmap-base-client`.

## What This Crate Is Not

- Not a server-side crate
- Not a standalone HTTP client (no auth, no transport — that is `jmap-base-client`)
- Not a CalDAV client — VTODO / iCalendar round-tripping is outside scope

## Source Material

This is greenfield — no existing Rust JMAP Tasks client to extract from.

Design pattern to follow:
- `~/PROJECT/JMAP/crate-jmap-mail-client/src/` — how the extension trait is
  structured, how `JmapRequestBuilder` is used to issue calls, how request
  and response types are declared per module
- `~/PROJECT/crate-jmapchat-client/src/methods/` — how method inputs and
  outputs are structured in the reference jmapchat client
- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-tasks-06.txt` —
  normative spec; read the relevant section before writing each method

## Extension Trait Pattern

Cross-crate inherent impls are not valid Rust (orphan rule). To add methods to
`JmapClient` from this crate, use an extension trait:

```rust
pub trait JmapTasksExt {
    async fn task_list_get(...) -> Result<...>;
    // ...
}

impl JmapTasksExt for JmapClient { ... }
```

Callers must bring the trait into scope: `use jmap_tasks_client::JmapTasksExt;`

Rust 1.75 AFIT (async fn in trait, stable) is used — no `async-trait` crate
needed. This works because we do not need `dyn JmapTasksExt`.

## Planned Public API

```rust
use jmap_base_client::{ClientError, JmapClient};
use jmap_tasks_types::{Task, TaskList, TaskNotification};
use jmap_types::{Id, State};

/// Extension trait adding JMAP Tasks methods (draft-ietf-jmap-tasks-06)
/// to [`JmapClient`].
///
/// Import this trait to use: `use jmap_tasks_client::JmapTasksExt;`
pub trait JmapTasksExt {
    // ── TaskList ─────────────────────────────────────────────────────────────

    /// TaskList/get (draft-tasks-06 §3.2).
    /// Pass ids=None to fetch all task lists.
    async fn task_list_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<TaskList>, ClientError>;

    /// TaskList/changes (draft-tasks-06 §3.3).
    async fn task_list_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// TaskList/set (draft-tasks-06 §3.4).
    /// Includes the `onDestroyRemoveTasks` argument.
    async fn task_list_set(
        &self,
        account_id: &Id,
        req: TaskListSetRequest,
    ) -> Result<SetResponse<TaskList>, ClientError>;

    // ── Task ──────────────────────────────────────────────────────────────────

    /// Task/get (draft-tasks-06 §4.9).
    async fn task_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<Task>, ClientError>;

    /// Task/changes (draft-tasks-06 §4.10).
    async fn task_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// Task/set (draft-tasks-06 §4.11).
    async fn task_set(
        &self,
        account_id: &Id,
        req: SetRequest<Task>,
    ) -> Result<SetResponse<Task>, ClientError>;

    /// Task/copy (draft-tasks-06 §4.12, RFC 8620 §5.4).
    async fn task_copy(
        &self,
        from_account_id: &Id,
        to_account_id: &Id,
        req: TaskCopyRequest,
    ) -> Result<SetResponse<Task>, ClientError>;

    /// Task/query (draft-tasks-06 §4.13).
    async fn task_query(
        &self,
        account_id: &Id,
        req: TaskQueryRequest,
    ) -> Result<QueryResponse, ClientError>;

    /// Task/queryChanges (draft-tasks-06 §4.14).
    async fn task_query_changes(
        &self,
        account_id: &Id,
        req: TaskQueryChangesRequest,
    ) -> Result<QueryChangesResponse, ClientError>;

    // ── TaskNotification ──────────────────────────────────────────────────────

    /// TaskNotification/get (draft-tasks-06 §5.2).
    async fn task_notification_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<TaskNotification>, ClientError>;

    /// TaskNotification/changes (draft-tasks-06 §5.3).
    async fn task_notification_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// TaskNotification/set: destroy only (draft-tasks-06 §5.4).
    /// Only the `destroy` field of the SetRequest is sent; create and update
    /// are rejected by the server with a `forbidden` SetError.
    async fn task_notification_destroy(
        &self,
        account_id: &Id,
        ids: &[Id],
    ) -> Result<SetResponse<TaskNotification>, ClientError>;

    /// TaskNotification/query (draft-tasks-06 §5.5).
    async fn task_notification_query(
        &self,
        account_id: &Id,
        req: TaskNotificationQueryRequest,
    ) -> Result<QueryResponse, ClientError>;

    /// TaskNotification/queryChanges (draft-tasks-06 §5.6).
    async fn task_notification_query_changes(
        &self,
        account_id: &Id,
        req: TaskNotificationQueryChangesRequest,
    ) -> Result<QueryChangesResponse, ClientError>;
}

impl JmapTasksExt for JmapClient {
    // implementations in task_list.rs, task.rs, notification.rs
}
```

### Request types used above

**`TaskListSetRequest`** — wraps `SetRequest<TaskList>` and adds:
- `on_destroy_remove_tasks: bool` (wire: `onDestroyRemoveTasks`, default false)

**`TaskCopyRequest`** — RFC 8620 §5.4 copy arguments for Task:
- `if_in_state: Option<State>`
- `create: HashMap<String, TaskCopyCreate>` (source task id + target task list id)
- `on_success_destroy_original: bool`
- `destroy_from_if_in_state: Option<State>`

**`TaskQueryRequest`** — RFC 8620 §5.5 query arguments for Task:
- `filter: Option<Filter<TaskFilterCondition>>`
- `sort: Option<Vec<TaskComparator>>`
- `position: i64`
- `anchor: Option<Id>`
- `anchor_offset: i64`
- `limit: Option<u64>`
- `calculate_total: bool`

**`TaskQueryChangesRequest`** — RFC 8620 §5.6 arguments:
- `filter: Option<Filter<TaskFilterCondition>>`
- `sort: Option<Vec<TaskComparator>>`
- `since_query_state: State`
- `max_changes: Option<u64>`
- `up_to_id: Option<Id>`
- `calculate_total: bool`

**`TaskNotificationQueryRequest`** — RFC 8620 §5.5 with TaskNotification filter:
- `filter: Option<Filter<TaskNotificationFilterCondition>>`
- `sort: Option<Vec<TaskNotificationComparator>>`
- `position: i64`
- `anchor: Option<Id>`
- `anchor_offset: i64`
- `limit: Option<u64>`
- `calculate_total: bool`

**`TaskNotificationQueryChangesRequest`** — analogous to TaskQueryChangesRequest
but for TaskNotification.

## Module Layout

```
src/
  lib.rs            pub trait JmapTasksExt; impl JmapTasksExt for JmapClient; re-exports
  task_list.rs      TaskList/get, /changes, /set; TaskListSetRequest
  task.rs           Task/get, /changes, /set, /copy, /query, /queryChanges;
                    TaskCopyRequest, TaskQueryRequest, TaskQueryChangesRequest
  notification.rs   TaskNotification/get, /changes, /set (destroy), /query,
                    /queryChanges; TaskNotificationQueryRequest,
                    TaskNotificationQueryChangesRequest
```

## Extras-preservation policy (JMAP-lbdy)

This crate has **no in-scope structs** of its own under the workspace
extras-preservation policy (see workspace `AGENTS.md`). The crate
defines only `SessionClient` (internal Rust state, not wire-format).

Wire-format types reach callers through re-exports rather than locally
defined types:

- Standard response wrappers (`GetResponse<T>`, `SetResponse<T>`,
  `ChangesResponse`, `QueryResponse`, `QueryChangesResponse`) are
  re-exported from `jmap-types` and carry their own `extra` field per
  JMAP-lbdy.1.
- The data object types this crate operates on (`Task`, `TaskList`,
  task-related types) are defined in `jmap-tasks-types` and carry their
  `extra` field per JMAP-lbdy.6.

### New-type rule

If a future method requires a locally-defined method-argument or
method-response struct, that new struct MUST include the `extra` field
from day one with the documented serde attributes:

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

and at least one round-trip preservation test. Per the canonical-template
propagation rule (workspace AGENTS.md), the new struct should mirror the
shape used in the canonical `jmap-mail-client` extension-client template.

## Test Strategy

- All tests use `wiremock` via `jmap-base-client`'s HTTP layer — no live network
- Request serialization tests: construct a typed request, verify JSON wire
  format matches the field descriptions in draft-tasks-06
- Response deserialization tests: feed hand-constructed JSON, verify typed structs
- The spec does not include full example exchanges (the draft is incomplete),
  so test fixtures are constructed by hand from the field-level descriptions

### Key test cases

- `task_list_get(None)` serializes to `{"ids": null}`
- `task_list_set` with `onDestroyRemoveTasks: true` includes the field on the wire
- `task_get` with `properties: Some(&["id", "title", "due"])` serializes only those
- `task_set` create with `isDraft: true` round-trips correctly
- `task_notification_destroy` sends only the `destroy` array, no `create` or `update`
- `task_query` filter with `after` / `before` / `taskIds` round-trips correctly
- Response deserialization: `notFound` ids parsed from GetResponse
- Response deserialization: `notCreated` / `notUpdated` / `notDestroyed` in SetResponse

## Dependencies

```toml
jmap-types        = { path = "../crate-jmap-types" }
jmap-tasks-types  = { path = "../crate-jmap-tasks-types" }
jmap-base-client  = { path = "../crate-jmap-base-client" }
serde_json        = "1"
thiserror         = "2"
# No direct reqwest/tokio dep — all I/O through jmap-base-client
```
