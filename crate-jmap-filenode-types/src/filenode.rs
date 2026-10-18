//! draft-ietf-jmap-filenode-16 §3.1 — FileNode object and component types.
//!
//! Provides [`FileNode`], [`NodeType`], [`NodeRole`], and [`FilesRights`].

use std::collections::HashMap;

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Serialize, Serializer};

/// The `@type` of a FileNode (draft-ietf-jmap-filenode-16 §3.1.1, IANA "JMAP FileNode
/// Types" registry §11.5).
///
/// Values are registered strings.  Any unrecognised value is preserved as
/// [`NodeType::Other`] so clients do not lose data when the registry gains new entries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NodeType {
    /// `"File"`: regular file with blob content (§3.1.2).
    File,
    /// `"Directory"`: collection that may contain child nodes (§3.1.3).
    Directory,
    /// `"SymLink"`: symbolic link to another path in the tree (§3.1.4).
    Symlink,
    /// Any node type string not recognised by this implementation.
    ///
    /// The inner string retains the original wire value for round-trip fidelity.
    Other(String),
}

impl_string_enum!(NodeType, "a JMAP FileNode type string",
    "File"      => File,
    "Directory" => Directory,
    "SymLink"   => Symlink,
);

impl NodeType {
    /// Return the wire-format string for this node type.
    pub fn to_wire_str(&self) -> &str {
        match self {
            Self::File => "File",
            Self::Directory => "Directory",
            Self::Symlink => "SymLink",
            Self::Other(s) => s.as_str(),
        }
    }
}

/// Special role identifying a directory's common purpose
/// (draft-ietf-jmap-filenode-16 §3.1.3, IANA "JMAP FileNode Roles" registry §11.6).
///
/// Clients MUST ignore unrecognised role values (§3.1.3).  Unknown values are preserved
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
/// (draft-ietf-jmap-filenode-16 §3.1.1, `myRights` description).
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
    /// User may update content-related properties (`blobId`, `mediaType`, `target`,
    /// `modified`, `accessed`, `executable`).
    pub may_modify_content: bool,
    /// User may change the sharing of this node (see RFC 9670 JMAP Sharing).
    pub may_share: bool,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A JMAP FileNode object (draft-ietf-jmap-filenode-16 §3.1).
///
/// ## Type-specific properties
///
/// Properties defined for one `@type` MUST NOT appear on a node of another type;
/// they are omitted rather than set to null (§3.1, §3.2.1).  Serialization therefore
/// depends on [`node_type`](Self::node_type):
///
/// | `@type` | Emitted type-specific properties |
/// |---|---|
/// | `File` | `blobId`, `size`, `mediaType`, `executable` (each only when `Some`) |
/// | `Directory` | `role` (always; `null` when `None`, as it is `String|null`) |
/// | `SymLink` | `target` (only when `Some`) |
/// | absent or unrecognised | every type-specific property that is `Some` |
///
/// Deserialization accepts any of these properties regardless of `@type`, and an
/// explicit `null` deserializes to `None`; the server must ignore such nulls (§3.1).
///
/// ## Common properties
///
/// `parentId` and `shareWith` are required-and-nullable: always emitted, as `null`
/// when `None`.  `@type`, `created`, `modified`, `accessed`, `changed`,
/// `isSubscribed`, and `myRights` are omitted when `None`.
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
/// the FileNode exists (§3.1.2).  The server backend must enforce this invariant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNode {
    /// Server-assigned immutable identifier.
    pub id: Id,

    /// Id of the parent node, or `null` if this is a top-level node.
    ///
    /// Required-and-nullable: always present in wire JSON (as `null` or an Id string).
    pub parent_id: Option<Id>,

    /// The `@type` of the node.  Immutable after creation.
    ///
    /// MUST be provided on creation (§3.1.1); the server does not infer it.
    /// `None` only for partial objects that omit it.
    #[serde(rename = "@type")]
    pub node_type: Option<NodeType>,

    /// The blobId for the file content.  `File` only; MUST be provided on creation.
    pub blob_id: Option<Id>,

    /// Symlink target as an array of path elements.  `SymLink` only; MUST be
    /// provided on creation.
    pub target: Option<Vec<String>>,

    /// Size in bytes of the associated blob data (server-set).  `File` only.
    pub size: Option<u64>,

    /// User-visible name for the FileNode.  Net-Unicode, at least 1 character.
    pub name: String,

    /// The media type (IANA) of the file.  `File` only; the server defaults it
    /// to `"application/octet-stream"`.
    pub media_type: Option<String>,

    /// The date the node was created.
    ///
    /// Default: current server time.  Absent from wire when `None`.
    ///
    /// Uses the [`UTCDate`] newtype to make the wire-format constraint
    /// (RFC 8620 §1.4: 20-character UTCDateTime string) explicit at the
    /// type level. JSON wire format is unchanged because `UTCDate` is a
    /// transparent newtype around `String`.
    pub created: Option<UTCDate>,

    /// The date the node was last updated, client-managed.
    ///
    /// Setting to `null` in an update signals the server to reset to the current time.
    /// The server does NOT auto-update this value.  Absent from wire when `None`.
    ///
    /// See [`created`](Self::created) for the typing rationale.
    pub modified: Option<UTCDate>,

    /// The date the node was last accessed, client-managed.
    ///
    /// Setting to `null` in an update signals the server to reset to the current time.
    /// The server does NOT auto-update this value.  Absent from wire when `None`.
    ///
    /// See [`created`](Self::created) for the typing rationale.
    pub accessed: Option<UTCDate>,

    /// The date the server last recorded a change to any property (server-set).
    ///
    /// Automatically updated on every mutation.  Not settable by clients.
    /// Absent from wire when `None`.
    ///
    /// See [`created`](Self::created) for the typing rationale.
    pub changed: Option<UTCDate>,

    /// If true, the file should be treated as executable.  `File` only.
    /// Default: false.
    pub executable: Option<bool>,

    /// Whether the current user is subscribed to this node.  Default: true.
    ///
    /// Per-user property.  Absent from wire when `None`.
    pub is_subscribed: Option<bool>,

    /// ACL rights the authenticated user holds for this node (server-set).
    ///
    /// Absent from wire when `None`.
    pub my_rights: Option<FilesRights>,

    /// Map of userId → rights for users this node is shared with.
    ///
    /// Required-and-nullable: always present in wire JSON.
    /// `null` when the requesting user lacks `myRights.mayShare` or the node is
    /// not shared.
    pub share_with: Option<HashMap<Id, FilesRights>>,

    /// Special role identifying this directory's purpose.  `Directory` only
    /// (§3.1.3 type: `String|null`), where it serializes as `null` when unset.
    pub role: Option<NodeRole>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Serialize for FileNode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire<'a> {
            id: &'a Id,
            parent_id: Option<&'a Id>,
            #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
            node_type: Option<&'a NodeType>,
            #[serde(skip_serializing_if = "Option::is_none")]
            blob_id: Option<&'a Id>,
            #[serde(skip_serializing_if = "Option::is_none")]
            target: Option<&'a Vec<String>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            size: Option<u64>,
            name: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            media_type: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            created: Option<&'a UTCDate>,
            #[serde(skip_serializing_if = "Option::is_none")]
            modified: Option<&'a UTCDate>,
            #[serde(skip_serializing_if = "Option::is_none")]
            accessed: Option<&'a UTCDate>,
            #[serde(skip_serializing_if = "Option::is_none")]
            changed: Option<&'a UTCDate>,
            #[serde(skip_serializing_if = "Option::is_none")]
            executable: Option<bool>,
            #[serde(skip_serializing_if = "Option::is_none")]
            is_subscribed: Option<bool>,
            #[serde(skip_serializing_if = "Option::is_none")]
            my_rights: Option<&'a FilesRights>,
            share_with: Option<&'a HashMap<Id, FilesRights>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            role: Option<Option<&'a NodeRole>>,
            #[serde(flatten)]
            extra: &'a serde_json::Map<String, serde_json::Value>,
        }

        let known = match self.node_type.as_ref() {
            Some(t @ (NodeType::File | NodeType::Directory | NodeType::Symlink)) => Some(t),
            _ => None,
        };
        let defines = |owner: &NodeType| known.is_none_or(|t| t == owner);
        let file = defines(&NodeType::File);
        let role = match known {
            Some(NodeType::Directory) => Some(self.role.as_ref()),
            Some(_) => None,
            None => self.role.as_ref().map(Some),
        };

        Wire {
            id: &self.id,
            parent_id: self.parent_id.as_ref(),
            node_type: self.node_type.as_ref(),
            blob_id: self.blob_id.as_ref().filter(|_| file),
            target: self.target.as_ref().filter(|_| defines(&NodeType::Symlink)),
            size: self.size.filter(|_| file),
            name: &self.name,
            media_type: self.media_type.as_deref().filter(|_| file),
            created: self.created.as_ref(),
            modified: self.modified.as_ref(),
            accessed: self.accessed.as_ref(),
            changed: self.changed.as_ref(),
            executable: self.executable.filter(|_| file),
            is_subscribed: self.is_subscribed,
            my_rights: self.my_rights.as_ref(),
            share_with: self.share_with.as_ref(),
            role,
            extra: &self.extra,
        }
        .serialize(serializer)
    }
}
