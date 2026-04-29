# jmap-chat-server — Implementation Plan

JMAP Chat extension method handlers. Plugs into `jmap-server`'s `Dispatcher`.
Backend-agnostic: defines a `ChatBackend` trait; consumers provide the implementation.

## Crate Family Position

```
jmap-types
    ├── jmap-server          dispatcher
    └── jmap-chat-types      data types
            └── jmap-chat-server  ← this crate
```

## What This Crate Is

Method handler implementations for the JMAP Chat extension: `Chat/get`, `Chat/set`,
`Chat/query`, `Message/get`, `Message/set`, `Message/query`, `Space/get`, `Space/set`,
`ReadPosition/get`, `ReadPosition/set`, `ChatContact/get`, `ChatContact/set`, etc.

Defines a `ChatBackend` trait (analogous to `StorageBackend` in `jmapchat-server`) that
the application implements. The crate handles JMAP semantics; the backend handles storage.

Known consumer: `kith` (the Tailscale-authenticated JMAP Chat server).

## What This Crate Is Not

- Not a full JMAP server
- Not coupled to any specific storage system (SQLite, PostgreSQL, in-memory)
- Not handling auth — caller's responsibility before `Dispatcher::dispatch()`
- Not axum-specific — any `http`-based framework works

## Source Material

The reference implementation is `~/PROJECT/crate-jmapchat-server/jmapchat-server/`.
This crate is an extraction and adaptation, not a rewrite.

| Item | Source file | Notes |
|---|---|---|
| `ChatBackend` trait | `jmapchat-server/src/backend.rs` | Rename `StorageBackend` → `ChatBackend`; swap `jmapchat-types` deps for `jmap-chat-types` + `jmap-types` |
| `BackendChangesError`, `BackendSetError` | `jmapchat-server/src/backend.rs` | Copy verbatim |
| `ChangesResult`, `QueryResult` | `jmapchat-server/src/backend.rs` | Copy verbatim |
| `Dispatcher<B>`, `RequestContext` | `jmapchat-server/src/dispatcher.rs` | Strip Tailscale/kith-specific caps; `SUPPORTED_CAPABILITIES` becomes a const on the crate |
| `Method` enum | `jmapchat-server/src/method.rs` | Rename variants to match `jmap-chat-types` type names |
| Push / `PushSink` / `Notifier` | `jmapchat-server/src/push.rs` | Copy with type updates |
| `ResultReferenceStore` | `jmapchat-server/src/ref_store.rs` | This logic now lives in `jmap-server::resolve_args`; remove entirely or thin wrapper |

Spec: `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-00.md`

## Dependencies

```toml
jmap-types      = { path = "../crate-jmap-types" }
jmap-chat-types = { path = "../crate-jmap-chat-types" }
jmap-server     = { path = "../crate-jmap-server" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror  = "2"
tokio      = { version = "1", features = ["rt"] }
```

## Key Design Decisions vs. jmapchat-server

1. **No `ResultReferenceStore`** — `jmap-server::resolve_args` handles RFC 8620 §3.7
   resolution before dispatch. This crate does not need its own implementation.

2. **`jmap-server::Dispatcher<CallerCtx>`** is the dispatcher — this crate registers
   handlers with it via a `register_chat_handlers()` function, same pattern as
   `jmap-mail-server`. The `jmapchat-server::Dispatcher` is not used here.

3. **AFIT** (`async fn in trait`, stable since Rust 1.75) — no `#[async_trait]` macro.
   Same as `jmapchat-server/src/backend.rs` already does.

4. **`ChatBackend` is not object-safe** — generic over `B: ChatBackend`, monomorphized
   at compile time. Same invariant as `StorageBackend` in the reference.

## Planned Public API

```rust
/// Implement for your chat storage system.
pub trait ChatBackend: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    // Chat
    async fn get_objects<O: GetObject>(...) -> Result<(Vec<O>, Vec<Id>), Self::Error>;
    async fn create_object<O: SetObject>(...) -> Result<(Id, O), BackendSetError<Self::Error>>;
    async fn update_object<O: SetObject>(...) -> Result<Option<O>, BackendSetError<Self::Error>>;
    async fn destroy_object<O: SetObject>(...) -> Result<(), BackendSetError<Self::Error>>;
    async fn get_state<O: JmapObject>(...) -> Result<State, Self::Error>;
    async fn get_changes<O: JmapObject>(...) -> Result<ChangesResult, BackendChangesError<Self::Error>>;
    async fn query_objects<O: QueryObject>(...) -> Result<QueryResult, Self::Error>;
    async fn get_session(&self, user_id: &Id) -> Result<Session, Self::Error>;
    fn supports_type<O: JmapObject>(&self) -> bool;
}

/// Register all JMAP Chat handlers with a jmap-server Dispatcher.
pub fn register_chat_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where B: ChatBackend + 'static, C: Clone + Send + 'static;

pub use backend::{BackendChangesError, BackendSetError, ChangesResult, QueryResult};
```

## Module Layout

```
src/
  lib.rs       re-exports; register_chat_handlers
  backend.rs   ChatBackend trait + BackendChangesError/BackendSetError/ChangesResult/QueryResult
  chat.rs      Chat/get, Chat/set, Chat/changes, Chat/query handlers
  message.rs   Message/get, Message/set, Message/changes, Message/query handlers
  space.rs     Space/get, Space/set, Space/changes, Space/query handlers
  contact.rs   ChatContact/get, ChatContact/set handlers
  position.rs  ReadPosition/get, ReadPosition/set handlers
  push.rs      PushSink, Notifier, NullSink, StateChange
```

## Test Strategy

A `MemoryBackend` (HashMap-based, same as in `jmapchat-server`'s integration tests)
lives in `tests/` as both test harness and canonical example for implementors.

Dispatch tests extract from `~/PROJECT/crate-jmapchat-server/jmapchat-server/tests/`:
- `dispatch_happy_path.rs`
- `dispatch_error_paths.rs`
- `result_reference_tests.rs`

Update type imports from `jmapchat-types` → `jmap-chat-types` + `jmap-types`.
The oracle values (expected JSON, error codes) stay the same.
