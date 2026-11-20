# jmap-tasks

JMAP Tasks ([draft-ietf-jmap-tasks-06]) in one dependency: re-exports
[`jmap-tasks-types`] always, [`jmap-tasks-client`] with the `client`
feature and [`jmap-tasks-server`] with the `server` feature (`memory`
adds the server's in-memory reference backend).

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_tasks::types` |
| `client` | `jmap_tasks::client` |
| `server` | `jmap_tasks::server` |
| `memory` | `server`, plus `jmap_tasks::server::memory` |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapTasksExt` extension trait over
  `jmap-base-client`.
- `server`: method handlers for `TaskList/*`, `Task/*` and `TaskNotification/*` over `TasksBackend`.

## What it's for

Tasks are to-do items in task lists, synchronised over JMAP. Applications that want the whole family depend on this crate
and pick `client`, `server` or both; workspace crates depend on the
component crates directly. Bridges to other families (for example the
`jmap-chat-*-server` crates) are not re-exported; depend on them directly.

## How to use

```toml
[dependencies]
jmap-tasks = { version = "0.5", features = ["client"] }
```

```rust
use jmap_tasks::types::JMAP_TASKS_URI;

assert_eq!(JMAP_TASKS_URI, "urn:ietf:params:jmap:tasks");
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_tasks::server::X` is the same item as `jmap_tasks_server::X`.
The component crates version in lockstep with this one on the shared
`jmap-*` line.

## Gotchas

- `server` alone does not build the reference backend; enable `memory` for
  `server::memory::MemoryBackend` (demonstration quality, not production).
- Depending on this crate and on a component crate at a different version
  gives two copies of each type, which do not unify. Keep them on one
  version line.
- Version 0.5.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-ietf-jmap-tasks-06] — JMAP Tasks
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-tasks-06]: https://datatracker.ietf.org/doc/draft-ietf-jmap-tasks/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-tasks-types`]: https://crates.io/crates/jmap-tasks-types
[`jmap-tasks-client`]: https://crates.io/crates/jmap-tasks-client
[`jmap-tasks-server`]: https://crates.io/crates/jmap-tasks-server
