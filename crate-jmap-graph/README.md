# jmap-graph

JMAP Graph ([draft-atwood-jmap-graph-00]) in one dependency: re-exports
[`jmap-graph-types`] always, [`jmap-graph-client`] with the `client` feature
and [`jmap-graph-server`] with the `server` feature (`memory` adds the
server's in-memory reference backend).

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_graph::types` |
| `client` | `jmap_graph::client` |
| `server` | `jmap_graph::server` |
| `memory` | `server`, plus `jmap_graph::server::memory` |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapGraphExt` extension trait over
  `jmap-base-client`, and Graph edit WebSocket frames.
- `server`: `Graph`, `GraphNode`, `GraphEdge` and `GraphNodeOp` handlers
  over `GraphBackend`, `register_graph_handlers`, and the `search` and
  `provenance` modules.

## What it's for

Graph stores typed nodes and edges as JMAP objects (`Graph`, `GraphNode`,
`GraphEdge`), with live editing, search and provenance extensions.
Applications that want the whole family depend on this crate and pick
`client`, `server` or both; workspace crates depend on the component crates
directly. `jmap-graph-projection-server`, which projects Mail and Contacts
into a graph, is not re-exported; depend on it directly.

## How to use

```toml
[dependencies]
jmap-graph = { version = "0.6", features = ["client"] }
```

```rust
use jmap_graph::types::{GraphEdgeFilterCondition, JMAP_GRAPH_URI};

assert_eq!(JMAP_GRAPH_URI, "urn:ietf:params:jmap:graph");
let f: GraphEdgeFilterCondition =
    serde_json::from_str(r#"{"graphId": "G1", "fromId": ["N1"]}"#).unwrap();
assert_eq!(f.from_id.as_deref().map(<[_]>::len), Some(1));
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_graph::server::X` is the same item as `jmap_graph_server::X`. The
component crates version in lockstep with this one on the shared `jmap-*`
line.

## Gotchas

- `server` alone does not build the reference backend; enable `memory` for
  `server::memory::MemoryBackend` (demonstration quality, not production).
- Depending on this crate and on a component crate at a different version
  gives two copies of each type, which do not unify. Keep them on one
  version line.
- Version 0.6.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-atwood-jmap-graph-00] — JMAP Graph
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-graph-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-graph-types`]: https://crates.io/crates/jmap-graph-types
[`jmap-graph-client`]: https://crates.io/crates/jmap-graph-client
[`jmap-graph-server`]: https://crates.io/crates/jmap-graph-server
