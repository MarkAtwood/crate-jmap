//! JMAP Graph
//! ([draft-atwood-jmap-graph-00](https://github.com/MarkAtwood/jmap-chat-spec)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-graph-types`](https://crates.io/crates/jmap-graph-types),
//!   always enabled.
//! - `client`: [`jmap-graph-client`](https://crates.io/crates/jmap-graph-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-graph-server`](https://crates.io/crates/jmap-graph-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_graph::types::{GraphEdgeFilterCondition, JMAP_GRAPH_URI};
//!
//! assert_eq!(JMAP_GRAPH_URI, "urn:ietf:params:jmap:graph");
//! let f: GraphEdgeFilterCondition =
//!     serde_json::from_str(r#"{"graphId": "G1", "fromId": ["N1"]}"#).unwrap();
//! assert_eq!(f.from_id.as_deref().map(<[_]>::len), Some(1));
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub use jmap_graph_types as types;

#[cfg(feature = "client")]
pub use jmap_graph_client as client;

#[cfg(feature = "server")]
pub use jmap_graph_server as server;
