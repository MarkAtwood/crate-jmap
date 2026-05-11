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

## Extras-preservation policy (JMAP-lbdy)

Every public method-response struct and push-notification struct defined in
this crate that appears on the JMAP wire carries an `extra` field per the
workspace extras-preservation policy (see workspace `AGENTS.md`):

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

This preserves vendor / site / private-extension fields across
deserialize/serialize round-trip. Wire format is byte-identical when extras
are empty. The `default` attribute is active on the Deserialize side and
inert on the Serialize side; the attribute set is kept uniform across both
directions of wire flow.

In scope in this crate (each has at least one round-trip preservation
test following the `*_preserves_vendor_extras` pattern; for the
bidirectional `StateChange`, the test covers the deserialize side):

- `BlobUploadResponse` in `src/blob.rs` (Deserialize; response to the
  RFC 8620 §6.1 blob upload endpoint).
- `Session` and `AccountInfo` in `src/request.rs` (Deserialize; the
  RFC 8620 §2 session document and its nested account info objects).
- `WebSocketCapability` in `src/request.rs` (Deserialize; the RFC 8887
  WebSocket capability object).
- `StateChange` in `src/push.rs` (Serialize + Deserialize; the
  RFC 8620 §7.1 push notification body).

Note: `Session` and `AccountInfo` have manual `impl std::fmt::Debug`
blocks rather than `#[derive(Debug)]`. Those impls were updated in
JMAP-lbdy.9 to include the new `extra` field. Any further manual `Debug`
impls added to in-scope structs in this crate MUST be extended in the
same way so the `extra` map is visible in debug output.

This is the foundation client crate; the canonical-template-sibling
propagation rule that applies to the extension-client family (workspace
AGENTS.md "Canonical Templates") does NOT apply here. Changes to the
extras shape in this crate ripple through every `*-client` extension
because they all depend on these foundation types, but there is no
peer foundation crate to mirror against.

Out of scope (explicitly excluded by the workspace policy):

- Filter / comparator algebra types and control enums — see workspace
  AGENTS.md "Filter algebra and control enums are explicitly EXCLUDED"
  for the full rationale.
- Internal Rust state types (`JmapClient`, `BearerAuth`, `ClientConfig`,
  and the other auth / transport / SSE / WebSocket plumbing types) —
  not wire-format.

### New-type rule

Any new public method-response or push-notification struct added to this
crate that appears on the JMAP wire MUST include the `extra` field from
day one with the documented serde attributes and at least one round-trip
preservation test. Any new manual `impl std::fmt::Debug` block for an
in-scope struct MUST include the `extra` field in its debug output.

## Test Strategy

- `JmapClient::new()` validation: empty URL, wrong scheme, URL with path/query/fragment
- `fetch_session`: mock server returns valid session JSON → `AccountInfo` parses correctly
- `call`: mock server returns valid `JmapResponse` → deserialized correctly
- `JmapRequestBuilder`: unit tests for request construction (no network)
- SSE: parse known SSE block formats (from `src/sse.rs` existing tests)
- Auth: `BearerAuth` adds correct `Authorization` header
- All tests use `wiremock` for HTTP mocking — no live network
