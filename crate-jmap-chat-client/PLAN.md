# jmap-chat-client — Implementation Plan

Auth-agnostic JMAP Chat HTTP client with WebSocket and SSE support.

## Crate Family Position

```
jmap-types
    ├── jmap-base-client              base HTTP client (auth, session, blob, SSE, WebSocket)
    └── jmap-chat-types
            └── jmap-chat-client  ← this crate (will depend on jmap-base-client once it exists)
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

The reference implementation is `~/PROJECT/crate-jmapchat-client/`.
This crate is an extraction and adaptation.

| Item | Source file | Notes |
|---|---|---|
| `JmapChatClient` | `src/client.rs` | Core HTTP client struct; swap `jmapchat-types` → `jmap-chat-types` + `jmap-types` |
| Auth providers | `src/auth.rs` | `AuthProvider`, `BearerAuth`, `BasicAuth`, `NoneAuth`, `TransportConfig` — copy verbatim |
| Error types | `src/error.rs` | `ClientError` — copy verbatim, update type imports |
| JMAP wire helpers | `src/jmap.rs` | `JmapRequestBuilder`, `AccountInfo`, capability structs — adapt to use `jmap-types` wire types directly where possible |
| Method impls | `src/methods/` | All files — update type imports; method signatures use typed `&Id`/`&State` per bd:JMAP-6by7.3 |
| WebSocket session | `src/ws/mod.rs` | `WsSession`, `WsFrame` — copy with type updates |
| SSE | `src/sse.rs` | `SseEvent`, `SseFrame` — copy verbatim |
| Blob | `src/blob.rs` | `BlobUploadResponse` — copy verbatim |
| Utility fns | `src/utils.rs` | `format_receipt_timestamp` etc. — copy verbatim |

Spec:
- `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-00.md`
- `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-wss-00.md`
- `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-push-00.md`

## Dependencies

```toml
jmap-types      = { path = "../crate-jmap-types" }
jmap-chat-types = { path = "../crate-jmap-chat-types" }
reqwest           = { version = "0.12", features = ["json", "stream", "rustls-tls-webpki-roots"] }
serde             = { version = "1", features = ["derive"] }
serde_json        = "1"
thiserror         = "2"
tokio             = { version = "1", features = ["rt"] }
tokio-tungstenite = { version = "0.29", features = ["rustls-tls-webpki-roots"] }
url               = "2"
```

Note: `jmapchat-client` also depends on `chrono`, `base64`, `sha2`, and `ulid`. Audit
whether those are still needed once type imports are updated (some may have been needed
for types now supplied by `jmap-chat-types`).

## Extension Trait Pattern

Cross-crate inherent impls are not valid Rust (orphan rule: only the crate that defines
a type may add inherent methods to it). When `jmap-base-client` is adopted as the base
transport (see Key Design Decision 3 below), Chat methods will be added to `JmapClient`
via an extension trait — not via `impl JmapClient`:

```rust
use jmap_base_client::{ClientError, JmapClient};

/// Extension trait adding JMAP Chat methods to [`JmapClient`].
///
/// Import this trait to use: `use jmap_chat_client::JmapChatExt;`
pub trait JmapChatExt {
    async fn chat_get(&self, account_id: &Id, ids: &[Id])
        -> Result<GetResponse<Chat>, ClientError>;
    async fn chat_set(&self, account_id: &Id, req: SetRequest<Chat>)
        -> Result<SetResponse<Chat>, ClientError>;
    // ... all Chat, Message, Space, ReadPosition, ChatContact methods
}

impl JmapChatExt for JmapClient {
    // implementations in methods/
}
```

Callers must bring the trait into scope: `use jmap_chat_client::JmapChatExt;`

Rust 1.75 AFIT (async fn in trait, via RPITIT) is used — no `async-trait` crate needed
for the common non-dyn case. If `dyn JmapChatExt` is ever required, add `async-trait 0.1`
at that time.

During the skeleton stage (before `jmap-base-client` is ready), `JmapChatClient` is a
standalone struct in `src/client.rs`. The trait-based API is the target end state.

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
   Remove the parallel definitions in `src/jmap.rs` where they duplicate `jmap-types`.

2. **Use `jmap-chat-types` domain types directly** — `Chat`, `Message`, `Space`, etc.
   come from `jmap-chat-types`. The `src/types.rs` re-export layer in `jmapchat-client`
   may be eliminable or significantly thinned.

3. **Auth, transport, session, SSE, WebSocket, blob belong in `jmap-base-client`** —
   `jmap-base-client` now exists in the workspace. Add it as a dependency and remove
   the corresponding duplicated modules from this crate (`auth.rs`, `blob.rs`,
   `client.rs`, `error.rs`, `sse.rs`, `ws/`). Tracked under `bd:JMAP-g7wu.7`.

4. **Auth is unchanged** — the pluggable `AuthProvider` trait and the three built-in
   providers (`BearerAuth`, `BasicAuth`, `NoneAuth`) are directly portable.

## Module Layout

```
src/
  lib.rs        re-exports
  auth.rs       AuthProvider, BearerAuth, BasicAuth, NoneAuth, TransportConfig
  blob.rs       BlobUploadResponse
  client.rs     JmapChatClient — core HTTP request/response
  error.rs      ClientError
  jmap.rs       JmapRequestBuilder, AccountInfo, capability structs
  sse.rs        SseEvent, SseFrame
  utils.rs      format_receipt_timestamp, etc.
  methods/
    mod.rs      method input/output types, re-exports
    chat.rs     Chat/get, Chat/set, Chat/query, Chat/changes
    message.rs  Message/get, Message/set, Message/query, Message/changes
    space.rs    Space/get, Space/set, Space/query, Space/changes
    contact.rs  ChatContact/get, ChatContact/set
    blob.rs     Blob/copy, Blob/lookup
    quota.rs    Quota/get
    misc.rs     Core/echo, PushSubscription/set
  ws/
    mod.rs      WsSession, WsFrame — WebSocket event stream
```

## Test Strategy

- Unit tests against a `wiremock` mock server (already used in `jmapchat-client/`)
- Round-trip tests for request serialization using `JmapRequestBuilder`
- WebSocket tests using `tokio-tungstenite`'s test helpers
- No live network in CI — all tests use local mocks or recorded fixtures
