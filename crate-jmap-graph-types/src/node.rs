//! draft-atwood-jmap-graph-00 §GraphNode.

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// A vertex in a Graph (draft-atwood-jmap-graph-00 §GraphNode).
///
/// Properties with a draft default (`annotations`, `blobId`, `mediaType`,
/// `uri`) take it when absent. `T|null` properties are `Option<T>` and
/// serialize `None` as `null`.
///
/// `uri` and `mediaType` are written by any user with `mayWrite` and are
/// untrusted (draft §Untrusted Data, §Links to Outside Resources). Nothing
/// here validates them; rejecting a `uri` that is not a URI with
/// `invalidProperties` is a server check.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    /// The id of the node. Immutable, server-set.
    pub id: Id,
    /// "The id of the Graph the node belongs to." Immutable.
    pub graph_id: Id,
    /// "Application data about the node." Draft default `{}`.
    #[serde(default)]
    pub annotations: serde_json::Map<String, serde_json::Value>,
    /// "The id of a blob holding the node's content, or `null` if the node
    /// has no content. Content is opaque to the server."
    #[serde(default)]
    pub blob_id: Option<Id>,
    /// "The media type of the content identified by `blobId`." With a
    /// non-null `blobId` and a `null` `mediaType`, "the content is treated
    /// as `application/octet-stream`".
    #[serde(default)]
    pub media_type: Option<String>,
    /// "A URI (RFC 3986) of a resource outside the Graph that this node
    /// stands for, or `null`." Any scheme; a JMAP Object URI parses with
    /// [`jmap_types::JmapUri`].
    #[serde(default)]
    pub uri: Option<String>,
    /// When the node was created. Server-set.
    pub created: UTCDate,
    /// "When any property of the node was last changed." Server-set.
    pub updated: UTCDate,
    /// "The id of the Principal (RFC 9670) that created the node, or `null`
    /// if the server does not support Principals or cannot determine it."
    /// Server-set.
    pub created_by: Option<Id>,
    /// "The id of the Principal that last changed the node", or `null`
    /// under the same conditions as `createdBy`. Server-set.
    pub updated_by: Option<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
