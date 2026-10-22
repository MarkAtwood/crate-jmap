# Agent Instructions — jmap-graph-server

## Sibling of the canonical jmap-mail-server — mirror its shape

Module layout, backend-trait shape, handler registration, `/set`
boilerplate and test layout mirror `jmap-mail-server` (via its sibling
`jmap-scene-server`); the differences are the draft-atwood-jmap-graph-00
rules. Diverging shape changes go to `jmap-mail-server` first (workspace
AGENTS.md "Canonical Templates").

## What this crate is

draft-atwood-jmap-graph-00 `Graph/*`, `GraphNode/*` and `GraphEdge/*`
method handlers, the draft-atwood-jmap-graph-edit-00 `GraphNodeOp/*` and
`GraphNode/cursor` handlers and WebSocket stream check, the `GraphBackend`
trait and a `memory` reference backend. Depends on RFC 8620 crates and `jmap-graph-types` only.

## Before starting any work

1. Run `bd ready`.
2. Read `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-graph-00.md` (or
   `draft-atwood-jmap-graph-edit-00.md` for live editing) for the method
   you are changing.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-graph-server --all-targets --all-features -- -D warnings
cargo test -p jmap-graph-server
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features -p jmap-graph-server
```

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Rights | from the per-caller `Graph.myRights` the backend's `/get` returns; no extra hook; backend re-verifies atomically |
| Unreadable Graph | `notFound` on `/get` and `/set` update/destroy, `invalidProperties` `graphId` on create, omitted from `/query` (backend) |
| `/set` loop | one `helpers::handle_set` over a per-type `SetOps` impl |
| `createdBy` / `updatedBy` | `JmapBackend::principal_id` |
| Graph `updated` | bumped by every Graph patch except one touching only the caller's `isSubscribed` |
| Destroy cascades | `GraphBackend::destroy_object`, atomically |
| Edge endpoints | handler check plus backend re-check, `invalidProperties` naming the property |
| `uri` | RFC 3986 scheme + character set |
| `maxAnnotationSize` | compact JSON length, `tooLarge` |
| Sharing | RFC 9670 via `jmap-sharing-server`; ShareNotification `name` is the Graph's `name` (backend duty) |
| Cross-call `#creationId` | not resolved: dispatcher gap, bd:JMAP-8ypy |
| Edit capability | `GraphBackend::graph_edit_capability`, default `None`; without it the edit handlers answer `accountNotSupportedByMethod` and `GraphNode/set` ignores the extensions |
| Op `seq` | assigned by the backend in `create_object`, atomically with the `baseSeq`/`editFormat`/`mayWrite` re-check; op events leave from that step |
| `opsCompacted` | `EditLog::compacted_through` read after the query; front-only compaction makes that sufficient |
| Compaction | backend policy, not a trait method; memory `compact_ops` |
| Stream | `GraphEditSubscription` per connection + `should_deliver`; transport is the consumer's (scene/chat pattern) |
| Cursor fan-out, block check | `publish_cursor` (no-op default), `is_sender_blocked(caller, sender)` (fail-open, bd:JMAP-bghk), asked only when `has_capability` reports Chat on the recipient's account |
| Tests | Each assertion quotes its draft line; never the code under test |
