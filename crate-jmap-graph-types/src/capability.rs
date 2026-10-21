//! draft-atwood-jmap-graph-00 §Capability — capability URI, the
//! account-level and Principal capability objects, and the JMAP Object URI
//! type names.

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// Capability URI for JMAP Graph (draft-atwood-jmap-graph-00 §Capability).
///
/// "Servers supporting this specification MUST advertise the
/// `urn:ietf:params:jmap:graph` capability in the JMAP Session object." The
/// session-level value is an empty object `{}` and is not modelled; the
/// account-level value is [`GraphCapability`].
pub const JMAP_GRAPH_URI: &str = "urn:ietf:params:jmap:graph";

/// Capability URI for the JMAP Graph projection
/// (draft-atwood-jmap-graph-projection-00 §Capability): "A server that
/// provides a projection for an account advertises the
/// `urn:ietf:params:jmap:graph:projection` capability for that account."
/// The session-level value is an empty object `{}` and is not modelled;
/// the account-level value is [`GraphProjectionCapability`].
pub const JMAP_GRAPH_PROJECTION_URI: &str = "urn:ietf:params:jmap:graph:projection";

/// JMAP Object URI type name of a [`Graph`](crate::Graph)
/// (draft-atwood-jmap-graph-00 §JMAP Object URI Type Registrations), for
/// comparison with [`jmap_types::JmapUri::data_type`].
pub const JMAP_URI_GRAPH_TYPE: &str = "graph";

/// JMAP Object URI type name of a [`GraphNode`](crate::GraphNode), as in
/// the draft's `jmap:graphnode:A2:N9`.
pub const JMAP_URI_GRAPH_NODE_TYPE: &str = "graphnode";

/// JMAP Object URI type name of a [`GraphEdge`](crate::GraphEdge).
pub const JMAP_URI_GRAPH_EDGE_TYPE: &str = "graphedge";

/// Account-level capability object for `"urn:ietf:params:jmap:graph"`.
///
/// Found at `accounts[id].accountCapabilities["urn:ietf:params:jmap:graph"]`.
///
/// Spec: draft-atwood-jmap-graph-00 §Account-Level Capability Object.
///
/// # Example
///
/// ```
/// use jmap_graph_types::GraphCapability;
///
/// let cap: GraphCapability = serde_json::from_str(r#"{"maxAnnotationSize": 65536}"#)?;
/// assert_eq!(cap.max_annotation_size, Some(65536));
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphCapability {
    /// "The maximum size, in octets, of the `annotations` property of a
    /// single Graph, GraphNode, or GraphEdge, measured as UTF-8 encoded
    /// JSON." `None` (`null`) "means the server advertises no limit".
    /// Exceeding it is a `tooLarge` SetError, a server check.
    pub max_annotation_size: Option<u64>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The value of `Principal.capabilities["urn:ietf:params:jmap:graph"]`
/// (draft-atwood-jmap-graph-00 §Principal Capability Object, RFC 9670
/// §2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalGraphCapability {
    /// "The id of the account holding the Principal's Graphs, or `null` if
    /// the user has no access to it."
    pub account_id: Option<Id>,
    /// "`true` if the user may add this Principal to the `shareWith`
    /// property of a Graph."
    pub may_share_with: bool,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Account-level capability object for
/// `"urn:ietf:params:jmap:graph:projection"`.
///
/// Found at
/// `accounts[id].accountCapabilities["urn:ietf:params:jmap:graph:projection"]`.
///
/// Spec: draft-atwood-jmap-graph-projection-00 §Account-Level Capability
/// Object. "An account has at most one projected Graph. It lives in the
/// account whose data it projects."
///
/// # Example
///
/// ```
/// use jmap_graph_types::GraphProjectionCapability;
///
/// let cap: GraphProjectionCapability = serde_json::from_str(r#"{"graphId": "G1"}"#)?;
/// assert_eq!(cap.graph_id.as_ref(), "G1");
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphProjectionCapability {
    /// "The id of the account's projected Graph."
    pub graph_id: Id,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
