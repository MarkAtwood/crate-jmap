# Agent Instructions — jmap-did-types

## What this crate is

draft-atwood-jmap-did-00 wire-format types for the `jmap-*` crate family:
the `urn:ietf:params:jmap:did` capability objects. Pure data types: no
JMAP-method semantics, no async, no network I/O. DID resolution I/O and
caching belong in `crate-jmap-did-resolver` (decided bd:JMAP-6itf.6), and
no `*-types` crate depends on it.

## Crate family context

```
jmap-types
    └── jmap-did-types  ← this crate
```

## Before starting any work

1. Read `PLAN.md` — scope and bead map (epic bd:JMAP-6itf)
2. Run `bd ready` — check for open issues before creating new ones
3. Read draft-atwood-jmap-did-00 before adding or changing types

## Source material

Normative reference: draft-atwood-jmap-did-00 at
`~/PROJECT/jmap-chat-spec/draft-atwood-jmap-did-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-did-types --all-targets -- -D warnings
cargo test -p jmap-did-types
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p jmap-did-types
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Async | None — this crate is sync only |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types (for `impl_string_enum!`), serde, serde_json |
| Wire format | camelCase JSON — `#[serde(rename_all = "camelCase")]` |
| All public structs and enums | `#[non_exhaustive]` |
