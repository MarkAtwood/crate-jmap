//! draft-ietf-jmap-metadata-02 §1.2.1 — capability registration and
//! per-data-type metadata info.
//!
//! Provides [`DataTypeMetadataInfo`] and the capability URI constant
//! [`JMAP_METADATA_URI`].

use serde::{Deserialize, Serialize};

/// The JMAP capability URI for the Metadata extension
/// (draft-ietf-jmap-metadata-02 §1.2.1).
///
/// Present as a key in both the session-level `capabilities` object (value:
/// empty object `{}`) and in each account's `accountCapabilities` object
/// (value: an object with a `dataTypes` field mapping type names to
/// [`DataTypeMetadataInfo`]).
///
/// The URI is unchanged between -01 and -02.
pub const JMAP_METADATA_URI: &str = "urn:ietf:params:jmap:metadata";

/// Per-data-type metadata capability advertisement
/// (draft-ietf-jmap-metadata-02 §1.2.1).
///
/// Appears as the value in the `dataTypes` map within the account-level
/// `urn:ietf:params:jmap:metadata` capability object. A type that does not
/// appear in `dataTypes` does not gain the `metadata` or `privateMetadata`
/// properties in that account.
///
/// # Wire example (from §6.1)
///
/// ```json
/// {
///   "namespaces": ["photography"],
///   "supportsVendorNamespaces": false,
///   "supportsPrivate": false,
///   "maxDepth": 3
/// }
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataTypeMetadataInfo {
    /// IANA-registered metadata namespace names supported on this data type
    /// (§1.2.1). Each value is a registered name (US-ASCII letters, digits,
    /// hyphens, underscores — no dot). Vendor domain-name namespaces MUST
    /// NOT appear in this list; their support is signalled by
    /// [`supports_vendor_namespaces`](Self::supports_vendor_namespaces).
    #[serde(default)]
    pub namespaces: Vec<String>,

    /// Whether the server accepts vendor (domain-name) namespaces on this
    /// data type (§1.2.1). Default `false`.
    #[serde(default)]
    pub supports_vendor_namespaces: bool,

    /// Whether this account supports per-user `privateMetadata` on this
    /// data type (§1.2.1). Default `false`.
    ///
    /// When `false`, the `privateMetadata` property MUST be absent from
    /// response objects of this type, all `privateMetadata*` filter
    /// conditions MUST be rejected with `unsupportedFilter`, and any `/set`
    /// targeting `privateMetadata` MUST be rejected with
    /// `invalidProperties`.
    #[serde(default)]
    pub supports_private: bool,

    /// Maximum depth of nested objects within a namespace value (§1.2.1,
    /// §2.1). `null` means no server-enforced limit.
    ///
    /// Depth 1 = flat properties only; depth 2 = one level of nesting.
    /// Arrays do not contribute to depth themselves, but objects inside
    /// arrays do (§2.1).
    pub max_depth: Option<u64>,
}
