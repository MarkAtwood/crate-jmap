# Agent Instructions — jmap-graph-types

## What this crate is

draft-atwood-jmap-graph-00 wire-format types: the
`urn:ietf:params:jmap:graph` capability, the Graph, GraphNode and
GraphEdge data types, and their `/query` filters and comparators; the
`edit` module adds draft-atwood-jmap-graph-edit-00
(`urn:ietf:params:jmap:graph:edit`). Pure data types: no method
semantics, no async, no I/O.

## Before starting any work

1. Read `PLAN.md`.
2. Run `bd ready`.
3. Read `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-graph-00.md` (and
   `draft-atwood-jmap-graph-edit-00.md` for `edit`).

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-graph-types --all-targets -- -D warnings
cargo test -p jmap-graph-types
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p jmap-graph-types
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Async | None |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types, serde, serde_json |
| Wire format | camelCase JSON |
| All public structs | `#[non_exhaustive]` with `extra`, except filters |
