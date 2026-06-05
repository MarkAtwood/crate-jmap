//! Property selector enum and [`jmap_types`] trait impls for [`crate::FileNode`].
//!
//! Defined here so `jmap-filenode-server` can use them without violating the
//! orphan rule (`JmapObject` is foreign; `FileNode` is local to this crate).

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enum
// ---------------------------------------------------------------------------

/// Property selector for [`crate::FileNode`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FileNodeProperty {
    /// The `id` property (draft-ietf-jmap-filenode-14 §3.1).
    Id,
    /// The `parentId` property (draft-ietf-jmap-filenode-14 §3.1).
    ParentId,
    /// The `nodeType` property (draft-ietf-jmap-filenode-14 §3.1).
    NodeType,
    /// The `blobId` property (draft-ietf-jmap-filenode-14 §3.1).
    BlobId,
    /// The `target` property (draft-ietf-jmap-filenode-14 §3.1).
    Target,
    /// The `size` property (draft-ietf-jmap-filenode-14 §3.1).
    Size,
    /// The `name` property (draft-ietf-jmap-filenode-14 §3.1).
    Name,
    /// The `type` property (media type) (draft-ietf-jmap-filenode-14 §3.1).
    MediaType,
    /// The `created` property (draft-ietf-jmap-filenode-14 §3.1).
    Created,
    /// The `modified` property (draft-ietf-jmap-filenode-14 §3.1).
    Modified,
    /// The `accessed` property (draft-ietf-jmap-filenode-14 §3.1).
    Accessed,
    /// The `changed` property (draft-ietf-jmap-filenode-14 §3.1).
    Changed,
    /// The `executable` property (draft-ietf-jmap-filenode-14 §3.1).
    Executable,
    /// The `isSubscribed` property (draft-ietf-jmap-filenode-14 §3.1).
    IsSubscribed,
    /// The `myRights` property (draft-ietf-jmap-filenode-14 §3.1).
    MyRights,
    /// The `shareWith` property (draft-ietf-jmap-filenode-14 §3.1).
    ShareWith,
    /// The `role` property (draft-ietf-jmap-filenode-14 §3.1).
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
    type Patch = PatchObject;
}

impl QueryObject for crate::FileNode {
    type Filter = crate::FileNodeFilterCondition;
    type Comparator = serde_json::Value;
}
