# Agent Instructions — jmap-scene-server

## Sibling of the canonical jmap-mail-server — mirror its shape

Module layout, backend-trait shape, handler registration, `/set`
boilerplate and test layout mirror `jmap-mail-server` (via its sibling
`jmap-vtc-server`); the differences are the draft-atwood-jmap-scene-00
rules. Diverging shape changes go to `jmap-mail-server` first (workspace
AGENTS.md "Canonical Templates").

## What this crate is

draft-atwood-jmap-scene-00 `SceneAsset/*` method handlers, the
`SceneBackend` trait and a `memory` reference backend. Depends on RFC 8620
crates and `jmap-scene-types` only: Chat and VTC bindings live in binding
crates.

## Before starting any work

1. Run `bd ready`.
2. Read `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-scene-00.md` for the
   method you are changing.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-scene-server --all-targets --all-features -- -D warnings
cargo test -p jmap-scene-server
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p jmap-scene-server
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Permission hooks | Fail-open defaults (bd:JMAP-bghk) |
| glTF | `model/gltf-binary` always accepted (mandatory to implement) |
| "Recognized audio/image type" | Any `audio/*` or `image/*` media type, no registry |
| Account storage quota, admin-only create | `SceneBackend::create_object`, atomically |
| Tests | Each assertion quotes its draft line; never the code under test |
