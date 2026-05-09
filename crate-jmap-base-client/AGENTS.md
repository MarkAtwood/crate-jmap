# Agent Instructions — jmap-base-client

## 🧬 Canonical foundation client — every `*-client` extension uses this

This crate is the **foundation that every `jmap-*-client` extension is built
on**. The auth traits (`AuthProvider`, `TransportConfig`), the transport
(`JmapClient`), the SSE/WebSocket plumbing, the `ClientError` variant set,
and the wrapper-types abstraction (`HttpError`, `WebSocketError`,
`InvalidHeaderValueError`) all live here. Extension `*-client` crates
import these and add their own `Jmap*Ext` extension trait + `SessionClient`
on top.

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- Any reshape of `ClientError` variants, `AuthProvider` / `TransportConfig`
  method signatures, `JmapClient` method signatures, or `ClientConfig`
  field set ripples through every `*-client` extension crate. Plan the
  downstream propagation in the same pass (or file a follow-up sweep
  bead before merging).
- New accessor methods on `HttpError` / `WebSocketError` (the wrapper types
  are `#[non_exhaustive]`), new variants on existing `#[non_exhaustive]`
  enums, new methods on `JmapClient`, and new fields on `ClientConfig` are
  the additive shape — no SemVer break, no propagation churn.

Prefer non-breaking changes. Reshape only when the workspace is bumping
a major (e.g. the JMAP-6by7 typed-Id epic).

`reqwest` and `tokio-tungstenite` are private dependencies after the
SemVer-isolation work in JMAP-6lsm.22 — `HttpError`, `WebSocketError`, and
`InvalidHeaderValueError` wrap them so the transport is replaceable without
breaking downstream. Bump those deps freely; just keep the wrapper accessor
signatures, `Display` text, and `Error::source` chain stable across the swap.

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
