# jmap-graph-server

Method handlers for draft-atwood-jmap-graph-00, the
`urn:ietf:params:jmap:graph` capability: the standard RFC 8620 `/get`,
`/changes`, `/set`, `/query` and `/queryChanges` of Graph, GraphNode and
GraphEdge, a `GraphBackend` trait, and an in-memory reference backend
behind the `memory` feature. It also serves draft-atwood-jmap-graph-edit-00
(`urn:ietf:params:jmap:graph:edit`): GraphNodeOp edit logs,
`GraphNode/cursor` and the WebSocket op and cursor stream, and
draft-atwood-jmap-graph-search-00 (`urn:ietf:params:jmap:graph:search`):
the built-in `where` filter language on GraphNode and GraphEdge `/query`.
No HTTP, no persistence, no WebSocket transport.

## What it is

| Item | Spec |
|---|---|
| `register_graph_handlers` | §Methods: the 15 method names |
| `handle_graph_*` | §Graph Methods |
| `handle_graph_node_*` | §GraphNode Methods |
| `handle_graph_edge_*` | §GraphEdge Methods |
| `GraphBackend` | storage, `graph_capability`, atomic rights re-checks and destroy cascades; edit hooks `graph_edit_capability`, `edit_log`, `projected_graph_id`, `publish_cursor`, `is_sender_blocked`; search hook `graph_search_capability` |
| `register_graph_edit_handlers` | graph-edit §Methods: GraphNodeOp `/get`, `/changes`, `/set`, `/query`, `/queryChanges`, `GraphNode/cursor` |
| `op::*` | graph-edit §GraphNodeOp/set, §GraphNodeOp/query, §GraphNode/cursor |
| `stream::GraphEditSubscription`, `should_deliver` | graph-edit §WebSocket Stream |
| `query::*` | §Graph/query, §GraphNode/query, §GraphEdge/query match and order functions |
| `search::check_search_filter`, `search::GraphView`, `COLLATIONS` | graph-search §FilterCondition Extensions, §The Built-In Filter Language, §Visibility |
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
object `GraphBackend::graph_capability` returns. For live editing, also
call `register_graph_edit_handlers` and advertise `JMAP_GRAPH_EDIT_URI`
with the object `GraphBackend::graph_edit_capability` returns. For
search, advertise `JMAP_GRAPH_SEARCH_URI` with the object
`GraphBackend::graph_search_capability` returns, and `search::COLLATIONS`
in the core `collationAlgorithms`.

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
- Live editing: `GraphNode/set` enforces the `editFormat` and
  `snapshotSeq` rules (set once; a snapshot changes `blobId` and
  `snapshotSeq` together; `snapshotSeq` never decreases and never passes
  the head; no `editFormat` on a projected Graph's nodes).
  `GraphNodeOp/set` is create-only; the backend assigns `seq`
  atomically. `GraphNodeOp/query` takes one `nodeId` condition with
  `afterSeq`, sorts by `seq` ascending, and answers `opsCompacted` when
  an op after `afterSeq` has been compacted. Compaction is backend policy:
  front-only, never past `snapshotSeq` or inside `opRetention`.
- Search: with `graph_search_capability`, `where`, `language` and
  `expression` are checked (`invalidArguments` when malformed,
  `unsupportedFilter` for an unknown operator, an unlisted language,
  `$text` without `supportsText` or a `$collation` outside `COLLATIONS`,
  `queryTooComplex` past `maxDepth`); `/query` says
  `canCalculateChanges: false` and `/queryChanges` is
  `cannotCalculateChanges`. The backend evaluates `where` with a
  `GraphView` of the Graph as the caller sees it, so hidden nodes and
  edges cannot affect results. Without the capability the three are
  `unsupportedFilter`.
- The WebSocket transport keeps one `GraphEditSubscription` per
  connection and asks `should_deliver` before sending each
  `GraphNodeOpEvent` and `GraphCursorEvent`; rights are checked at
  delivery, cursors skip the sender and recipients who blocked it.

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
  `cannotCalculateChanges`. It compacts only when `compact_ops` is called
  and records cursor events but not op events.
- Rate limits on ops and cursors (SHOULD rules) are the deployment's.
- No advertised search language (`gql`, `opencypher`, ...) is
  implemented; `MemoryBackend` answers an `expression` with
  `unsupportedFilter`. `GraphView` has no work budget beyond `maxDepth`,
  and its `$text` is a `i;unicode-casemap` substring match over
  string-valued annotations.

## References

- [draft-atwood-jmap-graph-00] — JMAP for Graphs
- [draft-atwood-jmap-graph-edit-00] — JMAP Graph Live Editing
- [draft-atwood-jmap-graph-search-00] — JMAP Graph Search
- [RFC 4790], [RFC 5051] — collations
- [RFC 8620] — JMAP Core
- [RFC 9670] — JMAP Sharing
- [RFC 3986] — URI Generic Syntax

[draft-atwood-jmap-graph-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-graph-edit-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-graph-search-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 4790]: https://www.rfc-editor.org/rfc/rfc4790
[RFC 5051]: https://www.rfc-editor.org/rfc/rfc5051
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 9670]: https://www.rfc-editor.org/rfc/rfc9670
[RFC 3986]: https://www.rfc-editor.org/rfc/rfc3986
