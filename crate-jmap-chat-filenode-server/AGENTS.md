# Agent Instructions — jmap-chat-filenode-server

## What this crate is

The Chat binding for `jmap-filenode-server` (Mark, 2026-10-20,
bd:JMAP-v9py.16.8): cross-layer rules of draft-atwood-jmap-chat-filenode-00
as free functions a deployment calls from its `FileNodeBackend` and
`ChatBackend` impls. No handlers, no backend trait, no memory backend.

## Before starting any work

1. Run `bd ready`.
2. Read the cited sections of
   `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-filenode-00.md` and
   `draft-atwood-jmap-chat-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-chat-filenode-server --all-targets --all-features -- -D warnings
cargo test -p jmap-chat-filenode-server
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p jmap-chat-filenode-server
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Shape | Free functions taking the backends they read |
| Base crates | `jmap-filenode-server` and `jmap-chat-server` stay independent of each other |
| `@everyone` permissions | Caller-supplied: the draft puts them on no wire object |
| Tests | Each assertion quotes its draft line; the two reference `MemoryBackend`s |
