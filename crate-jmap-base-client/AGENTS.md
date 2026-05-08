# Agent Instructions — jmap-base-client

## 🔒 LOCKED CRATE — EXPLICIT PERMISSION REQUIRED BEFORE ANY CHANGE

This crate has been reviewed and stabilized. Its public API, auth traits, wire
behavior, error variants, configuration types, and test oracles are locked.

**You may NOT, under any circumstances:**
- Add, rename, or remove any public type, trait, method, or field
- Change `ClientError` variants, their fields, or their `#[error]` strings
- Change the `HttpError`, `WebSocketError`, or `InvalidHeaderValueError`
  wrapper types' accessor signatures or `Display` output
- Change `AuthProvider`, `TransportConfig`, or their method signatures
- Change `ClientConfig` field names, types, or defaults
- Change `JmapClient` method signatures (`new`, `new_plain`, `fetch_session`,
  `call`, `subscribe_events`, `upload_blob`, `download_blob`)
- Change `WsSession`, `WsFrame`, `SseFrame`, or `SseEvent` shapes
- Change `Session`, `AccountInfo`, or `WebSocketCapability` field names or serde
- Alter `#[non_exhaustive]` annotations
- Modify test oracles or fixture files
- Refactor or "clean up" any existing code

**You MAY** (these are explicitly allowed and do not require per-change
approval, since `reqwest` and `tokio-tungstenite` are private dependencies
of this crate after the SemVer-isolation work in JMAP-6lsm.22):
- Bump `reqwest` major versions
- Bump `tokio-tungstenite` major versions
- Swap to a different HTTP or WebSocket library entirely
- Add additional accessor methods to `HttpError` / `WebSocketError` (the
  types are `#[non_exhaustive]`)

…provided the `HttpError`, `WebSocketError`, and `InvalidHeaderValueError`
public surface — accessor signatures, `Display` text, `Error::source` chain
— remains unchanged across the swap. The whole point of the wrappers is
that the underlying transport is replaceable without breaking downstream.

**To make ANY change to this crate** you must first describe the exact change to
the user and receive explicit written approval for that specific change. "Fixing
a bug" or "improving the code" is not sufficient — stop and report, then wait.

## Before Starting Any Work

1. Read `PLAN.md` — design rationale, public API, source material
2. Run `bd ready` — check for open issues before creating new ones
3. Ask the user for explicit permission before touching any source file

## What This Is

RFC 8620 base JMAP client: auth-agnostic HTTP transport, session fetch, API
calls, blob upload/download, SSE event streaming, and WebSocket support.
Extension crates (`jmap-mail-client`, `jmap-chat-client`) build on top of this.

## Crate Family Context

```
jmap-types
    └── jmap-base-client  ← this crate
            ├── jmap-mail-client
            └── jmap-chat-client
```

## Build & Test

```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test -p jmap-base-client
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p jmap-base-client
```

Run all four before considering any work done.

## Design Constraints (Settled)

| Decision | Choice |
|---|---|
| Auth | `AuthProvider` trait — transport and credentials are independent |
| TLS | `TransportConfig` trait — `DefaultTransport` and `CustomCaTransport` |
| Error type | `ClientError` enum with `#[non_exhaustive]` and `thiserror`; `Http` / `WebSocket` / `InvalidHeaderValue` variants wrap opaque `HttpError` / `WebSocketError` / `InvalidHeaderValueError` so `reqwest` and `tokio-tungstenite` stay private deps |
| SSE framing | `SseStreamState` unfold loop with `scan_from` 3-byte overlap |
| UTF-8 streaming | `raw_buf` + `decode_utf8_chunk` split-sequence handling |
| WS frames | `WsRequestFrame` with `#[serde(flatten)]` — single-pass serialization |
| Size caps | Content-Length early exit + streaming per-chunk cap in `download_blob` |
| Unsafe | Forbidden — `#![forbid(unsafe_code)]` |

## Non-Interactive Shell Commands

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

Use `-o BatchMode=yes` for scp/ssh. Use `-y` for apt-get.
