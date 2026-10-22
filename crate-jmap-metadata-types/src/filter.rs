//! draft-ietf-jmap-metadata-04 §3.6 — metadata filter condition types.
//!
//! Provides [`MetadataTextMatch`] — the value shape used by the four
//! text-matching filter condition fields that this extension adds to each
//! listed data type's `FilterCondition` — and [`WithMetadata`], which
//! adds those fields to a per-type condition on the client side.
//!
//! The six filter condition fields themselves (`metadataExists`,
//! `privateMetadataExists`, `metadataTextContains`,
//! `privateMetadataTextContains`, `metadataTextEquals`,
//! `privateMetadataTextEquals`) extend each listed data type's own
//! `/query` `FilterCondition` (§3.6). They are NOT a standalone filter
//! type — this is the key architectural difference from -01, which had a
//! standalone `MetadataFilterCondition`. The typed per-type filter
//! structs in the extension-types crates (e.g. `EmailFilterCondition`)
//! do not carry these fields (bd JMAP-ua37.4). Clients send them either
//! as a `jmap_types::query::Filter<WithMetadata<C>>`, which the
//! extension clients' `/query` and `/queryChanges` methods accept, or in
//! a `serde_json::Value` filter tree built with `jmap-metadata-client`'s
//! condition builders.

use serde::{Deserialize, Serialize};

/// Value shape for the text-matching metadata filter conditions
/// (draft-ietf-jmap-metadata-04 §3.6).
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
/// draft-ietf-jmap-metadata-04 §3.6: the `path` is "a namespace
/// identifier, optionally followed by a JSON Pointer \[RFC6901\] into the
/// namespace value (for example, `<namespace>`, `<namespace>/<key>` or
/// `<namespace>/<key>/<subkey>`)". `/` and `~` inside a key are escaped as
/// `~1` and `~0` per [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901).
/// Object members are addressed by name; a path that would traverse an
/// array or a scalar addresses no value. A server rejects a path with an
/// invalid `~` escape with `invalidArguments`. Same syntax as the string
/// value of `metadataExists`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataTextMatch {
    /// Path within the metadata object to match against.
    ///
    /// Form: `<namespace>`, optionally followed by an RFC 6901 JSON
    /// Pointer into the namespace value, such as `<namespace>/<key>` or
    /// `<namespace>/<key>/<subkey>`.
    pub path: String,

    /// The string to search for at `path`. Interpretation (substring
    /// containment vs exact equality) depends on which filter condition
    /// field carries this `MetadataTextMatch`.
    pub value: String,
}

impl MetadataTextMatch {
    /// Create a `MetadataTextMatch` for `path` and `value`.
    pub fn new(path: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            value: value.into(),
        }
    }
}

/// A per-type `FilterCondition` extended with the six
/// draft-ietf-jmap-metadata-04 §3.6 condition fields.
///
/// §3.6: "For each data type listed in dataTypes, the FilterCondition
/// object for Foo/query is extended with the optional fields defined
/// below." `base` is the data type's own condition (for example
/// `jmap_mail_types::EmailFilterCondition`); its fields are
/// flattened into the same JSON object as the metadata fields. Use it as
/// the leaf of a `jmap_types::query::Filter<WithMetadata<C>>` and pass that
/// to the extension client's `/query` or `/queryChanges` method; the
/// conditions compose with `AND` / `OR` / `NOT` as usual (§3.6, RFC 8620
/// §5.5).
///
/// # Serialize only
///
/// This type is a client-side request builder. It does not implement
/// `Deserialize`: `#[serde(flatten)]` cannot be combined with
/// `#[serde(deny_unknown_fields)]`, and decoding a filter without that
/// check lets a misspelt key decode as a match-all condition (see the
/// `jmap_types::query::Filter` rustdoc).
///
/// # Example
///
/// ```rust
/// use jmap_metadata_types::{MetadataTextMatch, WithMetadata};
///
/// #[derive(Default, serde::Serialize)]
/// #[serde(rename_all = "camelCase")]
/// struct EmailCond {
///     in_mailbox: Option<String>,
/// }
///
/// let mut cond = WithMetadata::<EmailCond>::default();
/// cond.base.in_mailbox = Some("MB-inbox".into());
/// cond.private_metadata_text_contains =
///     Some(MetadataTextMatch::new("acme.example.com/memo", "follow up"));
/// assert_eq!(
///     serde_json::to_value(&cond).unwrap(),
///     serde_json::json!({
///         "inMailbox": "MB-inbox",
///         "privateMetadataTextContains": {
///             "path": "acme.example.com/memo",
///             "value": "follow up"
///         }
///     })
/// );
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WithMetadata<C> {
    /// The data type's own filter condition.
    #[serde(flatten)]
    pub base: C,

    /// Matches if a value is present at this path in the object's
    /// `metadata`. The path has the [`MetadataTextMatch::path`] syntax.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_exists: Option<String>,

    /// As `metadata_exists`, but against the authenticated user's
    /// `privateMetadata`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_metadata_exists: Option<String>,

    /// Substring match under the `i;unicode-casemap` collation
    /// ([RFC 5051](https://www.rfc-editor.org/rfc/rfc5051)) on the
    /// `metadata` string at a path (draft-ietf-jmap-metadata-04 §3.6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_text_contains: Option<MetadataTextMatch>,

    /// As `metadata_text_contains`, but against `privateMetadata`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_metadata_text_contains: Option<MetadataTextMatch>,

    /// Exact match on the `metadata` string at a path, comparing Unicode
    /// code points without case folding or normalization
    /// (draft-ietf-jmap-metadata-04 §3.6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_text_equals: Option<MetadataTextMatch>,

    /// As `metadata_text_equals`, but against `privateMetadata`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_metadata_text_equals: Option<MetadataTextMatch>,
}
