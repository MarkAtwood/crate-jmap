//! Property selector enum and [`jmap_types`] trait impls for [`crate::FileNode`].
//!
//! Defined here so `jmap-filenode-server` can use them without violating the
//! orphan rule (`JmapObject` is foreign; `FileNode` is local to this crate).

use jmap_types::{GetObject, JmapObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enum
// ---------------------------------------------------------------------------

/// Property selector for [`crate::FileNode`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FileNodeProperty {
    Id,
    ParentId,
    NodeType,
    BlobId,
    Target,
    Size,
    Name,
    MediaType,
    Created,
    Modified,
    Accessed,
    Changed,
    Executable,
    IsSubscribed,
    MyRights,
    ShareWith,
    Role,
}

// ---------------------------------------------------------------------------
// JmapObject / marker trait impls
// ---------------------------------------------------------------------------

impl JmapObject for crate::FileNode {
    const TYPE_NAME: &'static str = "FileNode";
    type Property = FileNodeProperty;
}

impl GetObject for crate::FileNode {}

impl SetObject for crate::FileNode {
    type Patch = serde_json::Value;
}

impl QueryObject for crate::FileNode {
    type Filter = crate::FileNodeFilterCondition;
    type Comparator = serde_json::Value;
}
