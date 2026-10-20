# Agent Instructions — jmap-chat-tasks-server

## What this crate is

The Chat binding for `jmap-tasks-server` (Mark, 2026-10-20,
bd:JMAP-v9py.16.8): cross-layer rules of draft-atwood-jmap-chat-tasks-00
as free functions a deployment calls from its `TasksBackend` and
`ChatBackend` impls. No handlers, no backend trait, no memory backend.

## Before starting any work

1. Run `bd ready`.
2. Read the cited sections of
   `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-tasks-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-chat-tasks-server --all-targets --all-features -- -D warnings
cargo test -p jmap-chat-tasks-server
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p jmap-chat-tasks-server
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Shape | Free functions taking the backends they read |
| Base crates | `jmap-tasks-server` and `jmap-chat-server` stay independent of each other |
| `mayBindTaskList: false` | `taskListId` is not registered as a Space extension property, so no runtime gate |
| `manage_space` gate | `apply_space_metadata_patch`'s, not this crate's |
| Space destroy | Never destroys the bound TaskList (draft §Binding and unbinding SHOULD NOT) |
| TaskList destroy | No Tasks-side hook: the deployment calls `clear_space_task_list_id` from `TasksBackend::destroy_object::<TaskList>` |
| Chat destroy | `clear_task_chat_ids` from `ChatBackend::on_chat_destroyed` |
| Task destroy | Never destroys the referenced Chat (draft §Lifecycle) |
| Tests | Each assertion quotes its draft line; the two reference `MemoryBackend`s |
