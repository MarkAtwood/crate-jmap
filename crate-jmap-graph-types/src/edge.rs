//! draft-atwood-jmap-graph-00 §GraphEdge.

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// A directed link from one node to another in the same Graph
/// (draft-atwood-jmap-graph-00 §GraphEdge).
///
/// "Any number of edges, of the same or different types, may connect the
/// same pair of nodes in either direction." Checking that `fromId` and
/// `toId` name nodes of the edge's Graph is a server check.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    /// The id of the edge. Immutable, server-set.
    pub id: Id,
    /// "The id of the Graph the edge belongs to." Immutable.
    pub graph_id: Id,
    /// "The id of the GraphNode the edge starts at."
    pub from_id: Id,
    /// "The id of the GraphNode the edge ends at. It may equal `fromId`."
    pub to_id: Id,
    /// "An application-defined edge type. The server attaches no meaning
    /// to it." Draft default `""`. Untrusted (draft §Untrusted Data).
    #[serde(default, rename = "type")]
    pub edge_type: String,
    /// "Application data about the edge." Draft default `{}`.
    #[serde(default)]
    pub annotations: serde_json::Map<String, serde_json::Value>,
    /// When the edge was created. Server-set.
    pub created: UTCDate,
    /// When any property of the edge was last changed. Server-set.
    pub updated: UTCDate,
    /// The Principal that created the edge, or `null`, as defined for
    /// [`GraphNode`](crate::GraphNode). Server-set.
    pub created_by: Option<Id>,
    /// The Principal that last changed the edge, or `null`. Server-set.
    pub updated_by: Option<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
