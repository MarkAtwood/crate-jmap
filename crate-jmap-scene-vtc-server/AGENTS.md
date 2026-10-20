# Agent Instructions — jmap-scene-vtc-server

## What this crate is

The VTC binding for `jmap-scene-server` (Mark, 2026-10-20, bd:JMAP-38uq.9):
cross-layer rules of draft-atwood-jmap-scene-00 and draft-atwood-jmap-vtc-00
as free functions a deployment calls from its `SceneBackend` and
`VtcBackend` impls. No handlers, no backend trait, no memory backend.

## Before starting any work

1. Run `bd ready`.
2. Read the cited sections of `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-scene-00.md`
   and `draft-atwood-jmap-vtc-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-scene-vtc-server --all-targets --all-features -- -D warnings
cargo test -p jmap-scene-vtc-server
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p jmap-scene-vtc-server
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Shape | Free functions, each taking only the backend it touches |
| Base crates | `jmap-scene-server` and `jmap-vtc-server` stay independent of each other |
| Tests | Each assertion quotes its draft line; the two reference `MemoryBackend`s wired in `tests/common` |
