# jmap-tasks-client — Implementation Plan

JMAP Tasks method implementations on top of `jmap-base-client`.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-tasks-06.txt`

## Crate Family Position

```
jmap-types
    └── jmap-base-client
            └── jmap-tasks-client  ← this crate
```

## What This Crate Is

Extension trait `JmapTasksExt` over `jmap_base_client::JmapClient` that adds typed
methods for every JMAP Tasks operation.

## Planned Public API

```rust
pub trait JmapTasksExt {
    async fn task_list_get(&self, account_id: &Id, ids: Option<&[Id]>)
        -> Result<GetResponse<TaskList>, ClientError>;
    async fn task_get(&self, account_id: &Id, ids: Option<&[Id]>, props: &[&str])
        -> Result<GetResponse<Task>, ClientError>;
    async fn task_set(&self, account_id: &Id, req: SetRequest<Task>)
        -> Result<SetResponse<Task>, ClientError>;
    async fn task_query(&self, account_id: &Id, req: TaskQueryRequest)
        -> Result<QueryResponse, ClientError>;
    // ... all TaskList, Task, TaskNotification methods
}
```

## Pattern to Follow

`~/PROJECT/JMAP/crate-jmap-mail-client/` — identical extension trait pattern.
`~/PROJECT/JMAP/crate-jmap-calendars-client/` — very close structural analog.
