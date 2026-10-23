//! draft-ietf-jmap-filenode-16 §3.2.5 — FileNode/query filter condition.
//!
//! Provides [`FileNodeFilterCondition`] and [`FileNodeComparator`].

use jmap_metadata_types::MetadataTextMatch;
use jmap_types::{Id, UTCDate, impl_string_enum};
use serde::{Deserialize, Serialize};

use crate::filenode::{NodeRole, NodeType};

/// Filter condition for `FileNode/query` (draft-ietf-jmap-filenode-16 §3.2.5).
///
/// All fields are optional; a condition with no fields set matches every FileNode.
/// A node matches only when every provided field matches.
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
/// `urn:ietf:params:jmap:metadata`), which adds namespace-keyed
/// `metadata` / `privateMetadata` properties to each listed data type,
/// filterable through `metadata*` conditions on that type's own `/query`
/// (draft-ietf-jmap-metadata-04 §2, §3.6). This condition carries the six
/// -04 §3.6 conditions as typed leaf fields (bd JMAP-s4kp.6). Helpers live in
/// `jmap-metadata-types`, `jmap-metadata-server`, and
/// `jmap-metadata-client`.
///
/// **Pre-IETF escape.** Vendors who cannot wait for the metadata draft can
/// either escape the filter tree to `serde_json::Value` or fork the
/// `FilterCondition` type. See `crate-jmap-calendars-types/PLAN.md` for
/// the hybrid sloppy-value pattern.
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

    /// Exact match on the node's `@type`.  Named `nodeType` on the wire
    /// rather than `@type` (§3.2.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<NodeType>,

    /// Exact match on the node's `role`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<NodeRole>,

    /// Exact match on the node's `spaceId` extension property
    /// (draft-atwood-jmap-chat-filenode-00 §The `spaceId` Extension
    /// Property: "Servers MUST support `spaceId` as a filter criterion in
    /// `FileNode/query`"). Nodes without `spaceId` never match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Id>,

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

    /// Exact byte match on the node's `mediaType` property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// Glob match on the node's `mediaType` property using the same glob
    /// syntax as `nameMatch`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type_match: Option<String>,

    /// Full-text search in the referenced blob content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    /// Equivalent to `body` OR `nameMatch` OR `mediaTypeMatch`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// draft-ietf-jmap-metadata-04 §3.6 `metadataExists`: a value must be
    /// present at this path in the node's `metadata`. The path is a
    /// namespace identifier, optionally followed by an RFC 6901 JSON
    /// Pointer into the namespace value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_exists: Option<String>,

    /// -04 §3.6 `privateMetadataExists`: as `metadata_exists`, against the
    /// authenticated user's `privateMetadata`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_metadata_exists: Option<String>,

    /// -04 §3.6 `metadataTextContains`: substring match under the
    /// `i;unicode-casemap` collation on the `metadata` string at a path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_text_contains: Option<MetadataTextMatch>,

    /// -04 §3.6 `privateMetadataTextContains`: as
    /// `metadata_text_contains`, against `privateMetadata`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_metadata_text_contains: Option<MetadataTextMatch>,

    /// -04 §3.6 `metadataTextEquals`: exact code-point match on the
    /// `metadata` string at a path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_text_equals: Option<MetadataTextMatch>,

    /// -04 §3.6 `privateMetadataTextEquals`: as `metadata_text_equals`,
    /// against `privateMetadata`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_metadata_text_equals: Option<MetadataTextMatch>,
}

impl FileNodeFilterCondition {
    /// `true` if any of the six draft-ietf-jmap-metadata-04 §3.6
    /// `metadata*` / `privateMetadata*` fields is set.
    pub fn has_metadata_condition(&self) -> bool {
        self.metadata_exists.is_some()
            || self.private_metadata_exists.is_some()
            || self.metadata_text_contains.is_some()
            || self.private_metadata_text_contains.is_some()
            || self.metadata_text_equals.is_some()
            || self.private_metadata_text_equals.is_some()
    }
}

/// Concrete filter type for FileNode/query (draft-ietf-jmap-filenode-16 §3.2.5).
///
/// Alias for `jmap_types::Filter<FileNodeFilterCondition>` provided
/// so callers do not have to reach into `jmap-types` directly. Mirrors the
/// canonical [`jmap_mail_types::EmailFilter`] shape from the workspace
/// canonical extension-types template.
///
/// [`jmap_mail_types::EmailFilter`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/type.EmailFilter.html
pub type FileNodeFilter = jmap_types::Filter<FileNodeFilterCondition>;

/// The property to sort by in a [`FileNodeComparator`].
///
/// draft-ietf-jmap-filenode-16 §3.2.5 adds the `tree` and `nodeType` sort
/// properties; the §1 capability example lists `name`, `mediaType`,
/// `size`, `created` and `modified` as well. A server advertises what it
/// supports in `fileNodeQuerySortOptions`. Unknown names are kept in
/// [`Other`](FileNodeComparatorProperty::Other) so they round-trip; a
/// server answers one it does not support with `unsupportedSort`
/// (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FileNodeComparatorProperty {
    /// Sort by `name`.
    Name,
    /// Sort by `mediaType`.
    MediaType,
    /// Sort by `size`.
    Size,
    /// Sort by `created`.
    Created,
    /// Sort by `modified`.
    Modified,
    /// Sort by `@type`: Directories, then symlinks, then files, then any
    /// other `@type` by value (§3.2.5).
    NodeType,
    /// Sort by name, with each Directory followed by its children sorted
    /// the same way (§3.2.5).
    Tree,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(FileNodeComparatorProperty, "a FileNode sort property string",
    "name"      => Name,
    "mediaType" => MediaType,
    "size"      => Size,
    "created"   => Created,
    "modified"  => Modified,
    "nodeType"  => NodeType,
    "tree"      => Tree,
);

/// Comparator for `FileNode/query` (draft-ietf-jmap-filenode-16 §3.2.5).
///
/// Build one with [`FileNodeComparator::new`]; the struct is
/// `#[non_exhaustive]`. It carries no flatten-extras field: backends
/// dispatch on `property`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNodeComparator {
    /// The property to sort by.
    pub property: FileNodeComparatorProperty,

    /// If `true`, sort ascending; if `false`, sort descending (default `true`).
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub is_ascending: bool,

    /// A collation identifier (RFC 4790) to use when comparing strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl FileNodeComparator {
    /// An ascending comparator on `property` with no collation
    /// (`isAscending` defaults to `true`, RFC 8620 §5.5).
    pub fn new(property: FileNodeComparatorProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

fn bool_true() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

#[cfg(test)]
mod comparator_tests {
    use super::*;

    /// Oracle: the `fileNodeQuerySortOptions` example in
    /// draft-ietf-jmap-filenode-16 §1 ("name", "mediaType", "size",
    /// "created", "modified", "nodeType", "tree"); every listed name is a
    /// known variant and re-serializes unchanged.
    #[test]
    fn spec_example_sort_options_are_known_variants() {
        let want = [
            ("name", FileNodeComparatorProperty::Name),
            ("mediaType", FileNodeComparatorProperty::MediaType),
            ("size", FileNodeComparatorProperty::Size),
            ("created", FileNodeComparatorProperty::Created),
            ("modified", FileNodeComparatorProperty::Modified),
            ("nodeType", FileNodeComparatorProperty::NodeType),
            ("tree", FileNodeComparatorProperty::Tree),
        ];
        for (wire, variant) in want {
            let json = serde_json::json!({"property": wire, "isAscending": true});
            let c: FileNodeComparator = serde_json::from_value(json).expect("must parse");
            assert_eq!(c.property, variant);
            // RFC 8620 §5.5 isAscending "(default: true)": the default is omitted.
            assert_eq!(
                serde_json::to_value(&c).unwrap(),
                serde_json::json!({"property": wire})
            );
        }
    }

    /// Oracle: hand-written JSON; RFC 8620 §5.5 defaults `isAscending` to
    /// true, and an unknown property survives as `Other`.
    #[test]
    fn unknown_property_is_other_and_ascending_defaults_true() {
        let c: FileNodeComparator =
            serde_json::from_str(r#"{"property":"blobId"}"#).expect("must parse");
        assert_eq!(
            c.property,
            FileNodeComparatorProperty::Other("blobId".to_owned())
        );
        assert!(c.is_ascending);
    }
}
