# jmap-graph-server

> **Draft, not published.** Tracks draft-atwood-jmap-graph-00, which is
> still changing; `publish = false` until Mark reviews it (JMAP-ef3q).

Method handlers for draft-atwood-jmap-graph-00, the
`urn:ietf:params:jmap:graph` capability: the standard RFC 8620 `/get`,
`/changes`, `/set`, `/query` and `/queryChanges` of Graph, GraphNode and
GraphEdge, a `GraphBackend` trait, and an in-memory reference backend
behind the `memory` feature. No HTTP, no persistence.

## What it is

| Item | Spec |
|---|---|
| `register_graph_handlers` | §Methods: the 15 method names |
| `handle_graph_*` | §Graph Methods |
| `handle_graph_node_*` | §GraphNode Methods |
| `handle_graph_edge_*` | §GraphEdge Methods |
| `GraphBackend` | storage, `graph_capability`, atomic rights re-checks and destroy cascades |
| `query::*` | §Graph/query, §GraphNode/query, §GraphEdge/query match and order functions |
| `memory::MemoryBackend` (feature `memory`) | reference backend, not production |

## What it's for

It gives a JMAP server the Graph rules of the draft on top of
`jmap-server`'s dispatcher. The crate depends on RFC 8620 crates and
`jmap-graph-types` only. Sharing (`shareWith`, Principals,
ShareNotifications) is RFC 9670 and lives in `jmap-sharing-server`.

## How to use

```rust,no_run
use std::sync::Arc;
use jmap_graph_server::{register_graph_handlers, GraphBackend};
use jmap_server::Dispatcher;

fn wire<B: GraphBackend<CallerCtx = ()> + 'static>(backend: B) -> Dispatcher<()> {
    let mut dispatcher = Dispatcher::new();
    register_graph_handlers(&mut dispatcher, Arc::new(backend));
    dispatcher
}
```

Advertise `JMAP_GRAPH_URI` in the Session with the same account-level
object `GraphBackend::graph_capability` returns.

## How it works

- Rights come from the caller's `myRights` on the Graph, which the
  backend's `/get` fills per caller. `Graph/set` needs `mayRead` for the
  caller's own `isSubscribed`, `mayWrite` for `name` and `annotations`,
  `mayAdmin` for `shareWith` and destroy; GraphNode and GraphEdge `/set`
  need `mayWrite`. A missing right is `forbidden`.
- A Graph the caller may not read is hidden with its nodes and edges:
  `notFound` on `/get` and on `/set` update or destroy, `invalidProperties`
  naming `graphId` on create, absent from `/query`.
- Edge `fromId` and `toId` must name nodes of the edge's Graph, on create
  and on an update that changes them.
- Destroying a Graph destroys its nodes and edges; destroying a node its
  edges. The backend does both atomically.
- `annotations` keys are non-empty; `maxAnnotationSize` gives `tooLarge`;
  a node's `uri` must be an RFC 3986 URI.
- GraphNode and GraphEdge `/query` need a single condition with `graphId`
  (`unsupportedFilter` otherwise); unknown sort properties are
  `unsupportedSort`.
- `createdBy` and `updatedBy` are `JmapBackend::principal_id`.

## Gotchas

- `#creationId` references across method calls (the draft's
  `"graphId": "#g"` in a later `GraphNode/set`) need dispatcher support
  that `jmap-server` does not have yet; until then each id is taken
  literally.
- The `uri` check is the scheme and the RFC 3986 character set, not the
  full hier-part grammar.
- `maxAnnotationSize` counts the compact JSON serialization.
- `MemoryBackend` lets only the account owner create Graphs, keeps no
  Principals, and creates no ShareNotifications. `/queryChanges` there is
  `cannotCalculateChanges`.

## References

- [draft-atwood-jmap-graph-00] — JMAP for Graphs
- [RFC 8620] — JMAP Core
- [RFC 9670] — JMAP Sharing
- [RFC 3986] — URI Generic Syntax

[draft-atwood-jmap-graph-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 9670]: https://www.rfc-editor.org/rfc/rfc9670
[RFC 3986]: https://www.rfc-editor.org/rfc/rfc3986
