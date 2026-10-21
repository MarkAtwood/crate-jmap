# Agent Instructions — jmap-chat-mimi-client

## What this crate is

draft-atwood-jmap-chat-mimi-00 client: the `Room/create`, `Room/update`,
`Room/send`, `Room/unbind` and `GroupInfo/fetch` bindings over a
`jmap-base-client` session, the §Chat.roomId last-client rule, and the
§GroupInfo/fetch checks on a GroupInfoResponse. No MLS state, no secrets.

## Before starting any work

1. Read `PLAN.md`.
2. Run `bd ready`.
3. Read `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-mimi-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-chat-mimi-client --all-targets --all-features -- -D warnings
cargo test -p jmap-chat-mimi-client
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p jmap-chat-mimi-client
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| MLS library | crates.io `openmls =0.9.1` (`extensions-draft`) and `tls_codec =0.5.0`; never a git fork (bd:JMAP-6k5d.1) |
| Crypto provider | supplied by the caller; `openmls_rust_crypto` is a dev-dependency only |
| Wire types | from `jmap-chat-mimi-types`; never redefined here |
| GroupInfoResponse signature | VerifyWithLabel over the received TBS octets, never a re-encoding |
| Test fixtures | `tests/fixtures/group_info_response.py` (pyca/cryptography), RFC 9420 crypto-basics vectors, the draft's §Examples JSON |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
