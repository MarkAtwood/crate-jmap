# jmap-base-client — Implementation Plan

RFC 8620 base JMAP client. Auth-agnostic. Session fetch, JMAP request/response,
blob upload/download, SSE event stream, WebSocket session.

Extension-specific clients (`jmap-chat-client`, `jmap-mail-client`) depend on this crate
and add only their method implementations.

## Crate Family Position

```
jmap-types
    └── jmap-base-client  ← this crate
            ├── jmap-chat-client   + Chat methods (also depends on jmap-chat-types)
            └── jmap-mail-client   + RFC 8621 methods (also depends on jmap-mail-types)
```

## What This Crate Is

The base HTTP client for any JMAP server:
- Pluggable auth (`AuthProvider` trait: Bearer, Basic, None, custom CA)
- Session document fetch and parse (RFC 8620 §2)
- JMAP method call dispatch: `JmapClient::call(JmapRequest) -> JmapResponse`
- `JmapRequestBuilder` for constructing batched requests
- Blob upload/download (RFC 8620 §6)
- SSE event stream subscription (RFC 8620 §7.3 / JMAP push)
- WebSocket session (RFC 8887)
- `ClientError` covering HTTP, auth, and deserialization failures

## What This Crate Is Not

- Not coupled to any JMAP extension (Chat, Mail, Calendars, etc.)
- Not opinionated about connection pooling beyond `reqwest`
- Not a server-side crate

## Source Material

The reference implementation is `~/PROJECT/crate-jmapchat-client/`. Extract and
generalize — replace Chat-specific type references with `jmap-types` equivalents.

| Item | Source file | Notes |
|---|---|---|
| `AuthProvider` trait + impls | `src/auth.rs` | `BearerAuth`, `BasicAuth`, `NoneAuth`, `CustomCaTransport`, `DefaultTransport`, `TransportConfig` — copy verbatim, no Chat types involved |
| `ClientError` | `src/error.rs` | Copy verbatim |
| `JmapClient` struct + `new()` | `src/client.rs` | Rename `JmapChatClient` → `JmapClient`; remove Chat-specific methods (those move to `jmap-chat-client`); keep `fetch_session`, `call`, `subscribe_events`, `upload_blob`, `download_blob` |
| SSE parser | `src/sse.rs` | `SseEvent`, `SseFrame`, `parse_sse_block` — copy verbatim |
| WebSocket session | `src/ws/mod.rs` | `WsSession`, `WsFrame` — copy verbatim, update type imports to `jmap-types` |
| Blob types | `src/blob.rs` | `BlobUploadResponse` — copy verbatim |
| `JmapRequestBuilder` | `src/jmap.rs` | Extract builder; replace locally-defined `Id`, `UTCDate`, `JmapRequest`, `JmapResponse`, `Invocation` with re-exports from `jmap-types`; keep `AccountInfo`, capability structs |

**Key simplification**: `src/jmap.rs` in `jmapchat-client` redefines `Id`, `UTCDate`,
`JmapRequest`, `JmapResponse`, `Invocation`, `ResultReference`, and `Session` from
scratch. All of these now come from `jmap-types`. Delete the redundant definitions;
keep only `JmapRequestBuilder`, `AccountInfo`, and the capability structs that are not
in `jmap-types`.

## Dependencies

```toml
jmap-types        = { path = "../crate-jmap-types" }
futures           = "0.3"
reqwest           = { version = "0.12", features = ["json", "stream", "rustls-tls-webpki-roots"] }
serde             = { version = "1", features = ["derive"] }
serde_json        = "1"
thiserror         = "2"
tokio             = { version = "1", features = ["rt"] }
tokio-tungstenite = { version = "0.29", features = ["rustls-tls-webpki-roots"] }
url               = "2"
```

Note: `jmapchat-client` also pulls in `chrono`, `base64`, `sha2`, `ulid`. Audit these
after extraction — several likely exist only because the client redefined types that are
now in `jmap-types` or `jmap-chat-types`.

## Impact on jmap-chat-client

Once `jmap-base-client` exists, `jmap-chat-client` should:
1. Add `jmap-base-client = { path = "../crate-jmap-base-client" }` dependency
2. Remove `auth.rs`, `blob.rs`, `client.rs`, `error.rs`, `sse.rs`, `ws/`, `utils.rs`
   (or keep only chat-specific utils)
3. Re-export `JmapClient`, auth providers, `ClientError` etc. from `jmap-base-client`
4. Retain only `methods/` and Chat-specific type re-exports

## Module Layout

```
src/
  lib.rs        re-exports
  auth.rs       AuthProvider, BearerAuth, BasicAuth, NoneAuth, CustomCaTransport,
                DefaultTransport, TransportConfig
  blob.rs       BlobUploadResponse
  client.rs     JmapClient — new(), fetch_session(), call(), upload_blob(),
                download_blob(), subscribe_events()
  error.rs      ClientError
  request.rs    JmapRequestBuilder, AccountInfo, capability structs
  sse.rs        SseEvent, SseFrame, parse_sse_block
  ws/
    mod.rs      WsSession, WsFrame
```

## Test Strategy

- `JmapClient::new()` validation: empty URL, wrong scheme, URL with path/query/fragment
- `fetch_session`: mock server returns valid session JSON → `AccountInfo` parses correctly
- `call`: mock server returns valid `JmapResponse` → deserialized correctly
- `JmapRequestBuilder`: unit tests for request construction (no network)
- SSE: parse known SSE block formats (from `src/sse.rs` existing tests)
- Auth: `BearerAuth` adds correct `Authorization` header
- All tests use `wiremock` for HTTP mocking — no live network
