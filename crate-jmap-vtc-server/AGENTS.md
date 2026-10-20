# Agent Instructions — jmap-vtc-server

## Sibling of the canonical jmap-mail-server — mirror its shape

Module layout, backend-trait shape, handler registration, `/set`
boilerplate and test layout mirror `jmap-mail-server`; the differences are
the draft-atwood-jmap-vtc-00 rules. Diverging shape changes go to
`jmap-mail-server` first (workspace AGENTS.md "Canonical Templates").

## What this crate is

draft-atwood-jmap-vtc-00 `VTCCall/*`, `VTCRecording/*` and
`VTCLivestream/*` method handlers, the `VtcBackend`
trait and a `memory` reference backend. Depends on RFC 8620 crates and
`jmap-vtc-types` only: Chat, Calendars and Scene bindings live in binding
crates (Mark, 2026-10-20, bd:JMAP-8mem.6).

## Before starting any work

1. Run `bd ready`.
2. Read `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-vtc-00.md` for the
   method you are changing.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-vtc-server --all-targets --all-features -- -D warnings
cargo test -p jmap-vtc-server
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p jmap-vtc-server
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Permission hooks | Fail-open defaults (bd:JMAP-bghk) |
| One active call per Chat | Enforced atomically in `VtcBackend::create_call` |
| Recording/livestream on an E2EE call | Always `forbidden`: no key-holding bridge here (the draft's MAY) |
| Recording consent signal | `VtcBackend::deliver_recording_state_event`, no default |
| Tests | Each assertion quotes its draft line; never the code under test |
