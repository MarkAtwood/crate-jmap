//! JMAP FileNode extension data types.
//!
//! Implements the data types defined in draft-ietf-jmap-filenode-13.
//! Types only — no method handlers, no async, no network I/O.
//!
//! ## Module layout
//!
//! - [`filenode`] — [`FileNode`], [`NodeType`], [`NodeRole`], [`FilesRights`]
//! - [`capability`] — [`FileNodeCapability`], [`JMAP_FILENODE_URI`]
//! - [`filter`] — [`FileNodeFilterCondition`]
//!
//! All public types are re-exported at the crate root.

#![forbid(unsafe_code)]

pub mod backend;
pub mod capability;
pub mod filenode;
pub mod filter;

pub use backend::FileNodeProperty;
pub use capability::{FileNodeCapability, JMAP_FILENODE_URI};
pub use filenode::{FileNode, FilesRights, NodeRole, NodeType};
pub use filter::{FileNodeFilter, FileNodeFilterCondition};

/// Generic filter algebra from `jmap-types::query` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-filenode-types` do not need a
/// direct dependency on `jmap-types`. Mirrors the canonical
/// [`jmap_mail_types::query`] re-exports from the workspace canonical
/// extension-types template.
///
/// [`jmap_mail_types::query`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/index.html
pub use jmap_types::query::{Filter, FilterOperator, Operator};
