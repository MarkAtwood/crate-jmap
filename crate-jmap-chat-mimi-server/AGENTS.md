# Agent Instructions — jmap-chat-mimi-server

## What this crate is

draft-atwood-jmap-chat-mimi-00 server rules: `KeyPackage/get` and
`KeyPackage/set` handlers over the `MimiBackend` trait, with KeyPackages
parsed and verified by openmls. No transport, no persistence, no MLS
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
| MLS library | crates.io `openmls =0.9.1` (`extensions-draft`) and `tls_codec =0.5.0`; never a git fork (bd:JMAP-6k5d.1) |
| Crypto provider | supplied by the backend (`MimiBackend::Crypto`); `openmls_rust_crypto` is a dev-dependency only |
| Account-wide rules | in `MimiBackend::create_key_package`, atomic with the insert |
| KeyPackage hand-out | in `MimiBackend::claim_key_packages`, atomic; compatibility via `KeyMaterialRequirements::fit` |
| Byte strings | stored verbatim; KeyPackageRef hashed over the received octets |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
