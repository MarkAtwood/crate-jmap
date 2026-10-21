# jmap-graph-types — Plan

Typed surface of draft-atwood-jmap-graph-00 (epic bd:JMAP-ef3q). Types
only: the handlers live in the planned `jmap-graph-server`, the method
bindings in `jmap-graph-client`.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-ef3q.1 | Capability, Principal capability, Graph, GraphRights, GraphNode, GraphEdge, `/query` filters and comparators, `JmapObject` impls, JMAP Object URI type names | landed |
| JMAP-ef3q.3 | Projection `objectUri` GraphNode filter, `JMAP_GRAPH_PROJECTION_URI` | landed |

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
