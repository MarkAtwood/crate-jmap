# jmap-chat-server

JMAP Chat extension method handlers for Rust ([draft-atwood-jmap-chat]). Backend-agnostic
— plugs into `jmap-server::Dispatcher`. Consumers implement `ChatBackend` for their
storage; the crate handles all wire protocol, RFC 8620 conformance, and non-trivial
concurrency logic (Direct chat dedup, `Space/join` TOCTOU, invite-use accounting).

## Usage

```rust
use std::sync::Arc;
use jmap_chat_server::{ChatBackend, register_chat_handlers};
use jmap_server::Dispatcher;

// 1. Implement ChatBackend for your storage.
struct MyChatBackend { /* db pool, etc. */ }
impl ChatBackend for MyChatBackend { /* ... */ }

// 2. Wire into a Dispatcher.
let mut dispatcher: Dispatcher<()> = Dispatcher::new();
register_chat_handlers(&mut dispatcher, Arc::new(MyChatBackend { /* ... */ }));

// 3. Dispatch requests in your HTTP handler.
// let response = dispatcher.dispatch(request, (), session_state).await;
```

## Registered methods

`register_chat_handlers` installs handlers for all of the following:

| Object | Methods |
|---|---|
| `Chat` | `get`, `changes`, `query`, `queryChanges`, `set`, `typing` |
| `Message` | `get`, `changes`, `query`, `queryChanges`, `set` |
| `Space` | `get`, `changes`, `query`, `queryChanges`, `set`, `join` |
| `ChatContact` | `get`, `changes`, `query`, `queryChanges`, `set` |
| `ReadPosition` | `get`, `changes`, `set` |
| `SpaceInvite` | `get`, `changes`, `set` |
| `SpaceBan` | `get`, `changes`, `set` |
| `CustomEmoji` | `get`, `changes`, `query`, `queryChanges`, `set` |
| `PresenceStatus` | `get`, `changes`, `set` |

## `ChatBackend` trait

```rust
pub trait ChatBackend: JmapBackend {
    /// Create a new object. Returns `(assigned_id, created_object)` on success.
    /// `create_id` is the client-side creation id from the `/set` request.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    /// Apply a partial update (patch) to an existing object.
    /// Returns `Some(updated_object)` if the backend modified any properties beyond
    /// what the client requested (RFC 8620 §5.3 server-set field echo), or `None`
    /// if the patch was applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an existing object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    /// Called by the session/capability builder — NOT called internally by handlers.
    /// Backends that support all types unconditionally can return `true` always.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Generate a cryptographically random invite code.
    /// MUST use a CSPRNG. Do NOT use timestamps or sequential counters.
    fn generate_invite_code(&self) -> String;
}
```

The read-side operations (`get_objects`, `get_state`, `get_changes`, `query_objects`,
`query_changes`) are defined on the `JmapBackend` supertrait from `jmap-server`.

## How it works

### Generic handlers

`*/get`, `*/changes`, `*/query`, and `*/queryChanges` all delegate to generic
implementations from `jmap-server`, ensuring RFC 8620 conformance across every object type
without duplication.

### `Chat/set` — Direct chat deduplication

Creating a `Chat` with `kind: "direct"` must be idempotent: if a Direct chat with the same
member set already exists, the caller should get back the existing chat rather than a
duplicate.

The handler uses optimistic create-then-validate deduplication:

1. Before the create loop, fetch all existing Direct chats for the account once (hoisted to
   avoid N+1 queries).
2. Attempt the create normally.
3. After the create, re-fetch to catch concurrent races where two clients create the same
   Direct chat simultaneously.
4. The canonical winner is the object with the lexicographically smallest ID. The loser
   destroys itself and returns an `alreadyExists` SetError pointing at the winner's ID.

### `Space/join` — invite-code and TOCTOU handling

`Space/join` is a non-standard method (not `/set`) that adds the caller as a member. It
accepts either an `inviteCode` or a `spaceId` (for public spaces).

- After the join write succeeds at the storage layer, the handler re-reads the member list
  to detect concurrent joins. If two callers both succeed, the racer with the later
  `joinedAt` timestamp undoes its write and returns a retryable error.
- `SpaceInvite.uses` is incremented only on the success path (after the TOCTOU check), so
  race-losses do not silently exhaust invite capacity.

### `Chat/typing` — ephemeral push

`Chat/typing` stores no state. It fires a push event via `JmapBackend::push_event` if the
backend implements push, then returns immediately. The wire response carries no payload
beyond the standard invocation envelope.

## CallerCtx

`register_chat_handlers` takes `CallerCtx = ()` — the handlers discard it. If your
application needs per-request auth context inside a handler (e.g. to enforce that a caller
can only modify their own `PresenceStatus`), implement `JmapHandler` directly for that
method instead of relying on the registered handler.

## Crate family

```
jmap-types
    ├── jmap-server          Dispatcher this crate plugs into
    └── jmap-chat-types      domain types (Chat, Message, Space, etc.)
            └── jmap-chat-server   ← this crate
```

## Spec references

| Document | Covers |
|---|---|
| [draft-atwood-jmap-chat-00] | Core Chat objects and methods |
| [draft-atwood-jmap-chat-push-00] | Push notification payloads |
| [draft-atwood-jmap-chat-wss-00] | WebSocket ephemeral events |
| [RFC 8620] | JMAP Core (request format, SetError, ResultReference) |

[draft-atwood-jmap-chat]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-push-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-wss-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

## License

MIT OR Apache-2.0
