# jmap-base-client — Design Plan

RFC 8620 base JMAP client. Auth-agnostic. Session fetch, JMAP request/response,
blob upload/download, SSE event stream, WebSocket session.

Extension-specific clients (`jmap-chat-client`, `jmap-mail-client`) depend on
this crate and add only their method implementations.

## Crate Family Position

```
jmap-types
    └── jmap-base-client  ← this crate
            ├── jmap-chat-client   + Chat methods (also depends on jmap-chat-types)
            └── jmap-mail-client   + RFC 8621 methods (also depends on jmap-mail-types)
```

## What This Crate Is

The base HTTP client for any JMAP server:

- Pluggable auth (`AuthProvider` trait: `BearerAuth`, `BasicAuth`, `NoneAuth`,
  plus a `TransportConfig` trait — `DefaultTransport`, `CustomCaTransport` —
  to swap TLS trust roots)
- Session document fetch and parse (RFC 8620 §2)
- JMAP method call dispatch: `JmapClient::call(JmapRequest) -> JmapResponse`
- Blob upload/download (RFC 8620 §6)
- SSE event stream subscription (RFC 8620 §7.3 / JMAP push)
- WebSocket session (RFC 8887)
- `extract_response::<T>` helper for typed result extraction
- `ClientError` covering HTTP, auth, deserialization, size-cap, and
  blob-integrity failures

## What This Crate Is Not

- Not coupled to any JMAP extension (Chat, Mail, Calendars, etc.)
- Not opinionated about connection pooling beyond `reqwest`
- Not a server-side crate

## Dependencies

See `Cargo.toml` for the authoritative list (versions inherited via
`workspace = true`). The notable design constraints are:

- `reqwest` and `tokio-tungstenite` are pinned with
  `default-features = false` and only `rustls-tls-*` features so the
  workspace's "no openssl" TLS-stack policy (workspace `AGENTS.md`) is
  preserved. They are private dependencies after the SemVer-isolation work
  in JMAP-6lsm.22 — `HttpError`, `WebSocketError`, and
  `InvalidHeaderValueError` wrap them so the transport is replaceable
  without breaking downstream consumers.
- `jmap-cid-types` is a typed-Sha256 dependency, used only by
  `BlobUploadResponse.sha256`.

## Module Layout

```
src/
  lib.rs        re-exports
  auth.rs       AuthProvider, BearerAuth, BasicAuth, NoneAuth,
                CustomCaTransport, DefaultTransport, TransportConfig
  blob.rs       BlobUploadResponse, DownloadBlobParams, upload_blob,
                download_blob, SHA-256 verification helpers
  client.rs     JmapClient, ClientConfig, fetch_session, call,
                subscribe_events, connect_ws_session, extract_response,
                read_capped_body (streaming size-cap helper)
  error.rs      ClientError, HttpError, WebSocketError,
                InvalidHeaderValueError
  push.rs       StateChange — RFC 8620 §7.1 push notification body
  request.rs    Session, AccountInfo, WebSocketCapability
  sse.rs        SseEvent, SseFrame, parse_sse_block, SSE block parser
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

## Streaming size-cap policy (JMAP-6r7c.1)

All HTTP body reads in this crate (`fetch_session`, `call`, `upload_blob`,
`download_blob`) route through the `read_capped_body(resp, limit)` helper
in `src/client.rs`. The helper does a Content-Length fast-path rejection
and then streams chunks while enforcing the cap before each accumulation.
This prevents a hostile or compromised server using chunked
Transfer-Encoding (RFC 7230 §3.3.1) without Content-Length, or
under-reporting Content-Length, from forcing the client to allocate the
full server-controlled body before the size cap can fire.

Any new HTTP body read added to this crate (a new method on `JmapClient`,
a new extension hook, etc.) MUST use `read_capped_body` — never call
`resp.bytes().await` directly. The integration test in
`tests/streaming_cap_tests.rs` uses a raw `tokio::net::TcpListener` to
fabricate a chunked-no-Content-Length response that hangs without an
end-of-stream frame; the post-fix client returns `ResponseTooLarge`
immediately, while a regression to `.bytes().await` would block in the
test and the 2-second timeout would fail it.
