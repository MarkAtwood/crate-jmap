# jmap-tasks-client

## What it is

Typed client methods for the JMAP Tasks extension ([draft-ietf-jmap-tasks-06]). Wraps
[`jmap-base-client`] transport with strongly-typed request builders and response types for
all 14 JMAP Tasks method names.

## What it's for

Implements draft-ietf-jmap-tasks method bindings on top of `jmap-base-client`:
`TaskList/*`, `Task/*`, and `TaskNotification/*`. Sibling of
`jmap-mail-client` in the extension-client family — mirrors that crate's
shape. Depends on `jmap-base-client` for transport and session, and on
`jmap-tasks-types` for the wire types.

## How to use

```rust,no_run
use jmap_base_client::{BearerAuth, ClientConfig, JmapClient};
use jmap_tasks_client::JmapTasksExt;
use jmap_types::Id;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
// 1. Build a JmapClient (auth, base URL — see jmap-base-client docs).
let auth = BearerAuth::new("my-token")?;
let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;

// 2. Fetch the session object.
let session = client.fetch_session().await?;

// 3. Bind to a SessionClient for Tasks methods.
let sc = client.with_tasks_session(session);

// 4. Call Tasks methods.
let task_lists = sc.task_list_get(None, None).await?;
let tasks = sc
    .task_get(Some(&[Id::from("task1"), Id::from("task2")]), None)
    .await?;

// Create a task.
sc.task_set(
    Some(serde_json::json!({
        "new1": { "taskListId": "list1", "title": "Draft spec review" }
    })),
    None,
    None,
).await?;
# Ok(())
# }
```

Id parameters are typed `&jmap_types::Id` (or `&[jmap_types::Id]` for slices)
to make invalid Ids unrepresentable. State tokens use `&jmap_types::State`.
Construct Ids with `Id::new_validated(s)` to enforce RFC 8620 §1.2 syntax at
the boundary, or with `Id::from(s)` when the value is known-valid (e.g.
already came back from a server response).

## Methods

All `pub async fn` on `SessionClient`:

| Method | JMAP method | Returns |
|---|---|---|
| `task_list_get(ids, properties)` | `TaskList/get` | `GetResponse<TaskList>` |
| `task_list_changes(since_state, max_changes)` | `TaskList/changes` | `ChangesResponse` |
| `task_list_set(create, update, destroy, on_destroy_remove_tasks)` | `TaskList/set` | `SetResponse<TaskList>` |
| `task_get(ids, properties)` | `Task/get` | `GetResponse<Task>` |
| `task_changes(since_state, max_changes)` | `Task/changes` | `ChangesResponse` |
| `task_set(create, update, destroy)` | `Task/set` | `SetResponse<Task>` |
| `task_copy(from_account_id, create)` | `Task/copy` | `SetResponse<Task>` |
| `task_query(filter, sort, position, limit)` | `Task/query` | `QueryResponse` |
| `task_query_changes(since_query_state, max_changes)` | `Task/queryChanges` | `QueryChangesResponse` |
| `task_notification_get(ids, properties)` | `TaskNotification/get` | `GetResponse<TaskNotification>` |
| `task_notification_changes(since_state, max_changes)` | `TaskNotification/changes` | `ChangesResponse` |
| `task_notification_set(destroy)` | `TaskNotification/set` | `SetResponse` |
| `task_notification_query(filter, sort, position, limit)` | `TaskNotification/query` | `QueryResponse` |
| `task_notification_query_changes(since_query_state, max_changes)` | `TaskNotification/queryChanges` | `QueryChangesResponse` |

**Note:** `task_notification_set` is destroy-only. The server creates `TaskNotification`
objects automatically; clients may only remove them. Any create or update sent to the server
would be rejected with `forbidden`.

## How it works

Each method on `SessionClient` runs the same pipeline:

1. Validate arguments (typed `&Id` / `&[Id]` makes invalid Ids unrepresentable;
   defence-in-depth empty-state guards return `InvalidArgument` before any I/O).
2. Resolve `(api_url, account_id)` from the bound session for
   `urn:ietf:params:jmap:tasks`.
3. Build the method-arguments JSON.
4. Wrap it into a `JmapRequest` via `JmapRequestBuilder` with
   `using = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:tasks"]`.
5. POST it via `jmap_base_client::JmapClient::call`.
6. `extract_response::<T>` finds the typed result for call ID `"r1"`.

The `Jmap*Ext` extension trait (`JmapTasksExt`) adds the
`with_tasks_session(session)` accessor to `JmapClient`. The returned
`SessionClient` carries the session and exposes every Tasks method as a typed
`async fn`.

## Gotchas

- The draft expired in 2023; if the spec is revised and published as an RFC, method
  signatures may change.
- No integration tests against a real JMAP server; tests use request-shape oracles and
  serialization checks only.

## References

- **[draft-ietf-jmap-tasks-06]** — JMAP Tasks
- **[RFC 8620]** — JMAP Core

[draft-ietf-jmap-tasks-06]: https://datatracker.ietf.org/doc/draft-ietf-jmap-tasks/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-base-client`]: ../crate-jmap-base-client
