# Agent Instructions — jmap-chat-mimi-types

## What this crate is

draft-atwood-jmap-chat-mimi-00 wire-format types: the
`urn:ietf:params:jmap:chat:mimi` capability and its methods' request and
response shapes, plus the pure converting-party functions in `convert`
(draft §Message Identifiers through §Message Status). No method semantics,
no async, no I/O, no stored state.

## Before starting any work

1. Read `PLAN.md`.
2. Run `bd ready`.
3. Read `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-mimi-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-chat-mimi-types --all-targets -- -D warnings
cargo test -p jmap-chat-mimi-types
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p jmap-chat-mimi-types
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Async | None |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types, serde, serde_json — no MLS, TLS-codec or base64 crate |
| Byte strings | `Base64Url`, verbatim, never re-encoded |
| Wire format | camelCase JSON |
| All public structs | `#[non_exhaustive]` with `extra` |
