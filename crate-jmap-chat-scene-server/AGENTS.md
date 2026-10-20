# Agent Instructions — jmap-chat-scene-server

## What this crate is

The Chat binding for `jmap-scene-server` (Mark, 2026-10-20, bd:JMAP-z996.4):
cross-layer rules of the Scene drafts and the Chat drafts as free
functions a deployment calls from its `SceneBackend` impl or WebSocket
transport. No handlers, no backend trait, no memory backend.

## Before starting any work

1. Run `bd ready`.
2. Read the cited sections of `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-scene-wss-00.md`
   and `draft-atwood-jmap-chat-wss-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-chat-scene-server --all-targets --all-features -- -D warnings
cargo test -p jmap-chat-scene-server
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p jmap-chat-scene-server
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Shape | Free functions, each taking only the backend it touches |
| Base crates | `jmap-scene-server` and `jmap-chat-server` stay independent of each other |
| Tests | Each assertion quotes its draft line; the reference Scene `MemoryBackend` |
