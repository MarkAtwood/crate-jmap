# jmap-graph-types — Plan

Typed surface of draft-atwood-jmap-graph-00 (epic bd:JMAP-ef3q). Types
only: the handlers live in the planned `jmap-graph-server`, the method
bindings in `jmap-graph-client`.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-ef3q.1 | Capability, Principal capability, Graph, GraphRights, GraphNode, GraphEdge, `/query` filters and comparators, `JmapObject` impls, JMAP Object URI type names | landed |
| JMAP-ef3q.3 | Projection `objectUri` GraphNode filter, `JMAP_GRAPH_PROJECTION_URI` | landed |
| JMAP-ef3q.4 | `edit` module (draft-atwood-jmap-graph-edit-00): capability, GraphNodeOp with filter/comparator, GraphNode `editFormat`/`snapshotSeq` accessors, `opsCompacted`, `GraphNode/cursor`, stream frames and events | landed |
| JMAP-ef3q.7 | `search` module (draft-atwood-jmap-graph-search-00): capability, filter `where`/`language`/`expression`, built-in filter language AST with well-formedness parse and traversal depth, `queryTooComplex` | landed |

## Decisions

- The session-level capability value `{}` is not modelled, as in
  `jmap-scene-types`.
- No method argument types: every Graph method is a standard RFC 8620
  method, covered by the generic `jmap-types` shapes.
- The GraphNode and GraphEdge `QueryObject::Filter` is the bare condition
  struct with a required `graph_id`, not `Filter<_>`: the draft (L285,
  L314) rejects a FilterOperator and a filter without `graphId` with
  `unsupportedFilter`, which is what a filter decode failure maps to.
- `GraphNodeFilterCondition::object_uri` is the
  draft-atwood-jmap-graph-projection-00 §GraphNode/query Filter addition
  (JMAP-ef3q.3), `Vec<String>` like `uri`. The GraphNode `objectUri`
  property itself is left to JMAP-ef3q.9; until then it lands in `extra`.
- `uri` is a plain `String`: the draft accepts any URI. JMAP Object URIs
  parse with `jmap_types::JmapUri`.
- `annotations` is `serde_json::Map<String, Value>` (sorted, as the
  workspace map rule requires); `shareWith` is `BTreeMap<Id, GraphRights>`.
- Server-set properties are required on deserialize, as in
  `jmap-scene-types`.
- graph-edit's GraphNode `editFormat` and `snapshotSeq` live in
  `GraphNode::extra` behind accessors (`edit_format`, `snapshot_seq`), as
  in `jmap-chat-types` `Space::calendar_id`: typed fields would serialize
  on every node, including those of servers without the edit capability.
- `GraphNodeOpFilterCondition` follows the GraphNode filter: the draft
  (L222) requires a single condition with `nodeId`.
- Op and cursor `data` stay base64url `String`s; the server never reads
  them (draft §Design Principles), so decoding belongs to clients.
- graph-search `where` is a raw `serde_json::Value` in the filter
  conditions, parsed by `search::Expression::parse(value, kind)`: the draft
  (L162) splits a malformed expression (`invalidArguments`) from an unknown
  `$` name (`unsupportedFilter`), and L242 makes `$edge`/`$from`
  well-formedness depend on the candidate kind; a serde decode can express
  neither. `Expression` mirrors the JSON object (pointer-keyed field map,
  one slot per operator), so it cannot serialize a duplicate member.
- Unknown members of an EdgeTest or ReachTest are malformed even when
  `$`-prefixed (L160 "contain only the members defined for them"); the
  `unsupportedFilter` rule (L162) covers expression members and operator
  objects.
- Checks needing the account (depth vs `maxDepth`, `supportsText`,
  `collationAlgorithms`, `languages`, the `language`/`expression` pairing)
  and evaluation are left to the server (JMAP-ef3q.8).
