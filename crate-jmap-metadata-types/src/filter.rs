//! draft-ietf-jmap-metadata-01 §3.4.1 — Metadata/query filter condition.
//!
//! Provides [`MetadataFilterCondition`].

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// Filter condition for `Metadata/query` (draft-ietf-jmap-metadata-01 §3.4.1).
///
/// All fields are optional; a condition with no fields set matches every
/// Metadata object the requesting user is allowed to see. A Metadata object
/// matches only when every provided field matches.
///
/// ## `@type` field
///
/// The wire field name `"@type"` is not a valid Rust identifier, so the
/// Rust field is named [`type_names`](Self::type_names) with
/// `#[serde(rename = "@type")]`. Values are matched as a set: a Metadata
/// object matches when its `@type` property equals any of the listed
/// strings (§3.4.1).
///
/// ## `relatedIds` / `relatedType` coupling
///
/// Per §3.4.1, `relatedIds` MUST only appear when `relatedType` is also
/// specified. Servers MUST reject queries that violate this with
/// `invalidArguments`. This crate does not enforce the constraint at the
/// type level — that is the server's responsibility.
///
/// ## `textMatch` semantics
///
/// Per §3.4.1 `textMatch` searches (case-insensitively, by default)
/// against vendor-specific string properties. Servers MAY extend the
/// search to standard properties as well. The exact matching algorithm
/// is implementation-defined.
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
/// The whole point of the Metadata extension is to give vendors a
/// capability-declared, server-aware extras mechanism. Vendor data that
/// needs to be filterable belongs in an [`Annotation`](super::Annotation)
/// payload queried via [`text_match`](Self::text_match), not in a
/// vendor-extended filter condition.
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataFilterCondition {
    /// Only Metadata objects whose `@type` property value is in this array
    /// are returned (§3.4.1).
    ///
    /// Wire field name is literally `"@type"` — not a valid Rust
    /// identifier, hence the rename. Rust field is named `type_names`
    /// (plural) to reflect that the wire value is `String[]`, not
    /// `String`.
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub type_names: Option<Vec<String>>,

    /// Only Metadata objects whose `relatedType` equals this value are
    /// returned (§3.4.1). Required when [`related_ids`](Self::related_ids)
    /// is specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_type: Option<String>,

    /// Only Metadata objects whose `relatedId` is in this array are
    /// returned (§3.4.1). MUST only be specified when
    /// [`related_type`](Self::related_type) is also specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_ids: Option<Vec<Id>>,

    /// Only Metadata objects whose `isPrivate` matches this value are
    /// returned (§3.4.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_private: Option<bool>,

    /// Only Metadata objects whose vendor-specific string properties
    /// contain this text (case-insensitively, by default) are returned
    /// (§3.4.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_match: Option<String>,
}

/// Concrete filter type for Metadata/query (draft-ietf-jmap-metadata-01 §3.4).
///
/// Alias for `jmap_types::query::Filter<MetadataFilterCondition>` provided
/// so callers do not have to reach into `jmap-types` directly. Mirrors the
/// canonical [`jmap_mail_types::EmailFilter`] shape from the workspace
/// canonical extension-types template.
///
/// [`jmap_mail_types::EmailFilter`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/type.EmailFilter.html
pub type MetadataFilter = jmap_types::query::Filter<MetadataFilterCondition>;
