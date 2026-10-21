//! draft-atwood-jmap-graph-00 §Graph, including GraphRights.

use std::collections::BTreeMap;

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// The rights a user has on a [`Graph`] (draft-atwood-jmap-graph-00
/// §Graph, GraphRights). "The owner of the account that holds the Graph
/// implicitly has all rights."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRights {
    /// "The user may fetch this Graph and every GraphNode and GraphEdge in
    /// it."
    pub may_read: bool,
    /// "The user may create, update, and destroy GraphNode and GraphEdge
    /// objects in this Graph, and may change the Graph's `name` and
    /// `annotations`."
    pub may_write: bool,
    /// "The user may change the Graph's `shareWith` property and may
    /// destroy the Graph."
    pub may_admin: bool,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A container of nodes and edges, and the unit of sharing
/// (draft-atwood-jmap-graph-00 §Graph).
///
/// Properties with a draft default (`name`, `annotations`, `isSubscribed`,
/// `shareWith`) take it when absent. `shareWith` is `Id[GraphRights]|null`
/// and serializes `None` as `null`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    /// The id of the Graph. Immutable, server-set.
    pub id: Id,
    /// "A user-visible name for the Graph." Draft default `""`.
    #[serde(default)]
    pub name: String,
    /// "Application data about the Graph as a whole." Draft default `{}`.
    /// Keys are non-empty strings; values are arbitrary JSON.
    #[serde(default)]
    pub annotations: serde_json::Map<String, serde_json::Value>,
    /// When the Graph was created. Server-set.
    pub created: UTCDate,
    /// "When a property of the Graph object itself was last changed.
    /// Changes to the Graph's nodes and edges do not change this value."
    /// Server-set.
    pub updated: UTCDate,
    /// As defined in RFC 9670 §4. Draft default `true` for the user who
    /// creates the Graph.
    #[serde(default = "default_true")]
    pub is_subscribed: bool,
    /// The rights the user has on this Graph (RFC 9670 §4). Server-set.
    pub my_rights: GraphRights,
    /// Principal id to rights, as defined in RFC 9670 §4. Draft default
    /// `null`.
    #[serde(default)]
    pub share_with: Option<BTreeMap<Id, GraphRights>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_true() -> bool {
    true
}
