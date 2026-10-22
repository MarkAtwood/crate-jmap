//! draft-atwood-jmap-graph-edit-00 — JMAP Graph Live Editing, the
//! `urn:ietf:params:jmap:graph:edit` capability.
//!
//! - [`JMAP_GRAPH_EDIT_URI`], [`GraphEditCapability`] (§Capability).
//! - [`GraphNode::edit_format`] and [`GraphNode::snapshot_seq`], the
//!   §GraphNode Extensions properties.
//! - [`GraphNodeOp`] (§The GraphNodeOp Data Type), with its `/query`
//!   [`GraphNodeOpFilterCondition`] and [`GraphNodeOpComparator`]
//!   (§GraphNodeOp/query and GraphNodeOp/queryChanges).
//! - [`OPS_COMPACTED`] (§Errors).
//! - [`GraphNodeCursorRequest`] and [`GraphNodeCursorResponse`]
//!   (§GraphNode/cursor).
//! - The §WebSocket Stream frames: [`GraphEditStreamMessage`] over
//!   [`GraphEditStreamEnable`] and [`GraphEditStreamDisable`]
//!   (client→server), and [`GraphEditEvent`] over [`GraphNodeOpEvent`] and
//!   [`GraphCursorEvent`] (server→client).
//!
//! The server never reads op or cursor `data`: both are base64url without
//! padding (RFC 4648 §5), with a meaning defined by the node's
//! `editFormat`. They are kept as the encoded `String`; rejecting invalid
//! base64url, oversized data, a `baseSeq` past the head and every other
//! rule of the draft is a server check.

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::GraphNode;

/// Capability URI for JMAP Graph Live Editing
/// (draft-atwood-jmap-graph-edit-00 §Capability).
///
/// "This capability is meaningful only when `urn:ietf:params:jmap:graph` is
/// also advertised for the account." The session-level value "is an empty
/// JSON object `{}`" and is not modelled; the account-level value is
/// [`GraphEditCapability`].
pub const JMAP_GRAPH_EDIT_URI: &str = "urn:ietf:params:jmap:graph:edit";

/// The method-level error type of `GraphNodeOp/query` when "the server has
/// destroyed an op of the node whose `seq` is greater than `afterSeq`, so
/// that the result would not be contiguous with `afterSeq`"
/// (draft-atwood-jmap-graph-edit-00 §Errors). [`jmap_types::JmapError`]
/// carries any type string, so this only names it.
pub const OPS_COMPACTED: &str = "opsCompacted";

/// Wire name of the GraphNode `editFormat` extension property
/// (draft-atwood-jmap-graph-edit-00 §GraphNode Extensions).
pub const GRAPH_NODE_EDIT_FORMAT_PROPERTY: &str = "editFormat";

/// Wire name of the GraphNode `snapshotSeq` extension property
/// (draft-atwood-jmap-graph-edit-00 §GraphNode Extensions).
pub const GRAPH_NODE_SNAPSHOT_SEQ_PROPERTY: &str = "snapshotSeq";

/// Account-level capability object for `"urn:ietf:params:jmap:graph:edit"`.
///
/// Found at
/// `accounts[id].accountCapabilities["urn:ietf:params:jmap:graph:edit"]`.
///
/// Spec: draft-atwood-jmap-graph-edit-00 §Account-Level Capability Object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEditCapability {
    /// "The largest op, in octets before base64url encoding, that the
    /// server accepts." A larger op is a `tooLarge` SetError.
    pub max_op_size: u64,
    /// "The largest cursor data, in octets before base64url encoding, that
    /// the server accepts in `GraphNode/cursor`."
    pub max_cursor_size: u64,
    /// "The number of seconds for which the server keeps an op after it is
    /// sequenced, even when a snapshot covers it." `None` (`null`) "means
    /// the server promises no retention beyond what compaction requires".
    pub op_retention: Option<u64>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl GraphNode {
    /// The node's `editFormat` (draft-atwood-jmap-graph-edit-00 §GraphNode
    /// Extensions): "The media type of the node's ops, or `null` if the
    /// node has no edit log." Read from [`extra`](Self::extra).
    ///
    /// Returns `Ok(None)` when the property is absent or `null` (default
    /// `null`).
    ///
    /// # Errors
    ///
    /// Returns the deserialization error when the value is not a String.
    pub fn edit_format(&self) -> Result<Option<String>, serde_json::Error> {
        self.extra
            .get(GRAPH_NODE_EDIT_FORMAT_PROPERTY)
            .map_or(Ok(None), Option::<String>::deserialize)
    }

    /// The node's `snapshotSeq` (draft-atwood-jmap-graph-edit-00
    /// §GraphNode Extensions): "The `seq` of the last op included in the
    /// node's current blob, or `null` if the node has no snapshot." Read
    /// from [`extra`](Self::extra).
    ///
    /// Returns `Ok(None)` when the property is absent or `null` (default
    /// `null`).
    ///
    /// # Errors
    ///
    /// Returns the deserialization error when the value is not an
    /// UnsignedInt.
    pub fn snapshot_seq(&self) -> Result<Option<u64>, serde_json::Error> {
        self.extra
            .get(GRAPH_NODE_SNAPSHOT_SEQ_PROPERTY)
            .map_or(Ok(None), Option::<u64>::deserialize)
    }
}

/// One op in a node's edit log (draft-atwood-jmap-graph-edit-00 §The
/// GraphNodeOp Data Type). "Ops are immutable once created."
///
/// Server-set properties are required on deserialize; a `/set` create
/// argument (`nodeId`, `baseSeq`, `data`) is not a whole GraphNodeOp.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeOp {
    /// "The id of the op." Immutable, server-set.
    pub id: Id,
    /// "The id of the Graph of the op's node." Immutable, server-set.
    pub graph_id: Id,
    /// "The id of the GraphNode whose content the op edits." Immutable.
    pub node_id: Id,
    /// "The op's position in the node's edit log. The first op of a node
    /// has `seq` 1, and each later op of that node has the previous op's
    /// `seq` plus one, with no gaps." Immutable, server-set.
    pub seq: u64,
    /// "The `seq` of the last op the author had applied when creating this
    /// op, or 0 if none." Immutable.
    pub base_seq: u64,
    /// "The op's content, encoded with base64url without padding", with a
    /// meaning defined by the node's `editFormat`. Immutable.
    pub data: String,
    /// "When the op was sequenced." Immutable, server-set.
    pub created: UTCDate,
    /// "The id of the Principal that created the op", as for GraphNode
    /// `createdBy`. Immutable, server-set. The only authorship the draft
    /// attests (§Attribution).
    pub created_by: Option<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Filter condition for `GraphNodeOp/query`, which is also its whole
/// filter (draft-atwood-jmap-graph-edit-00 §GraphNodeOp/query and
/// GraphNodeOp/queryChanges).
///
/// "The `filter` argument MUST be a single FilterCondition, not a
/// FilterOperator, and MUST include `nodeId`"; any other filter is
/// `unsupportedFilter`. As with the GraphNode and GraphEdge filters (see
/// [`query`](crate::query)), `node_id` is required and unknown keys are
/// rejected, so a FilterOperator, a condition without `nodeId` or an
/// unknown clause fails the decode.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNodeOpFilterCondition {
    /// "The op's node." Required.
    pub node_id: Id,
    /// "The op's `seq` is greater than this value."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<u64>,
}

impl GraphNodeOpFilterCondition {
    /// The condition selecting every op of `node_id`.
    pub fn new(node_id: Id) -> Self {
        Self {
            node_id,
            after_seq: None,
        }
    }
}

/// Sort property for `GraphNodeOp/query`: "The only supported sort is
/// `seq` ascending, which is also the order when `sort` is absent."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GraphNodeOpSortProperty {
    /// Sort by `seq`.
    Seq,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(GraphNodeOpSortProperty, "a sort property string",
    "seq" => Seq,
);

/// RFC 8620 §5.5 `Comparator` for `GraphNodeOp/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5). The draft supports
/// only ascending order; rejecting `false` is a server check.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNodeOpComparator {
    /// The property to sort by.
    pub property: GraphNodeOpSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl GraphNodeOpComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: GraphNodeOpSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Request arguments for `GraphNode/cursor` (draft-atwood-jmap-graph-edit-00
/// §GraphNode/cursor), which "reports the calling user's cursor or
/// selection in a node to other users subscribed to it".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeCursorRequest {
    /// "The account holding the node."
    pub account_id: Id,
    /// "The node."
    pub node_id: Id,
    /// "The cursor or selection, encoded with base64url without padding",
    /// with a meaning defined by the node's `editFormat`.
    pub data: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl GraphNodeCursorRequest {
    /// Construct a [`GraphNodeCursorRequest`] from its three required
    /// fields, with no extras.
    pub fn new(account_id: Id, node_id: Id, data: String) -> Self {
        Self {
            account_id,
            node_id,
            data,
            extra: serde_json::Map::new(),
        }
    }
}

/// Response arguments of `GraphNode/cursor`: "The response has the
/// arguments `accountId` and `nodeId` only."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeCursorResponse {
    /// The account holding the node.
    pub account_id: Id,
    /// The node.
    pub node_id: Id,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl GraphNodeCursorResponse {
    /// Construct a [`GraphNodeCursorResponse`], with no extras.
    pub fn new(account_id: Id, node_id: Id) -> Self {
        Self {
            account_id,
            node_id,
            extra: serde_json::Map::new(),
        }
    }
}

/// `dataTypes` value selecting [`GraphNodeOpEvent`]s.
pub const DATA_TYPE_OPS: &str = "ops";

/// `dataTypes` value selecting [`GraphCursorEvent`]s.
pub const DATA_TYPE_CURSORS: &str = "cursors";

/// The recognized values of [`GraphEditStreamEnable::data_types`]
/// (draft-atwood-jmap-graph-edit-00 §GraphEditStreamEnable): "`\"ops\"` for
/// `GraphNodeOpEvent` and `\"cursors\"` for `GraphCursorEvent`".
///
/// "If `dataTypes` is empty or contains no recognized value, the server
/// responds with a `RequestError` whose `type` is
/// `urn:ietf:params:jmap:error:invalidArguments`"; "Unrecognized values
/// alongside recognized ones are ignored."
pub const GRAPH_EDIT_DATA_TYPES: &[&str] = &[DATA_TYPE_OPS, DATA_TYPE_CURSORS];

/// Client→server: start or replace the op and cursor subscription on this
/// WebSocket connection (draft-atwood-jmap-graph-edit-00
/// §GraphEditStreamEnable). "A new `GraphEditStreamEnable` replaces the
/// previous subscription in its entirety. Success is silent".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEditStreamEnable {
    /// Client-specified identifier for this frame. A `RequestError` this
    /// frame causes carries it as its `requestId` (RFC 8887 §4.3.4).
    /// Absent unless the client sent one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// "The GraphNodes whose ops and cursors the client wishes to receive.
    /// An id matches that id in every account the user can access".
    pub node_ids: Vec<Id>,
    /// "A non-empty list of the event categories wanted". Recognized values
    /// are [`GRAPH_EDIT_DATA_TYPES`]; other values are kept verbatim.
    pub data_types: Vec<String>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl GraphEditStreamEnable {
    /// A subscription to `data_types` of `node_ids`, with no `id` and no
    /// extras.
    pub fn new(node_ids: Vec<Id>, data_types: Vec<String>) -> Self {
        Self {
            id: None,
            node_ids,
            data_types,
            extra: serde_json::Map::new(),
        }
    }
}

/// Client→server: end the subscription (draft-atwood-jmap-graph-edit-00
/// §GraphEditStreamDisable). Its "only property is `@type`".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GraphEditStreamDisable {
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A Graph edit stream control frame, discriminated by its `@type`.
///
/// Unrecognised `@type` values deserialize to
/// [`GraphEditStreamMessage::Unknown`], which keeps the discriminant and
/// every other field so the frame re-serializes verbatim. A frame with a
/// known `@type` but a malformed body is an error, not `Unknown`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphEditStreamMessage {
    /// `"GraphEditStreamEnable"`.
    Enable(GraphEditStreamEnable),
    /// `"GraphEditStreamDisable"`.
    Disable(GraphEditStreamDisable),
    /// Any `@type` not recognized by this version of the library.
    Unknown {
        /// The original wire value of `@type`.
        type_name: String,
        /// Every other top-level field, verbatim.
        payload: serde_json::Map<String, serde_json::Value>,
    },
}

const ENABLE: &str = "GraphEditStreamEnable";
const DISABLE: &str = "GraphEditStreamDisable";

impl Serialize for GraphEditStreamMessage {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (tag, body) = match self {
            GraphEditStreamMessage::Enable(e) => (ENABLE, serde_json::to_value(e)),
            GraphEditStreamMessage::Disable(e) => (DISABLE, serde_json::to_value(e)),
            GraphEditStreamMessage::Unknown { type_name, payload } => (
                type_name.as_str(),
                Ok(serde_json::Value::Object(payload.clone())),
            ),
        };
        let body = body.map_err(serde::ser::Error::custom)?;
        crate::wire::with_tag(body, tag)
            .map_err(serde::ser::Error::custom)?
            .serialize(s)
    }
}

impl<'de> Deserialize<'de> for GraphEditStreamMessage {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use crate::wire::from_map;
        let mut map = serde_json::Map::deserialize(d)?;
        let type_name = crate::wire::take_tag::<D::Error>(&mut map)?;
        Ok(match type_name.as_str() {
            ENABLE => GraphEditStreamMessage::Enable(from_map::<_, D>(map)?),
            DISABLE => GraphEditStreamMessage::Disable(from_map::<_, D>(map)?),
            _ => GraphEditStreamMessage::Unknown {
                type_name,
                payload: map,
            },
        })
    }
}

/// An op sequenced on a subscribed node (draft-atwood-jmap-graph-edit-00
/// §GraphNodeOpEvent). Server→client only, sent "when `\"ops\"` is in
/// `dataTypes`".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeOpEvent {
    /// "The account holding the op."
    pub account_id: Id,
    /// "The op, with all its properties."
    pub op: GraphNodeOp,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Another user's cursor or selection in a subscribed node
/// (draft-atwood-jmap-graph-edit-00 §GraphCursorEvent). Server→client only,
/// sent to connections "subscribed to the node with `\"cursors\"` in
/// `dataTypes`".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphCursorEvent {
    /// "The account holding the node."
    pub account_id: Id,
    /// "The node."
    pub node_id: Id,
    /// "The id of the Principal whose cursor this is, or `null` if the
    /// server does not support Principals."
    pub sender_id: Option<Id>,
    /// "The cursor data as given to `GraphNode/cursor`."
    pub data: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl GraphCursorEvent {
    /// Construct a [`GraphCursorEvent`] from its four fields, with no
    /// extras.
    pub fn new(account_id: Id, node_id: Id, sender_id: Option<Id>, data: String) -> Self {
        Self {
            account_id,
            node_id,
            sender_id,
            data,
            extra: serde_json::Map::new(),
        }
    }
}

/// A Graph edit stream event frame, discriminated by its `@type`.
///
/// Unrecognised `@type` values deserialize to [`GraphEditEvent::Unknown`],
/// which keeps the discriminant and every other field so the frame
/// re-serializes verbatim. A frame with a known `@type` but a malformed
/// body is an error, not `Unknown`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum GraphEditEvent {
    /// `"GraphNodeOpEvent"`.
    Op(GraphNodeOpEvent),
    /// `"GraphCursorEvent"`.
    Cursor(GraphCursorEvent),
    /// Any `@type` not recognized by this version of the library.
    Unknown {
        /// The original wire value of `@type`.
        type_name: String,
        /// Every other top-level field, verbatim.
        payload: serde_json::Map<String, serde_json::Value>,
    },
}

const OP_EVENT: &str = "GraphNodeOpEvent";
const CURSOR_EVENT: &str = "GraphCursorEvent";

impl GraphEditEvent {
    /// The event's `@type`, such as `"GraphNodeOpEvent"`; for
    /// [`GraphEditEvent::Unknown`], the original wire value.
    pub fn type_name(&self) -> &str {
        match self {
            GraphEditEvent::Op(_) => OP_EVENT,
            GraphEditEvent::Cursor(_) => CURSOR_EVENT,
            GraphEditEvent::Unknown { type_name, .. } => type_name,
        }
    }
}

impl Serialize for GraphEditEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let body = match self {
            GraphEditEvent::Op(e) => serde_json::to_value(e),
            GraphEditEvent::Cursor(e) => serde_json::to_value(e),
            GraphEditEvent::Unknown { payload, .. } => {
                Ok(serde_json::Value::Object(payload.clone()))
            }
        };
        let body = body.map_err(serde::ser::Error::custom)?;
        crate::wire::with_tag(body, self.type_name())
            .map_err(serde::ser::Error::custom)?
            .serialize(s)
    }
}

impl<'de> Deserialize<'de> for GraphEditEvent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use crate::wire::from_map;
        let mut map = serde_json::Map::deserialize(d)?;
        let type_name = crate::wire::take_tag::<D::Error>(&mut map)?;
        Ok(match type_name.as_str() {
            OP_EVENT => GraphEditEvent::Op(from_map::<_, D>(map)?),
            CURSOR_EVENT => GraphEditEvent::Cursor(from_map::<_, D>(map)?),
            _ => GraphEditEvent::Unknown {
                type_name,
                payload: map,
            },
        })
    }
}
