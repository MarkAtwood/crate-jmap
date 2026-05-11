//! draft-ietf-jmap-filenode-13 §3.1 — FileNode object and component types.
//!
//! Provides [`FileNode`], [`NodeType`], [`NodeRole`], and [`FilesRights`].

use std::collections::HashMap;

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Serialize};

/// The type of a FileNode (draft-ietf-jmap-filenode-13 §3.1, IANA "JMAP FileNode Types"
/// registry §10.4).
///
/// Values are registered strings.  Any unrecognised value is preserved as
/// [`NodeType::Other`] so clients do not lose data when the registry gains new entries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NodeType {
    /// Regular file; `blobId` MUST be non-null.
    File,
    /// Collection that may contain child nodes; `blobId` MUST be null.
    Directory,
    /// Symbolic link; `target` MUST be non-null and `blobId` MUST be null.
    Symlink,
    /// Any node type string not recognised by this implementation.
    ///
    /// The inner string retains the original wire value for round-trip fidelity.
    Other(String),
}

impl_string_enum!(NodeType, "a JMAP FileNode type string",
    "file"      => File,
    "directory" => Directory,
    "symlink"   => Symlink,
);

impl NodeType {
    /// Return the wire-format string for this node type.
    pub fn to_wire_str(&self) -> &str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
            Self::Other(s) => s.as_str(),
        }
    }
}

/// Special role identifying a directory's common purpose
/// (draft-ietf-jmap-filenode-13 §3.1, IANA "JMAP FileNode Roles" registry §10.5).
///
/// Clients MUST ignore unrecognised role values (§3.1).  Unknown values are preserved
/// as [`NodeRole::Other`] so they round-trip correctly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NodeRole {
    /// Base of a filesystem; should have `parentId` null.
    Root,
    /// User's home directory.
    Home,
    /// Temporary space; may be cleaned up automatically.
    Temp,
    /// Deleted data.
    Trash,
    /// Document storage.
    Documents,
    /// Downloaded files.
    Downloads,
    /// Audio files.
    Music,
    /// Photos and images.
    Pictures,
    /// Video files.
    Videos,
    /// Any role string not recognised by this implementation.
    Other(String),
}

impl_string_enum!(NodeRole, "a JMAP FileNode role string",
    "root"      => Root,
    "home"      => Home,
    "temp"      => Temp,
    "trash"     => Trash,
    "documents" => Documents,
    "downloads" => Downloads,
    "music"     => Music,
    "pictures"  => Pictures,
    "videos"    => Videos,
);

impl NodeRole {
    /// Return the wire-format string for this role.
    pub fn to_wire_str(&self) -> &str {
        match self {
            Self::Root => "root",
            Self::Home => "home",
            Self::Temp => "temp",
            Self::Trash => "trash",
            Self::Documents => "documents",
            Self::Downloads => "downloads",
            Self::Music => "music",
            Self::Pictures => "pictures",
            Self::Videos => "videos",
            Self::Other(s) => s.as_str(),
        }
    }
}

/// ACL rights the authenticated user (or a shared user) holds for a FileNode
/// (draft-ietf-jmap-filenode-13 §3.1, `myRights` description).
///
/// `Default` produces all-false (no access), which is the most restrictive valid
/// value and a safe starting point when constructing rights in tests or server code.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesRights {
    /// User may read the properties and blob content of this node.
    pub may_read: bool,
    /// User may create child nodes in this directory.
    pub may_add_children: bool,
    /// User may rename or move this node.
    pub may_rename: bool,
    /// User may destroy this node.
    pub may_delete: bool,
    /// User may update content-related properties (`blobId`, `type`, `target`,
    /// `modified`, `accessed`, `executable`).
    pub may_modify_content: bool,
    /// User may change the sharing of this node (see RFC 9670 JMAP Sharing).
    pub may_share: bool,
}

/// A JMAP FileNode object (draft-ietf-jmap-filenode-13 §3.1).
///
/// ## Nullable vs optional fields
///
/// Several fields are **required-but-nullable**: they MUST appear in the wire JSON
/// even when their value is `null`.  These use `Option<T>` with NO
/// `skip_serializing_if`, so serde emits `"field":null`.
///
/// Other fields are **truly optional**: absent from the wire when not set by the
/// client or server.  These use `#[serde(skip_serializing_if = "Option::is_none")]`.
///
/// | Nullable (must appear as `null`) | Optional (absent when `None`) |
/// |---|---|
/// | `parent_id`, `blob_id`, `target`, `size`, `media_type`, `share_with`, `role` | `node_type`, `created`, `modified`, `accessed`, `changed`, `executable`, `is_subscribed`, `my_rights` |
///
/// ## `modified` and `accessed` semantics
///
/// Both are client-managed.  The server does NOT automatically update them on change.
/// Setting either to `null` in an update (`None` after deserialization) signals the
/// server to reset it to the current time.  This differs from `changed`, which the
/// server sets automatically and clients cannot set.
///
/// ## `share_with` visibility
///
/// `shareWith` is `null` when the requesting user lacks `myRights.mayShare` or the
/// node is not shared with anyone.
///
/// ## `blob_id` lifetime guarantee
///
/// A blob referenced by a FileNode MUST NOT be garbage-collected by the server while
/// the FileNode exists (§3.1).  The server backend must enforce this invariant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNode {
    /// Server-assigned immutable identifier.
    pub id: Id,

    /// Id of the parent node, or `null` if this is a top-level node.
    ///
    /// Required-and-nullable: always present in wire JSON (as `null` or an Id string).
    pub parent_id: Option<Id>,

    /// The type of node.  Immutable after creation.
    ///
    /// If absent on creation the server infers: `"file"` if `blobId` is non-null,
    /// `"symlink"` if `target` is non-null, `"directory"` otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<NodeType>,

    /// The blobId for the file content.
    ///
    /// Required-and-nullable: always present in wire JSON.
    /// MUST be non-null for file nodes, null for directory and symlink nodes.
    pub blob_id: Option<Id>,

    /// Symlink target as an array of path elements.
    ///
    /// Required-and-nullable: always present in wire JSON.
    /// MUST be non-null for symlink nodes, null for file and directory nodes.
    pub target: Option<Vec<String>>,

    /// Size in bytes of the associated blob data (server-set).
    ///
    /// Required-and-nullable: always present in wire JSON.
    /// MUST be null for directory and symlink nodes, non-null for file nodes.
    pub size: Option<u64>,

    /// User-visible name for the FileNode.  Net-Unicode, at least 1 character.
    pub name: String,

    /// The media type (IANA) of the FileNode.
    ///
    /// Required-and-nullable: always present in wire JSON.
    /// Wire field name is literally `"type"` (a Rust keyword).
    /// MUST be non-null for file nodes, null for directory and symlink nodes.
    #[serde(rename = "type")]
    pub media_type: Option<String>,

    /// The date the node was created.
    ///
    /// Default: current server time.  Absent from wire when `None`.
    ///
    /// Uses the [`UTCDate`] newtype to make the wire-format constraint
    /// (RFC 8620 §1.4: 20-character UTCDateTime string) explicit at the
    /// type level. JSON wire format is unchanged because `UTCDate` is a
    /// transparent newtype around `String`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<UTCDate>,

    /// The date the node was last updated, client-managed.
    ///
    /// Setting to `null` in an update signals the server to reset to the current time.
    /// The server does NOT auto-update this value.  Absent from wire when `None`.
    ///
    /// See [`created`](Self::created) for the typing rationale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<UTCDate>,

    /// The date the node was last accessed, client-managed.
    ///
    /// Setting to `null` in an update signals the server to reset to the current time.
    /// The server does NOT auto-update this value.  Absent from wire when `None`.
    ///
    /// See [`created`](Self::created) for the typing rationale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessed: Option<UTCDate>,

    /// The date the server last recorded a change to any property (server-set).
    ///
    /// Automatically updated on every mutation.  Not settable by clients.
    /// Absent from wire when `None`.
    ///
    /// Uses the [`UTCDate`] newtype to make the wire-format constraint
    /// (RFC 8620 §1.4: 20-character UTCDateTime string) explicit at the
    /// type level. JSON wire format is unchanged because `UTCDate` is a
    /// `#[serde(transparent)]` newtype around `String`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<UTCDate>,

    /// If true, the node should be treated as executable.  Default: false.
    ///
    /// Absent from wire when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<bool>,

    /// Whether the current user is subscribed to this node.  Default: true.
    ///
    /// Per-user property.  Absent from wire when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_subscribed: Option<bool>,

    /// ACL rights the authenticated user holds for this node (server-set).
    ///
    /// Absent from wire when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_rights: Option<FilesRights>,

    /// Map of userId → rights for users this node is shared with.
    ///
    /// Required-and-nullable: always present in wire JSON.
    /// `null` when the requesting user lacks `myRights.mayShare` or the node is
    /// not shared.
    pub share_with: Option<HashMap<Id, FilesRights>>,

    /// Special role identifying this directory's purpose.
    ///
    /// Required-and-nullable (draft §3.1 type: `String|null`): always present
    /// in wire JSON; serializes as `null` when unset.  MUST be `null` for
    /// file nodes.
    pub role: Option<NodeRole>,
}
