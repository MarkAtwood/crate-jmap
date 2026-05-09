# Agent Instructions — jmap-base-client

This crate has been reviewed and stabilized. Its public API, auth traits, wire
behavior, error variants, configuration types, and test oracles are stabilized.
Treat it as a high-care surface — every downstream `jmap-*-client` crate depends
on it.

Prefer non-breaking changes:
- New accessor methods on `HttpError` / `WebSocketError` (the wrapper types are
  `#[non_exhaustive]`)
- New variants on existing `#[non_exhaustive]` enums
- New methods on `JmapClient`, new fields on `ClientConfig` (also `#[non_exhaustive]`)
- New free functions

Reshape carefully:
- Changing `ClientError` variant fields, `AuthProvider` / `TransportConfig`
  method signatures, or `JmapClient` method signatures is a SemVer break.
  Bundle such changes with a `0.x.0 → 0.(x+1).0` version bump and a clear
  changelog/upgrade-guide entry.

`reqwest` and `tokio-tungstenite` are private dependencies after the SemVer-isolation
work in JMAP-6lsm.22 — `HttpError`, `WebSocketError`, and `InvalidHeaderValueError`
wrap them so the transport is replaceable without breaking downstream. Bump those
deps freely; just keep the wrapper accessor signatures, `Display` text, and
`Error::source` chain stable across the swap.

## Before Starting Any Work

1. Read `PLAN.md` — design rationale, public API, source material
2. Run `bd ready` — check for open issues before creating new ones

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
| TLS | `TransportConfig` trait — `DefaultTransport` and `CustomCaTransport`. Backed by rustls (NOT native-tls / openssl) — see workspace `AGENTS.md` "TLS stack" rule. `reqwest` and `tokio-tungstenite` are pinned with `default-features = false` and only `rustls-tls-*` features. |
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
