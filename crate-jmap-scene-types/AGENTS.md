# Agent Instructions — jmap-scene-types

## What this crate is

draft-atwood-jmap-scene-00 wire-format types: the
`urn:ietf:params:jmap:scene` capability, the coordinate formats,
SceneBounds and GeoAnchor, the SceneAsset, SceneRegion, SceneObject and
SceneAvatar data types, and their `/query` filters and comparators. Pure
data types: no method semantics, no async, no I/O.

## Before starting any work

1. Read `PLAN.md`.
2. Run `bd ready`.
3. Read `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-scene-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-scene-types --all-targets -- -D warnings
cargo test -p jmap-scene-types
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p jmap-scene-types
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Async | None |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types, serde, serde_json — no Chat or VTC types |
| Wire format | camelCase JSON |
| All public structs | `#[non_exhaustive]` with `extra`, except filters |
