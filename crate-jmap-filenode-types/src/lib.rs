//! JMAP [`FileNode`] extension data types.
//!
//! Implements the data types defined in draft-ietf-jmap-filenode-20.
//! Types only — no method handlers, no async, no network I/O.
//!
//! ## Module layout
//!
//! - `filenode` — [`FileNode`], [`NodeType`], [`NodeRole`], [`FilesRights`]
//! - Crate root — [`FileNodeCapability`], [`JMAP_FILENODE_URI`]
//! - `filter` — [`FileNodeFilterCondition`]
//!
//! All public types are re-exported at the crate root.
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types ([`FileNode`]
//! and [`FileNodeFilterCondition`]). `tracing` or `log` macros that interpolate
//! one of these values with `?` (debug) or `%` (display) emit its full contents
//! to the configured sink: file and folder names (often people's names, or what
//! a document is about), metadata, and the name and full-text search terms of a
//! query. That data is typically regulated as personal data under GDPR, CCPA,
//! and similar privacy frameworks.
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks the file name and metadata.
//! tracing::debug!(node = ?file_node, "renamed node");
//!
//! // GOOD — emits only the opaque id.
//! tracing::debug!(node_id = ?file_node.id, "renamed node");
//! ```
//!
//! Plain `Debug` plus this section is the workspace policy for PII-bearing
//! types (decision 53, bd:JMAP-5rmi). Redacting `Debug` impls and log-capture
//! canaries are reserved for credential-grade secrets, such as `BearerAuth` and
//! `BasicAuth` in `jmap-base-client`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod backend;
mod capability;
mod filenode;
mod filter;

pub use backend::FileNodeProperty;
pub use capability::{FileNodeCapability, JMAP_FILENODE_URI};
pub use filenode::{FileNode, FilesRights, NodeRole, NodeType};
pub use filter::{
    FileNodeComparator, FileNodeComparatorProperty, FileNodeFilter, FileNodeFilterCondition,
};

/// Generic filter algebra from `jmap-types` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-filenode-types` do not need a
/// direct dependency on `jmap-types`. Mirrors the same root
/// re-exports in `jmap-mail-types`, the workspace canonical
/// extension-types template.
pub use jmap_types::{Filter, FilterOperator, Operator};
