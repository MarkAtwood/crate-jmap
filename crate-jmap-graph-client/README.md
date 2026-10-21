# jmap-graph-client

> **Draft, not published.** Tracks draft-atwood-jmap-graph-00, which is
> still changing; `publish = false` until Mark reviews it (JMAP-ef3q).

## What it is

Client bindings for the JMAP Graph extension (draft-atwood-jmap-graph-00).
`JmapGraphExt` binds the standard RFC 8620 `/get`, `/changes`, `/set`,
`/query` and `/queryChanges` of Graph, GraphNode and GraphEdge over
`jmap-base-client`, typed with the `jmap-graph-types` objects, filters and
comparators.

## What it's for

`with_graph_session` binds a `JmapClient` to a session; each method on the
returned `SessionClient` targets the primary `urn:ietf:params:jmap:graph`
account, or the account chosen with `for_account`. A shared Graph lives in
the account that created it, which other users reach as a shared account
(§Accounts).

GraphNode and GraphEdge `/query` take a required filter condition with a
`graphId`: the draft rejects any other filter with `unsupportedFilter`. A
GraphNode filter may also carry the draft-atwood-jmap-graph-projection-00
`objectUri` property; the request then adds
`urn:ietf:params:jmap:graph:projection` to `using`.

For draft-atwood-jmap-graph-edit-00 live editing, `SessionClient` also
binds `GraphNodeOp/get`, `/changes`, the create-only `/set`, `/query` (a
required `nodeId` condition with optional `afterSeq`) and `/queryChanges`,
and `GraphNode/cursor`; these requests add `urn:ietf:params:jmap:graph:edit`
to `using`. A `GraphNodeOp/query` that fails with `opsCompacted` arrives as
`ClientError::MethodError`. On a WebSocket session, `GraphEditWsExt` sends
`GraphEditStreamEnable` / `GraphEditStreamDisable` and returns
`GraphEditWsFrame::Op` and `::Cursor` for the op and cursor events. Send
the enable before loading or catching up on a node (§GraphEditStreamEnable).

## How to use

One traversal hop (§Traversal): the edges leaving a frontier, then those
edges.

```rust,no_run
use jmap_graph_client::JmapGraphExt;
use jmap_graph_types::query::GraphEdgeFilterCondition;
use jmap_types::Id;

# async fn run(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
let session = client.fetch_session().await?;
let gc = client.with_graph_session(session);
let mut cond = GraphEdgeFilterCondition::new(Id::from("G1"));
cond.from_id = Some(vec![Id::from("N1")]);
let edges = gc.graph_edge_query(&cond, None, None, None).await?;
let edges = gc.graph_edge_get(Some(&edges.ids), None).await?;
# Ok(()) }
```

Each method sends one method call. Chaining calls by result reference in
one request goes through `SessionClient::client`.
