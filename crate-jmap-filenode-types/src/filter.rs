//! draft-ietf-jmap-filenode-14 §3.2.5 — FileNode/query filter condition.
//!
//! Provides [`FileNodeFilterCondition`].

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// Filter condition for `FileNode/query` (draft-ietf-jmap-filenode-14 §3.2.5).
///
/// All fields are optional; a condition with no fields set matches every FileNode.
/// A node matches only when every provided field matches.
///
/// ## `media_type` field
///
/// The wire field is literally `"type"` (a Rust keyword).  The Rust field is named
/// `media_type` with `#[serde(rename = "type")]`.
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field. Filter clauses the
/// server does not understand are a query-correctness hazard — silently
/// preserving an unrecognised clause and round-tripping it back to the
/// client can return the wrong set of records with no error signal.
///
/// ## What to do instead
///
/// **IETF-track path.** Vendors who need both capability-level declaration
/// and filterability for custom fields should use
/// `draft-ietf-jmap-metadata` (capability URI
/// `urn:ietf:params:jmap:metadata`), which defines a filterable
/// `Metadata` / `Annotation` companion object. Implemented in `jmap-metadata-types`,
/// `jmap-metadata-server`, and `jmap-metadata-client` (bd JMAP-06zp).
///
/// **Pre-IETF escape.** Vendors who cannot wait for the metadata draft can
/// either escape the filter tree to `serde_json::Value` or fork the
/// `FilterCondition` type. See `crate-jmap-calendars-types/PLAN.md` for
/// the hybrid sloppy-value pattern.
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNodeFilterCondition {
    /// If `true`, the node must have a null `parentId`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_top_level: Option<bool>,

    /// Exact match on the node's `parentId`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Id>,

    /// The node must have an ancestor with this id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ancestor_id: Option<Id>,

    /// The node must be an ancestor of the node with this id (inverse of `ancestorId`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descendant_id: Option<Id>,

    /// Exact match on the node's `nodeType` string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,

    /// Exact match on the node's `role` string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    /// If `true`, only nodes with a non-null role match.  If `false`, only nodes
    /// with a null role match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_any_role: Option<bool>,

    /// Exact match on the node's `blobId`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<Id>,

    /// Exact match on the node's `executable` flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_executable: Option<bool>,

    /// The node's `created` date must be strictly before this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_before: Option<UTCDate>,

    /// The node's `created` date must be on or after this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<UTCDate>,

    /// The node's `modified` date must be strictly before this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_before: Option<UTCDate>,

    /// The node's `modified` date must be on or after this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_after: Option<UTCDate>,

    /// The node's `accessed` date must be strictly before this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessed_before: Option<UTCDate>,

    /// The node's `accessed` date must be on or after this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessed_after: Option<UTCDate>,

    /// The node's `size` in bytes must be ≥ this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size: Option<u64>,

    /// The node's `size` in bytes must be < this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,

    /// Exact byte match on the node's `name` property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Glob match on the node's `name` property (case-insensitive;
    /// `*`, `?`, `[abc]`, `[!abc]` supported).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_match: Option<String>,

    /// Exact byte match on the node's `type` (media type) property.
    ///
    /// Wire field name is literally `"type"` — a Rust keyword.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// Glob match on the node's `type` (media type) property using the same glob
    /// syntax as `nameMatch`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_match: Option<String>,

    /// Full-text search in the referenced blob content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    /// Equivalent to `body` OR `nameMatch` OR `typeMatch`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Concrete filter type for FileNode/query (draft-ietf-jmap-filenode-14 §3.2.5).
///
/// Alias for `jmap_types::query::Filter<FileNodeFilterCondition>` provided
/// so callers do not have to reach into `jmap-types` directly. Mirrors the
/// canonical [`jmap_mail_types::EmailFilter`] shape from the workspace
/// canonical extension-types template.
///
/// [`jmap_mail_types::EmailFilter`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/type.EmailFilter.html
pub type FileNodeFilter = jmap_types::query::Filter<FileNodeFilterCondition>;
