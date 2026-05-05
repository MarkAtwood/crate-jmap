# jmap-tasks-client

Typed client methods for the JMAP Tasks extension ([draft-ietf-jmap-tasks-06]). Wraps
[`jmap-base-client`] transport with strongly-typed request builders and response types for
all 14 JMAP Tasks method names.

## Usage

```rust
use jmap_tasks_client::JmapTasksExt;

// 1. Build a JmapClient (auth, base URL — see jmap-base-client docs).
let client = jmap_base_client::JmapClient::new(/* ... */);

// 2. Fetch the session object.
let session = client.fetch_session().await?;

// 3. Bind to a SessionClient for Tasks methods.
let sc = client.with_tasks_session(session);

// 4. Call Tasks methods.
let task_lists = sc.task_list_get(None, None).await?;
let tasks = sc.task_get(Some(&["task1", "task2"]), None).await?;

// Create a task.
sc.task_set(
    Some(serde_json::json!({
        "new1": { "taskListId": "list1", "title": "Draft spec review" }
    })),
    None,
    None,
).await?;
```

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

## Known Limitations

- The draft expired in 2023; if the spec is revised and published as an RFC, method
  signatures may change.
- No integration tests against a real JMAP server; tests use request-shape oracles and
  serialization checks only.

## References

- **[draft-ietf-jmap-tasks-06]** — JMAP Tasks
- **[RFC 8620]** — JMAP Core

[draft-ietf-jmap-tasks-06]: https://www.ietf.org/archive/id/draft-ietf-jmap-tasks-06.txt
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-base-client`]: ../crate-jmap-base-client

## License

MIT OR Apache-2.0
