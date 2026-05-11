//! draft-ietf-jmap-metadata-01 §2 — Metadata object types.
//!
//! Provides [`Metadata`] — a tagged union of [`Annotation`], [`ImapMetadata`],
//! and [`WebDavMetadata`] — discriminated by the `@type` property as defined
//! in §2.1.

use std::collections::BTreeMap;

use jmap_types::Id;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Metadata union
// ---------------------------------------------------------------------------

/// A JMAP Metadata object (draft-ietf-jmap-metadata-01 §2).
///
/// The spec defines three concrete object types — [`Annotation`],
/// [`ImapMetadata`], and [`WebDavMetadata`] — all discriminated by the
/// `@type` property. This enum is serialised with `@type` as the internal
/// tag, matching the wire format defined by §2.1.
///
/// Additional metadata types MAY be defined by future specifications
/// (§2.1). Such future types will require new variants on this enum; the
/// `#[non_exhaustive]` derive keeps that non-breaking.
///
/// # Wire-format examples
///
/// ```json
/// { "@type": "Annotation", "id": "MD1", "relatedType": "Mailbox",
///   "relatedId": "MB1", "isPrivate": true,
///   "acme.example.com:color": "blue" }
///
/// { "@type": "ImapMetadata", "id": "MD2", "relatedType": "Mailbox",
///   "relatedId": "MB1", "isPrivate": false,
///   "metadata": { "comment": "Team mailbox" } }
///
/// { "@type": "WebDavMetadata", "id": "MD3", "relatedType": "FileNode",
///   "relatedId": "F1", "isPrivate": false,
///   "metadata": { "{DAV:}displayname": "Project Documents" } }
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "@type")]
pub enum Metadata {
    /// General-purpose vendor metadata (draft-ietf-jmap-metadata-01 §2.1.1).
    Annotation(Annotation),
    /// IMAP RFC 5464 mapping (draft-ietf-jmap-metadata-01 §2.1.2).
    ImapMetadata(ImapMetadata),
    /// WebDAV RFC 4918 dead-property mapping (draft-ietf-jmap-metadata-01 §2.1.3).
    WebDavMetadata(WebDavMetadata),
}

impl Metadata {
    /// Return the `id` property common to every Metadata variant.
    pub fn id(&self) -> Option<&Id> {
        match self {
            Self::Annotation(a) => a.id.as_ref(),
            Self::ImapMetadata(m) => m.id.as_ref(),
            Self::WebDavMetadata(m) => m.id.as_ref(),
        }
    }

    /// Return the `relatedType` property common to every Metadata variant.
    pub fn related_type(&self) -> &str {
        match self {
            Self::Annotation(a) => a.related_type.as_str(),
            Self::ImapMetadata(m) => m.related_type.as_str(),
            Self::WebDavMetadata(m) => m.related_type.as_str(),
        }
    }

    /// Return the `relatedId` property common to every Metadata variant.
    pub fn related_id(&self) -> &Id {
        match self {
            Self::Annotation(a) => &a.related_id,
            Self::ImapMetadata(m) => &m.related_id,
            Self::WebDavMetadata(m) => &m.related_id,
        }
    }

    /// Return the `isPrivate` property common to every Metadata variant
    /// (defaulting to `false` per §2.2.1.5 when the inner field is `None`).
    pub fn is_private(&self) -> bool {
        match self {
            Self::Annotation(a) => a.is_private.unwrap_or(false),
            Self::ImapMetadata(m) => m.is_private.unwrap_or(false),
            Self::WebDavMetadata(m) => m.is_private.unwrap_or(false),
        }
    }

    /// Return the wire string for the `@type` discriminator.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Annotation(_) => "Annotation",
            Self::ImapMetadata(_) => "ImapMetadata",
            Self::WebDavMetadata(_) => "WebDavMetadata",
        }
    }
}

// ---------------------------------------------------------------------------
// Annotation
// ---------------------------------------------------------------------------

/// General-purpose vendor metadata (draft-ietf-jmap-metadata-01 §2.1.1).
///
/// The `@type` wire value is `"Annotation"`; it is supplied by the enclosing
/// [`Metadata`] tag and does NOT appear as a field on this struct.
///
/// ## Vendor-specific properties (§2.2.1.6)
///
/// Annotations are designed to carry arbitrary vendor-defined properties
/// beyond the common fields below. Vendor properties MUST be prefixed with a
/// domain name owned by the vendor, e.g. `acme.example.com:color`. Those
/// fields are captured in [`extra`](Self::extra) and round-trip
/// losslessly per the workspace extras-preservation policy.
///
/// ## `id` is server-set
///
/// On `Metadata/set` create operations the client omits `id`; the server
/// assigns it. `id` is `Option<Id>` so client-side request construction is
/// natural.
///
/// ## `isPrivate` default
///
/// Per §2.2.1.5 the default is `false`. The field is `Option<bool>` so
/// callers can distinguish "explicitly false on the wire" from "absent".
/// Wire-omitted defaults to shared.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    /// Server-assigned identifier. Absent on create requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// JMAP type name of the related object (e.g. `"Email"`, `"Mailbox"`).
    /// Mandatory per §2.2.1.3.
    pub related_type: String,

    /// Identifier of the related JMAP object. Mandatory per §2.2.1.4.
    pub related_id: Id,

    /// Whether this annotation is private to the authenticated user.
    /// Default `false` per §2.2.1.5.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_private: Option<bool>,

    /// Vendor-specific properties (§2.2.1.6) and any other extension fields
    /// not covered by the typed fields above.
    ///
    /// Captures every JSON key on the wire that is not `@type`, `id`,
    /// `relatedType`, `relatedId`, or `isPrivate`. Vendor properties are
    /// domain-prefixed strings (e.g. `acme.example.com:color`) per the
    /// spec's naming requirement.
    ///
    /// Round-trips losslessly per the workspace extras-preservation policy
    /// (see workspace `AGENTS.md`). Empty-map case serialises to nothing,
    /// preserving wire-byte identity with the spec examples.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// ImapMetadata
// ---------------------------------------------------------------------------

/// IMAP-METADATA-extension mapping (draft-ietf-jmap-metadata-01 §2.1.2).
///
/// The `@type` wire value is `"ImapMetadata"`; it is supplied by the enclosing
/// [`Metadata`] tag and does NOT appear as a field on this struct.
///
/// ## `relatedType` constraint
///
/// Per §2.1.2 `relatedType` MUST be `"Mailbox"`. Servers MUST reject other
/// values with an `invalidProperties` SetError. This crate does not enforce
/// the constraint at the type level — that is the server's responsibility.
///
/// ## `metadata` key semantics
///
/// Per §2.2.2.1 the keys are IMAP metadata entry names with the
/// `"/private/"` or `"/shared/"` prefix stripped. The interpretation
/// depends on [`is_private`](Self::is_private): keys map under `/private/`
/// when `true`, under `/shared/` otherwise.
///
/// An empty-string value is permitted and represents an IMAP entry that
/// exists with no value (§2.2.2.1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImapMetadata {
    /// Server-assigned identifier. Absent on create requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// JMAP type name of the related object. MUST be `"Mailbox"` per §2.1.2.
    pub related_type: String,

    /// Identifier of the related JMAP `Mailbox` object.
    pub related_id: Id,

    /// Whether this metadata is private to the authenticated user. Selects
    /// the `/private/` (`true`) vs `/shared/` (`false` or absent) IMAP
    /// namespace prefix per §2.1.2 and §2.2.2.1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_private: Option<bool>,

    /// IMAP metadata entries. Keys are entry names with the
    /// `/private/` or `/shared/` prefix stripped (§2.2.2.1). Empty-string
    /// values represent entries that exist but have no value.
    ///
    /// Uses `BTreeMap` for deterministic key ordering on serialise. The
    /// spec does not require a specific ordering; deterministic output is
    /// preferred for round-trip preservation tests and reproducible
    /// builds. The wire format is `Map<String, String>` either way.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// WebDavMetadata
// ---------------------------------------------------------------------------

/// WebDAV dead-property mapping (draft-ietf-jmap-metadata-01 §2.1.3).
///
/// The `@type` wire value is `"WebDavMetadata"`; it is supplied by the
/// enclosing [`Metadata`] tag and does NOT appear as a field on this struct.
///
/// ## `relatedType` constraint
///
/// Per §2.1.3 valid `relatedType` values include `"Calendar"`,
/// `"CalendarEvent"`, `"AddressBook"`, `"ContactCard"`, and `"FileNode"`.
/// Servers MAY reject other values with `invalidProperties`. This crate
/// does not enforce the constraint at the type level.
///
/// ## `metadata` key format
///
/// Per §2.2.3.1 keys MUST use the expanded-name format
/// `"{namespace-uri}localname"` (e.g.
/// `"{http://example.com/ns}priority"`, `"{DAV:}displayname"`). Values
/// are either simple text or serialised XML inner content for properties
/// with complex structure.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavMetadata {
    /// Server-assigned identifier. Absent on create requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// JMAP type name of the related object (e.g. `"FileNode"`,
    /// `"Calendar"`, `"CalendarEvent"`, `"AddressBook"`, `"ContactCard"`).
    pub related_type: String,

    /// Identifier of the related JMAP object.
    pub related_id: Id,

    /// Whether this metadata is private to the authenticated user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_private: Option<bool>,

    /// WebDAV dead properties. Keys MUST be in the expanded-name format
    /// `"{namespace-uri}localname"` (§2.2.3.1). Values are simple text
    /// for properties with text content, or serialised XML inner content
    /// for properties with complex structure.
    ///
    /// `BTreeMap` is used for deterministic serialise ordering; the wire
    /// format is `Map<String, String>` either way.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
