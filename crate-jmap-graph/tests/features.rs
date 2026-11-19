//! The umbrella's re-exports under each feature set.
//!
//! Oracles: draft-atwood-jmap-graph-00 (the capability URIs and a GraphEdge
//! filter condition) and the crate's documented feature table.

use jmap_graph::types::{GraphEdgeFilterCondition, JMAP_GRAPH_URI};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_GRAPH_URI, "urn:ietf:params:jmap:graph");
    let f: GraphEdgeFilterCondition =
        serde_json::from_str(r#"{"graphId": "G1", "fromId": ["N1"]}"#).unwrap();
    assert_eq!(f.from_id.as_deref().map(<[_]>::len), Some(1));
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-graph-client.
    fn _takes<T: jmap_graph::client::JmapGraphExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-graph-server.
    fn _takes<B: jmap_graph::server::GraphBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_graph::server::GraphBackend>(_: &B) {}
    implements(&jmap_graph::server::memory::MemoryBackend::new());
}
