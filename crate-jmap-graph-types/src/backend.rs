//! [`jmap_types::JmapObject`] impls for the Graph data types.
//!
//! Defined here so that `jmap-graph-server` can use them without violating
//! the orphan rule. The type names are those of draft-atwood-jmap-graph-00
//! §JMAP Data Type Registration.

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

use crate::query::{
    GraphComparator, GraphEdgeComparator, GraphEdgeFilterCondition, GraphFilter,
    GraphNodeComparator, GraphNodeFilterCondition,
};
use crate::{Graph, GraphEdge, GraphNode};

impl JmapObject for Graph {
    const TYPE_NAME: &'static str = "Graph";
    type Property = ();
}

impl GetObject for Graph {}

impl SetObject for Graph {
    type Patch = PatchObject;
}

impl QueryObject for Graph {
    type Filter = GraphFilter;
    type Comparator = GraphComparator;
}

impl JmapObject for GraphNode {
    const TYPE_NAME: &'static str = "GraphNode";
    type Property = ();
}

impl GetObject for GraphNode {}

impl SetObject for GraphNode {
    type Patch = PatchObject;
}

impl QueryObject for GraphNode {
    type Filter = GraphNodeFilterCondition;
    type Comparator = GraphNodeComparator;
}

impl JmapObject for GraphEdge {
    const TYPE_NAME: &'static str = "GraphEdge";
    type Property = ();
}

impl GetObject for GraphEdge {}

impl SetObject for GraphEdge {
    type Patch = PatchObject;
}

impl QueryObject for GraphEdge {
    type Filter = GraphEdgeFilterCondition;
    type Comparator = GraphEdgeComparator;
}
