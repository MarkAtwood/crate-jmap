# Agent Instructions — jmap-graph-projection-server

## What this crate is

The Mail and Contacts binding for `jmap-graph-server` (bd:JMAP-ef3q.9):
draft-atwood-jmap-graph-projection-00 as free functions a deployment
calls from its `GraphBackend` impl and its Mail and Contacts write paths.
No handlers, no backend trait, no memory backend.

## Before starting any work

1. Run `bd ready`.
2. Read the cited sections of
   `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-graph-projection-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-graph-projection-server --all-targets --all-features -- -D warnings
cargo test -p jmap-graph-projection-server
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p jmap-graph-projection-server
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Shape | Free functions and the `Projectable` trait over the RFC 8621/9610 types |
| Base crates | Depends on `jmap-graph-types`, the mail/contacts types and `jmap-server`, not on `jmap-graph-server` or the mail/contacts servers |
| Writes | None accepted: `projected_graph_rights` has `mayWrite: false`, so graph-server answers `forbidden` (§Writes) |
| Visibility | Native `get_objects` with the caller's context (§Visibility) |
| Ids and timestamps | The deployment's (no persistence in the kit) |
| Tests | Each assertion quotes its draft or RFC line; hand-written JSON; mail `MemoryBackend` for visibility |
