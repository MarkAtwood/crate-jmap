//! JMAP `FileNode`
//! ([draft-ietf-jmap-filenode-20](https://datatracker.ietf.org/doc/draft-ietf-jmap-filenode/)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-filenode-types`](https://crates.io/crates/jmap-filenode-types),
//!   always enabled.
//! - `client`: [`jmap-filenode-client`](https://crates.io/crates/jmap-filenode-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-filenode-server`](https://crates.io/crates/jmap-filenode-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_filenode::types::{FileNode, JMAP_FILENODE_URI, NodeType};
//!
//! assert_eq!(JMAP_FILENODE_URI, "urn:ietf:params:jmap:filenode");
//! let dir: FileNode = serde_json::from_str(r#"{"@type": "Directory", "id": "dir1",
//!     "parentId": null, "name": "Documents", "role": null,
//!     "created": "2024-06-01T00:00:00Z", "modified": "2025-01-20T14:30:00Z",
//!     "shareWith": null}"#)
//!     .unwrap();
//! assert_eq!(dir.node_type, Some(NodeType::Directory));
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub use jmap_filenode_types as types;

#[cfg(feature = "client")]
pub use jmap_filenode_client as client;

#[cfg(feature = "server")]
pub use jmap_filenode_server as server;
