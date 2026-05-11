//! Property selector enum and [`jmap_types`] trait impls for [`crate::Metadata`].
//!
//! Defined here so `jmap-metadata-server` can use them without violating the
//! orphan rule (`JmapObject` is foreign; `Metadata` is local to this crate).

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enum
// ---------------------------------------------------------------------------

/// Property selector for [`crate::Metadata`] `/get` and `/set`.
///
/// Mirrors the common-properties list from
/// draft-ietf-jmap-metadata-01 §2.2.1 plus the type-specific
/// [`Metadata`](MetadataProperty::Metadata) variant for IMAP- and
/// WebDAV-flavour objects. Vendor-specific Annotation properties are
/// not enumerated here — clients refer to them by their wire string
/// (e.g. `"acme.example.com:color"`) directly via
/// [`VendorProperty`](MetadataProperty::VendorProperty).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MetadataProperty {
    /// The `@type` property (draft-ietf-jmap-metadata-01 §2.2.1.1).
    TypeName,
    /// The `id` property (draft-ietf-jmap-metadata-01 §2.2.1.2).
    Id,
    /// The `relatedType` property (draft-ietf-jmap-metadata-01 §2.2.1.3).
    RelatedType,
    /// The `relatedId` property (draft-ietf-jmap-metadata-01 §2.2.1.4).
    RelatedId,
    /// The `isPrivate` property (draft-ietf-jmap-metadata-01 §2.2.1.5).
    IsPrivate,
    /// The `metadata` property carried by
    /// [`crate::ImapMetadata`] (§2.2.2.1) and [`crate::WebDavMetadata`]
    /// (§2.2.3.1).
    Metadata,
    /// A vendor-specific Annotation property (§2.2.1.6). The inner string
    /// is the domain-prefixed wire key (e.g. `"acme.example.com:color"`).
    VendorProperty(String),
}

// ---------------------------------------------------------------------------
// JmapObject / marker trait impls
// ---------------------------------------------------------------------------

impl JmapObject for crate::Metadata {
    const TYPE_NAME: &'static str = "Metadata";
    type Property = MetadataProperty;
}

impl GetObject for crate::Metadata {}

impl SetObject for crate::Metadata {
    type Patch = PatchObject;
}

impl QueryObject for crate::Metadata {
    type Filter = crate::MetadataFilterCondition;
    type Comparator = serde_json::Value;
}
