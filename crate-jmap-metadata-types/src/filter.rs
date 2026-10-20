//! draft-ietf-jmap-metadata-02 §3.5 — metadata filter condition types.
//!
//! Provides [`MetadataTextMatch`] — the value shape used by the four
//! text-matching filter condition fields that this extension adds to each
//! opted-in data type's `FilterCondition`.
//!
//! The six filter condition fields themselves (`metadataExists`,
//! `privateMetadataExists`, `metadataTextContains`,
//! `privateMetadataTextContains`, `metadataTextEquals`,
//! `privateMetadataTextEquals`) extend each opted-in data type's own
//! `/query` `FilterCondition` (§3.5). They are NOT a standalone filter
//! type — this is the key architectural difference from -01, which had a
//! standalone `MetadataFilterCondition`. The typed per-type filter
//! structs in the extension-types crates (e.g. `EmailFilterCondition`)
//! do not carry these fields (bd JMAP-ua37.4); clients send them in a
//! `serde_json::Value` filter tree, which `jmap-metadata-client`'s
//! condition builders produce.

use serde::{Deserialize, Serialize};

/// Value shape for the text-matching metadata filter conditions
/// (draft-ietf-jmap-metadata-02 §3.5).
///
/// Used as the value of the `metadataTextContains`,
/// `privateMetadataTextContains`, `metadataTextEquals`, and
/// `privateMetadataTextEquals` fields on per-type `FilterCondition` objects.
///
/// # Wire example (from §6.7)
///
/// ```json
/// {
///   "path": "acme.example.com/memo",
///   "value": "follow up"
/// }
/// ```
///
/// # Path syntax
///
/// The `path` field uses the form `<namespace>` or `<namespace>/<key>`,
/// with `/` and `~` escaped per [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901)
/// where applicable. Same syntax as the string value of `metadataExists`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataTextMatch {
    /// Path within the metadata object to match against.
    ///
    /// Form: `<namespace>` or `<namespace>/<key>` (with `/` and `~`
    /// escaped per RFC 6901 where applicable).
    pub path: String,

    /// The string to search for at `path`. Interpretation (substring
    /// containment vs exact equality) depends on which filter condition
    /// field carries this `MetadataTextMatch`.
    pub value: String,
}
