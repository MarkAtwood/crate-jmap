//! [`jmap_types::JmapObject`] impls for the Graph data types.
//!
//! Defined here so that `jmap-graph-server` can use them without violating
//! the orphan rule. The type names are those of draft-atwood-jmap-graph-00
//! §JMAP Data Type Registration and draft-atwood-jmap-graph-edit-00 §JMAP
//! Data Type Registration.

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

use crate::edit::{GraphNodeOpComparator, GraphNodeOpFilterCondition};
use crate::query::{
    GraphComparator, GraphEdgeComparator, GraphEdgeFilterCondition, GraphFilter,
    GraphNodeComparator, GraphNodeFilterCondition,
};
use crate::{Graph, GraphEdge, GraphNode, GraphNodeOp};

impl JmapObject for Graph {
    const TYPE_NAME: &'static str = "Graph";
    type Property = ();
}

impl GetObject for Graph {}

/// draft-atwood-jmap-graph-00 §Graph (lines 138-159): `id` is "immutable,
/// server-set"; `created`, `updated` and `myRights` are "server-set".
impl SetObject for Graph {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "created", "updated", "myRights"];
    const CLIENT_SETTABLE: &'static [&'static str] =
        &["name", "annotations", "isSubscribed", "shareWith"];
    const IMMUTABLE: &'static [&'static str] = &[];
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

/// draft-atwood-jmap-graph-00 §GraphNode (lines 179-206): `id` is
/// "immutable, server-set"; `created`, `updated`, `createdBy` and
/// `updatedBy` are "server-set"; `graphId` is "immutable".
impl SetObject for GraphNode {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] =
        &["id", "created", "updated", "createdBy", "updatedBy"];
    const CLIENT_SETTABLE: &'static [&'static str] =
        &["graphId", "annotations", "blobId", "mediaType", "uri"];
    const IMMUTABLE: &'static [&'static str] = &["graphId"];
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

/// draft-atwood-jmap-graph-00 §GraphEdge (lines 213-232): `id` is
/// "immutable, server-set"; `graphId` is "immutable"; `created`,
/// `updated`, `createdBy` and `updatedBy` are "As defined for GraphNode",
/// "server-set".
impl SetObject for GraphEdge {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] =
        &["id", "created", "updated", "createdBy", "updatedBy"];
    const CLIENT_SETTABLE: &'static [&'static str] =
        &["graphId", "fromId", "toId", "type", "annotations"];
    const IMMUTABLE: &'static [&'static str] = &["graphId"];
}

impl QueryObject for GraphEdge {
    type Filter = GraphEdgeFilterCondition;
    type Comparator = GraphEdgeComparator;
}

impl JmapObject for GraphNodeOp {
    const TYPE_NAME: &'static str = "GraphNodeOp";
    type Property = ();
}

impl GetObject for GraphNodeOp {}

/// draft-atwood-jmap-graph-edit-00 §The GraphNodeOp Data Type (lines
/// 172-193): `id`, `graphId`, `seq`, `created` and `createdBy` are
/// "immutable, server-set"; `nodeId`, `baseSeq` and `data` are
/// "immutable".
impl SetObject for GraphNodeOp {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "graphId", "seq", "created", "createdBy"];
    const CLIENT_SETTABLE: &'static [&'static str] = &["nodeId", "baseSeq", "data"];
    const IMMUTABLE: &'static [&'static str] = &["nodeId", "baseSeq", "data"];
}

impl QueryObject for GraphNodeOp {
    type Filter = GraphNodeOpFilterCondition;
    type Comparator = GraphNodeOpComparator;
}

#[cfg(test)]
mod tests {
    //! Each test builds a fully populated object with a struct literal, so a
    //! new field is a compile error here until it is set, then a
    //! classification failure until the `SetObject` lists name it.

    use super::*;
    use jmap_types::{check_property_classification, Id, UTCDate};
    use serde_json::{json, Map};
    use std::collections::BTreeMap;

    fn date() -> UTCDate {
        UTCDate::from("2026-01-01T00:00:00Z")
    }

    fn annotations() -> Map<String, serde_json::Value> {
        json!({"k": "v"}).as_object().unwrap().clone()
    }

    #[test]
    fn graph_properties_are_classified() {
        let rights = || -> crate::GraphRights {
            serde_json::from_value(json!({"mayRead": true, "mayWrite": true, "mayAdmin": true}))
                .unwrap()
        };
        let obj = Graph {
            id: Id::from("g1"),
            name: "n".into(),
            annotations: annotations(),
            created: date(),
            updated: date(),
            is_subscribed: true,
            my_rights: rights(),
            share_with: Some(BTreeMap::from([(Id::from("p1"), rights())])),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn graph_node_properties_are_classified() {
        let obj = GraphNode {
            id: Id::from("n1"),
            graph_id: Id::from("g1"),
            annotations: annotations(),
            blob_id: Some(Id::from("b1")),
            media_type: Some("text/plain".into()),
            uri: Some("https://example.com/".into()),
            created: date(),
            updated: date(),
            created_by: Some(Id::from("u1")),
            updated_by: Some(Id::from("u1")),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn graph_edge_properties_are_classified() {
        let obj = GraphEdge {
            id: Id::from("e1"),
            graph_id: Id::from("g1"),
            from_id: Id::from("n1"),
            to_id: Id::from("n2"),
            edge_type: "t".into(),
            annotations: annotations(),
            created: date(),
            updated: date(),
            created_by: Some(Id::from("u1")),
            updated_by: Some(Id::from("u1")),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn graph_node_op_properties_are_classified() {
        let obj = GraphNodeOp {
            id: Id::from("o1"),
            graph_id: Id::from("g1"),
            node_id: Id::from("n1"),
            seq: 1,
            base_seq: 0,
            data: "AAEC".into(),
            created: date(),
            created_by: Some(Id::from("u1")),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }
}
