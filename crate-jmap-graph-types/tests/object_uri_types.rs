//! The Graph URI type-name constants agree with the "JMAP Object URI
//! Types" registry (draft-atwood-jmap-graph-00 §JMAP Object URI Type
//! Registrations), as collected in `jmap-object-uri`.

use jmap_graph_types::{JMAP_URI_GRAPH_EDGE_TYPE, JMAP_URI_GRAPH_NODE_TYPE, JMAP_URI_GRAPH_TYPE};
use jmap_object_uri::uri_type;

#[test]
fn constants_match_the_registry() {
    for (name, data_type) in [
        (JMAP_URI_GRAPH_TYPE, "Graph"),
        (JMAP_URI_GRAPH_NODE_TYPE, "GraphNode"),
        (JMAP_URI_GRAPH_EDGE_TYPE, "GraphEdge"),
    ] {
        assert_eq!(
            uri_type(name).map(|e| e.data_type),
            Some(data_type),
            "{name}"
        );
    }
}
