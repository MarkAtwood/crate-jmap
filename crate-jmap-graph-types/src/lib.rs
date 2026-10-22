//! JMAP Graph extension types (draft-atwood-jmap-graph-00).
//!
//! Normative reference: draft-atwood-jmap-graph-00, the
//! `urn:ietf:params:jmap:graph` capability for storing and synchronizing
//! general-purpose directed graphs: a shareable Graph container holding
//! GraphNode vertices and directed, typed GraphEdge links. The draft
//! assigns no meaning to node content, edge types or annotations.
//!
//! ## Crate posture
//!
//! This is a wire-format type crate, per the workspace AGENTS.md
//! kit-vs-jig posture:
//!
//! - No async dependencies, no I/O, no method semantics.
//! - Depends on RFC 8620 types only.
//! - Forbids `unsafe`.
//!
//! All Graph methods are standard RFC 8620 methods (draft §Methods), so
//! their arguments and responses are the generic `jmap-types` shapes,
//! parameterized by the objects, filters and comparators here.
//!
//! ## Public surface
//!
//! - [`JMAP_GRAPH_URI`], [`GraphCapability`], [`PrincipalGraphCapability`]
//!   — the capability (draft §Capability).
//! - [`Graph`], [`GraphRights`] (draft §Graph).
//! - [`GraphNode`] (draft §GraphNode).
//! - [`GraphEdge`] (draft §GraphEdge).
//! - [`query`] — the `/query` filters and comparators of the three types,
//!   including the draft-atwood-jmap-graph-projection-00 GraphNode
//!   `objectUri` filter and its [`JMAP_GRAPH_PROJECTION_URI`] capability,
//!   whose account-level object is [`GraphProjectionCapability`].
//! - [`JMAP_URI_GRAPH_TYPE`], [`JMAP_URI_GRAPH_NODE_TYPE`],
//!   [`JMAP_URI_GRAPH_EDGE_TYPE`] — the JMAP Object URI type names (draft
//!   §JMAP Object URI Type Registrations).
//! - [`edit`] — draft-atwood-jmap-graph-edit-00, the
//!   `urn:ietf:params:jmap:graph:edit` capability: [`GraphNodeOp`], the
//!   GraphNode `editFormat` and `snapshotSeq` accessors, `GraphNode/cursor`
//!   and the WebSocket op and cursor stream.
//! - [`search`] — draft-atwood-jmap-graph-search-00, the
//!   `urn:ietf:params:jmap:graph:search` capability: the
//!   [`GraphSearchCapability`], the built-in filter language
//!   [`Expression`](search::Expression) carried in the GraphNode and
//!   GraphEdge filter conditions' `where`, and `queryTooComplex`.

#![forbid(unsafe_code)]

mod backend;
mod capability;
pub mod edge;
pub mod edit;
pub mod graph;
pub mod node;
pub mod query;
pub mod search;
mod wire;

pub use capability::{
    GraphCapability, GraphProjectionCapability, PrincipalGraphCapability,
    JMAP_GRAPH_PROJECTION_URI, JMAP_GRAPH_URI, JMAP_URI_GRAPH_EDGE_TYPE, JMAP_URI_GRAPH_NODE_TYPE,
    JMAP_URI_GRAPH_TYPE,
};
pub use edge::GraphEdge;
pub use edit::{GraphEditCapability, GraphNodeOp, JMAP_GRAPH_EDIT_URI};
pub use graph::{Graph, GraphRights};
pub use node::GraphNode;
pub use search::{GraphSearchCapability, JMAP_GRAPH_SEARCH_URI};
