# jmap-chat-client — Implementation Plan

Auth-agnostic JMAP Chat HTTP client with WebSocket and SSE support.

## Crate Family Position

```
jmap-types
    ├── jmap-base-client              base HTTP client (auth, session, blob, SSE, WebSocket)
    └── jmap-chat-types
            └── jmap-chat-client  ← this crate (depends on jmap-base-client + jmap-chat-types)
```

## What This Crate Is

An HTTP client library for the JMAP Chat extension. Handles:
- JMAP Core request/response (RFC 8620)
- All JMAP Chat methods (Chat, Message, Space, ReadPosition, ChatContact, etc.)
- Blob upload/download (RFC 8620 §6)
- WebSocket event stream (JMAP Chat WebSocket draft)
- SSE event stream
- Pluggable auth: Bearer, Basic, or none (caller supplies the `Authorization` header)

Known consumers: `kithctl` (CLI), any future JMAP Chat web or mobile client.

## What This Crate Is Not

- Not a server-side crate
- Not coupled to a specific auth system — auth is a pluggable provider
- Not opinionated about connection pooling beyond what `reqwest` provides

## Source Material

The original reference implementation was `~/PROJECT/crate-jmapchat-client/`.
This crate has been extracted and the transport layer (auth, HTTP client, error,
blob upload, base SSE, base WebSocket) has been moved into `jmap-base-client`.
What remains in this crate is the chat-specific surface: method handlers,
chat-extended SSE/WS frame wrappers, and chat-specific session/utility helpers.

| Item | Now lives in | Notes |
|---|---|---|
| `JmapClient` (HTTP core) | `jmap-base-client::client` | Consumed via dep; not re-implemented here |
| Auth providers | `jmap-base-client::auth` | `AuthProvider`, `BearerAuth`, `BasicAuth`, `NoneAuth` |
| `ClientError` | `jmap-base-client::error` | Re-exported from this crate's `lib.rs` |
| JMAP request builder | `jmap-base-client::request` | Consumed via dep |
| Base `Session` | `jmap-base-client::session` | Extended here by `ChatSessionExt` (`src/session.rs`) |
| Base `SseEvent` | `jmap-base-client::sse` | Wrapped by `ChatSseEvent` (`src/sse.rs`) |
| Base `WsFrame` | `jmap-base-client::ws` | Wrapped by `ChatWsFrame` (`src/ws.rs`) |
| Blob upload | `jmap-base-client::blob` | Consumed via dep |
| Method impls | `src/methods/` | Chat, Message, Space, SpaceBan, SpaceInvite, ChatContact, CustomEmoji, Blob, Quota, misc. Typed `&Id`/`&State` per bd:JMAP-6by7.3 |
| Utility fns | `src/utils.rs` | `format_receipt_timestamp`, etc. |

Spec:
- `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-00.md`
- `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-wss-00.md`
- `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-push-00.md`

## Dependencies

See `Cargo.toml`. Summary:

- `jmap-types`, `jmap-chat-types`, `jmap-base-client` — workspace path deps
- `ulid`, `chrono` — used by `utils.rs` for receipt-timestamp formatting
- `serde`, `serde_json` — wire format

All HTTP/TLS/WebSocket dependencies (`reqwest`, `tokio-tungstenite`, `url`,
`thiserror`, `tokio`) are transitive through `jmap-base-client`, not direct deps
of this crate.

## Extension Trait Pattern

Cross-crate inherent impls are not valid Rust (orphan rule: only the crate that defines
a type may add inherent methods to it). Chat methods reach `JmapClient` via an extension
trait — `JmapChatExt` — that hands back a `SessionClient` carrying both the client and
the session:

```rust
use jmap_base_client::{JmapClient, Session};
use jmap_chat_client::JmapChatExt;

let session: Session = client.fetch_session().await?;
let sc = client.with_chat_session(session);
let chats = sc.chat_get(&account_id, &ids).await?;
```

Callers must bring the trait into scope: `use jmap_chat_client::JmapChatExt;`

Rust 1.75 AFIT (async fn in trait, via RPITIT) is used for inherent `SessionClient`
methods — no `async-trait` crate needed.

## Typed-Id refactor (bd:JMAP-6by7.3)

The `SessionClient` method API uses **typed `Id` and `State` parameters** end-to-end:

- Id-shaped parameters: `&Id`, `&[Id]`, `Option<&[Id]>`, `Option<&'a Id>` etc.
- State-shaped parameters: `&State`, `Option<&State>`.
- Id-shaped fields on Input/Patch structs (`MessageCreateInput.chat_id`,
  `ChatPatch.pinned_message_ids`, `SpaceAddMemberInput.id`, etc.) are also typed,
  carrying the typed-Id boundary all the way to the call site.
- Caller-chosen creation reference keys (the `client_id: Option<&'a str>` fields,
  `with_client_id(id: &'a str)` methods) remain `String` / `&str` — they are not
  JMAP Ids but caller-chosen creation references resolved by the server.
- Display strings, descriptions, free-text filters, status text, message bodies,
  invite codes, push device-client IDs, and push URLs remain `&str`.

Consequences:
- Inline empty-Id guards have been removed where the typed parameter makes the
  empty case unrepresentable.
- Empty-state guards remain (defence-in-depth) on every `&State` / `Option<&State>`
  parameter.
- Empty-slice guards remain on `/set` `destroy` ids (typed `&[Id]` does not make
  the slice non-empty).

This was the canonical-template propagation of the typed-Id work landed first
in `jmap-calendars-client` (bd:JMAP-6by7.1) and the canonical
`jmap-mail-client` template (bd:JMAP-6by7.2).

## Key Design Decisions vs. jmapchat-client

1. **Use `jmap-types` wire types directly** — `JmapRequest`, `JmapResponse`,
   `Invocation`, `Id`, `UTCDate`, `State` come from `jmap-types`, not re-defined here.

2. **Use `jmap-chat-types` domain types directly** — `Chat`, `Message`, `Space`, etc.
   come from `jmap-chat-types`. A thin `src/types.rs` remains only for client-side
   auxiliary types that are not wire-format (e.g. `ContactPresenceFilter`).

3. **Auth, transport, session, SSE, WebSocket, blob live in `jmap-base-client`** —
   `jmap-base-client` is a workspace path dependency and supplies `AuthProvider`,
   `JmapClient`, `ClientError`, the base `SseEvent`/`WsFrame` types, blob upload,
   and session fetch. This crate's `sse.rs` and `ws.rs` are chat-specific *extension*
   layers that wrap the base types with chat semantics (typing, presence) — not
   duplicates of base-client modules.

4. **Auth is unchanged** — the pluggable `AuthProvider` trait and the three built-in
   providers (`BearerAuth`, `BasicAuth`, `NoneAuth`) come from `jmap-base-client`.

## Module Layout

```
src/
  lib.rs        re-exports + JmapChatExt trait
  session.rs    ChatSessionExt — ChatCapability, ChatPushCapability accessors
  sse.rs        ChatSseEvent, ChatSseFrame — chat-specific SSE wrappers over base SseEvent
  ws.rs         ChatWsExt, ChatWsFrame — chat-specific WS wrappers over base WsFrame
  types.rs      Client-side auxiliary types (e.g. ContactPresenceFilter)
  utils.rs      format_receipt_timestamp, etc.
  methods/
    mod.rs          method input/output types, re-exports, SessionClient
    chat.rs         Chat/get, Chat/set, Chat/query, Chat/changes
    message.rs      Message/get, Message/set, Message/query, Message/changes
    space.rs        Space/get, Space/set, Space/query, Space/changes
    space_ban.rs    SpaceBan/* sub-operations
    space_invite.rs SpaceInvite/* sub-operations
    contact.rs      ChatContact/get, ChatContact/set
    custom_emoji.rs CustomEmoji/* methods
    blob.rs         Blob/copy, Blob/lookup
    quota.rs        Quota/get
    misc.rs         Core/echo, PushSubscription/set
```

## Test Strategy

- Unit tests against a `wiremock` mock server (already used in `jmapchat-client/`)
- Round-trip tests for request serialization using `JmapRequestBuilder`
- WebSocket tests using `tokio-tungstenite`'s test helpers
- No live network in CI — all tests use local mocks or recorded fixtures
