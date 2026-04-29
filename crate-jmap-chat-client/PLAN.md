# jmap-chat-client — Implementation Plan

Auth-agnostic JMAP Chat HTTP client with WebSocket and SSE support.

## Crate Family Position

```
jmap-types
    └── jmap-chat-types
            └── jmap-chat-client  ← this crate
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
| Method impls | `src/methods/` | All files — update type imports; keep method signatures identical |
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

## Key Design Decisions vs. jmapchat-client

1. **Use `jmap-types` wire types directly** — `JmapRequest`, `JmapResponse`,
   `Invocation`, `Id`, `UTCDate`, `State` come from `jmap-types`, not re-defined here.
   Remove the parallel definitions in `src/jmap.rs` where they duplicate `jmap-types`.

2. **Use `jmap-chat-types` domain types directly** — `Chat`, `Message`, `Space`, etc.
   come from `jmap-chat-types`. The `src/types.rs` re-export layer in `jmapchat-client`
   may be eliminable or significantly thinned.

3. **Auth is unchanged** — the pluggable `AuthProvider` trait and the three built-in
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
