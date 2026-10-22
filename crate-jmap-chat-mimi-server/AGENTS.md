# Agent Instructions — jmap-chat-mimi-server

## What this crate is

draft-atwood-jmap-chat-mimi-00 server rules: `KeyPackage/get`,
`KeyPackage/set` and `Room/create` handlers over the `MimiBackend` trait,
with KeyPackages and GroupInfos parsed and verified by openmls, and the
room-bound Chat restrictions for `jmap-chat-server` hooks. No transport, no persistence, no MLS
secrets.

## Before starting any work

1. Read `PLAN.md`.
2. Run `bd ready`.
3. Read `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-mimi-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-chat-mimi-server --all-targets --all-features -- -D warnings
cargo test -p jmap-chat-mimi-server
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p jmap-chat-mimi-server
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| MLS library | crates.io `openmls =0.9.2` (`extensions-draft`) and `tls_codec =0.5.0`; never a git fork (bd:JMAP-6k5d.1) |
| Crypto provider | supplied by the backend (`MimiBackend::Crypto`); `openmls_rust_crypto` is a dev-dependency only |
| Account-wide rules | in `MimiBackend::create_key_package`, atomic with the insert |
| KeyPackage hand-out | in `MimiBackend::claim_key_packages`, atomic; compatibility via `KeyMaterialRequirements::fit` |
| Room/create | stateless checks in `check_room_create`; binding and room/group ID uniqueness in `MimiBackend::create_room`, atomic with the bound Chat |
| Room-bound Chats | `may_*_room_*` functions over `Chat.extra`, returned from `ChatBackend` hooks; no `jmap-chat-server` dependency |
| MIMI components | local tls_codec structs from draft-ietf-mimi-protocol-06 / room-policy-04; never mimid's crates |
| Byte strings | stored verbatim; KeyPackageRef hashed over the received octets |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
