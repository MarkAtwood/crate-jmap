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

#[macro_use]
mod string_enum;

pub mod backend;
pub mod capability;
pub mod filenode;
pub mod filter;

pub use backend::FileNodeProperty;
pub use capability::{FileNodeCapability, JMAP_FILENODE_URI};
pub use filenode::{FileNode, FilesRights, NodeRole, NodeType};
pub use filter::FileNodeFilterCondition;
