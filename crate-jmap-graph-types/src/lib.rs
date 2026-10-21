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
//! - [`query`] — the `/query` filters and comparators of the three types.
//! - [`JMAP_URI_GRAPH_TYPE`], [`JMAP_URI_GRAPH_NODE_TYPE`],
//!   [`JMAP_URI_GRAPH_EDGE_TYPE`] — the JMAP Object URI type names (draft
//!   §JMAP Object URI Type Registrations).

#![forbid(unsafe_code)]

mod backend;
pub mod capability;
pub mod edge;
pub mod graph;
pub mod node;
pub mod query;

pub use capability::{
    GraphCapability, PrincipalGraphCapability, JMAP_GRAPH_URI, JMAP_URI_GRAPH_EDGE_TYPE,
    JMAP_URI_GRAPH_NODE_TYPE, JMAP_URI_GRAPH_TYPE,
};
pub use edge::GraphEdge;
pub use graph::{Graph, GraphRights};
pub use node::GraphNode;
