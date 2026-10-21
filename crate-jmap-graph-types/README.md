# jmap-graph-types

> **Draft, not published.** Tracks draft-atwood-jmap-graph-00, which is
> still changing; `publish = false` until Mark reviews it (JMAP-ef3q).

Wire-format types for draft-atwood-jmap-graph-00, the
`urn:ietf:params:jmap:graph` capability for storing and synchronizing
general-purpose directed graphs over JMAP. Types only: no method handlers,
no I/O.

## What it is

| Item | Spec |
|---|---|
| `JMAP_GRAPH_URI` | §Capability |
| `GraphCapability` | §Account-Level Capability Object |
| `PrincipalGraphCapability` | §Principal Capability Object |
| `Graph`, `GraphRights` | §Graph |
| `GraphNode` | §GraphNode |
| `GraphEdge` | §GraphEdge |
| `query::*` filters and comparators | §Graph/query, §GraphNode/query, §GraphEdge/query |
| `JMAP_URI_GRAPH_TYPE`, `JMAP_URI_GRAPH_NODE_TYPE`, `JMAP_URI_GRAPH_EDGE_TYPE` | §JMAP Object URI Type Registrations |
| `edit::*` — `JMAP_GRAPH_EDIT_URI`, `GraphEditCapability`, `GraphNodeOp`, `GraphNode::edit_format`/`snapshot_seq`, `OPS_COMPACTED`, `GraphNode/cursor`, the WebSocket stream frames | draft-atwood-jmap-graph-edit-00 |
| `search::*` — `JMAP_GRAPH_SEARCH_URI`, `GraphSearchCapability`, the filter `where`/`language`/`expression` fields, the built-in filter language `Expression`, `QUERY_TOO_COMPLEX` | draft-atwood-jmap-graph-search-00 |

The session-level capability value is `{}` and has no type. Every Graph
method is a standard RFC 8620 method, so requests and responses use the
generic `jmap-types` shapes; the `JmapObject` impls tie them to these
objects, filters and comparators.

## What it's for

It gives the planned `jmap-graph-server` and `jmap-graph-client` one
definition of the Graph wire types. It depends on `jmap-types` only.

## How to use

```rust
use jmap_graph_types::query::GraphEdgeFilterCondition;

let f: GraphEdgeFilterCondition =
    serde_json::from_str(r#"{ "graphId": "G1", "fromId": ["N1"] }"#)?;
assert_eq!(f.from_id.as_deref().map(<[_]>::len), Some(1));
# Ok::<(), serde_json::Error>(())
```

## How it works

- Properties with a draft default (`name`, `annotations`, `isSubscribed`,
  `shareWith`, `blobId`, `mediaType`, `uri`, `type`) take it when absent.
- `T|null` properties are `Option<T>` and serialize `None` as `null`.
- `annotations` is a `serde_json::Map`: keys are application-chosen,
  values arbitrary JSON.
- `#[non_exhaustive]` on every public struct and enum and an `extra` map
  for vendor fields, per workspace policy. Filters and comparators reject
  unknown keys instead.
- No async. `#[forbid(unsafe_code)]` at the crate root.

## Gotchas

- The GraphNode and GraphEdge filter types are bare conditions with a
  required `graph_id`: the draft allows neither a FilterOperator nor a
  filter without `graphId`, and a decode failure is `unsupportedFilter`.
  Rejecting a missing `filter` is the server's job.
- The edge `type` property is the Rust field `edge_type`.
- The graph-edit GraphNode properties `editFormat` and `snapshotSeq` are
  not fields: they stay in `GraphNode::extra` and are read with
  `edit_format()` and `snapshot_seq()`, so nodes of a server without
  `urn:ietf:params:jmap:graph:edit` do not grow them.
- The graph-search filter `where` is raw JSON (`where_`): parse it with
  `search::Expression::parse`, giving the candidate kind, and map its
  `WhereError` into a `JmapError`. Depth against `maxDepth`, `$text`,
  `$collation` and `language` checks, and evaluation, are the server's.
- Op and cursor `data` are the base64url `String`; decoding, size limits
  and the `baseSeq`/`snapshotSeq` rules are server checks.
- Not enforced on deserialize: non-empty annotation keys,
  `maxAnnotationSize`, `uri` being a URI, and `fromId`/`toId` naming nodes
  of the edge's Graph. Those are server checks.
- `uri`, `mediaType`, edge types and annotations are user-supplied and
  untrusted (draft §Security Considerations).

## References

- [draft-atwood-jmap-graph-00] — JMAP for Graphs
- [draft-atwood-jmap-graph-edit-00] — JMAP Graph Live Editing
- [draft-atwood-jmap-graph-search-00] — JMAP Graph Search
- [RFC 8620] — JMAP Core
- [RFC 6901] — JSON Pointer
- [RFC 9670] — JMAP Sharing

[draft-atwood-jmap-graph-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-graph-edit-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-graph-search-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 6901]: https://www.rfc-editor.org/rfc/rfc6901
[RFC 9670]: https://www.rfc-editor.org/rfc/rfc9670
