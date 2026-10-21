//! `Graph/query`, `GraphNode/query` and `GraphEdge/query` filter and
//! comparator types (draft-atwood-jmap-graph-00 §Graph/query and
//! §Graph/queryChanges, §GraphNode/query and §GraphNode/queryChanges,
//! §GraphEdge/query and §GraphEdge/queryChanges; RFC 8620 §5.5).
//!
//! # Excluded from extras preservation
//!
//! Like every filter and comparator type in the workspace, these carry no
//! flatten-extras `extra` field and no `Other(String)` sort property: a
//! server that silently drops a filter clause or sort key it does not
//! understand returns the wrong result set. The conditions carry
//! `#[serde(deny_unknown_fields)]`, so an unknown clause fails the decode
//! and servers map that to `unsupportedFilter`. See the workspace
//! AGENTS.md "Filter algebra and control enums are explicitly EXCLUDED".
//!
//! # GraphNode and GraphEdge filters are single conditions
//!
//! "The `filter` argument MUST be a single FilterCondition, not a
//! FilterOperator, and MUST include `graphId`. A server receiving any other
//! filter, or no filter, rejects the call with an `unsupportedFilter`
//! error." The filter types of those two objects are therefore the bare
//! condition structs, with a required `graph_id`: a FilterOperator or a
//! condition without `graphId` fails the decode, which is
//! `unsupportedFilter`. Rejecting an absent `filter` is the server's job.
//! `Graph/query` takes the ordinary [`Filter`] tree.
//!
//! # Construction from outside this crate
//!
//! ```rust
//! use jmap_graph_types::query::GraphEdgeFilterCondition;
//! use jmap_types::Id;
//!
//! let mut cond = GraphEdgeFilterCondition::new(Id::from("G1"));
//! cond.from_id = Some(vec![Id::from("N1")]);
//! ```

use jmap_types::Id;
use serde::{Deserialize, Serialize};

pub use jmap_types::query::{Filter, FilterOperator, Operator};

fn default_true() -> bool {
    true
}

/// Filter for `Graph/query`.
pub type GraphFilter = Filter<GraphFilterCondition>;

/// Filter condition for `Graph/query` (draft-atwood-jmap-graph-00
/// §Graph/query and §Graph/queryChanges).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphFilterCondition {
    /// "The Graph's `isSubscribed` property equals this value."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_subscribed: Option<bool>,
}

/// Sort property for `Graph/query`: "The following sort properties are
/// supported: `name`, `created`, `updated`."
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GraphSortProperty {
    /// Sort by `name`.
    Name,
    /// Sort by `created`.
    Created,
    /// Sort by `updated`.
    Updated,
}

/// RFC 8620 §5.5 `Comparator` for `Graph/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphComparator {
    /// The property to sort by.
    pub property: GraphSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl GraphComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: GraphSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

/// Filter condition for `GraphNode/query`, which is also its whole filter
/// (draft-atwood-jmap-graph-00 §GraphNode/query and
/// §GraphNode/queryChanges; see the [module docs](self)).
///
/// `object_uri` is the draft-atwood-jmap-graph-projection-00 §GraphNode/query
/// Filter addition. A request that sets it uses
/// [`JMAP_GRAPH_PROJECTION_URI`](crate::JMAP_GRAPH_PROJECTION_URI).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNodeFilterCondition {
    /// "The node is in this Graph." Required.
    pub graph_id: Id,
    /// "The node's `objectUri` is one of these values"
    /// (draft-atwood-jmap-graph-projection-00). Values are JMAP object URIs
    /// such as `jmap:email:A1:M42`, compared as strings; they parse with
    /// [`jmap_types::JmapUri`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_uri: Option<Vec<String>>,
}

impl GraphNodeFilterCondition {
    /// The condition selecting the nodes of `graph_id`.
    pub fn new(graph_id: Id) -> Self {
        Self {
            graph_id,
            object_uri: None,
        }
    }
}

/// Sort property for `GraphNode/query`: "The following sort properties are
/// supported: `created`, `updated`."
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GraphNodeSortProperty {
    /// Sort by `created`.
    Created,
    /// Sort by `updated`.
    Updated,
}

/// RFC 8620 §5.5 `Comparator` for `GraphNode/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNodeComparator {
    /// The property to sort by.
    pub property: GraphNodeSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl GraphNodeComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: GraphNodeSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

/// Filter condition for `GraphEdge/query`, which is also its whole filter
/// (draft-atwood-jmap-graph-00 §GraphEdge/query and
/// §GraphEdge/queryChanges; see the [module docs](self)).
///
/// "All properties given must match."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphEdgeFilterCondition {
    /// "The edge is in this Graph." Required.
    pub graph_id: Id,
    /// "The edge's `fromId` is one of these ids."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_id: Option<Vec<Id>>,
    /// "The edge's `toId` is one of these ids."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_id: Option<Vec<Id>>,
    /// "The edge's `type` equals this value exactly."
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<String>,
}

impl GraphEdgeFilterCondition {
    /// The condition selecting every edge of `graph_id`.
    pub fn new(graph_id: Id) -> Self {
        Self {
            graph_id,
            from_id: None,
            to_id: None,
            edge_type: None,
        }
    }
}

/// Sort property for `GraphEdge/query`: "The following sort properties are
/// supported: `created`, `updated`."
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GraphEdgeSortProperty {
    /// Sort by `created`.
    Created,
    /// Sort by `updated`.
    Updated,
}

/// RFC 8620 §5.5 `Comparator` for `GraphEdge/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphEdgeComparator {
    /// The property to sort by.
    pub property: GraphEdgeSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl GraphEdgeComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: GraphEdgeSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}
