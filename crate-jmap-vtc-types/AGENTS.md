# Agent Instructions — jmap-vtc-types

## What this crate is

draft-atwood-jmap-vtc-00 wire-format types: the `urn:ietf:params:jmap:vtc`
capability, the VTCCall, VTCParticipant, VTCRecording and VTCLivestream
data types, the VTCCallPush payload and the ephemeral VTC events. Pure data
types: no method semantics, no async, no I/O.

## Before starting any work

1. Read `PLAN.md`.
2. Run `bd ready`.
3. Read `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-vtc-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-vtc-types --all-targets -- -D warnings
cargo test -p jmap-vtc-types
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p jmap-vtc-types
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Async | None |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types, serde, serde_json — no Chat, Calendars or Scene types |
| Wire format | camelCase JSON |
| All public structs | `#[non_exhaustive]` with `extra` |
