//! draft-ietf-jmap-metadata-04 §1.2.1 — capability registration and
//! per-data-type metadata info.
//!
//! Provides [`DataTypeMetadataInfo`] and the capability URI constant
//! [`JMAP_METADATA_URI`].

use serde::{Deserialize, Serialize};

/// The JMAP capability URI for the Metadata extension
/// (draft-ietf-jmap-metadata-04 §1.2.1).
///
/// Present as a key in both the session-level `capabilities` object (value:
/// empty object `{}`) and in each account's `accountCapabilities` object
/// (value: an object with a `dataTypes` field mapping type names to
/// [`DataTypeMetadataInfo`]).
///
/// The URI is unchanged from -01 through -04.
pub const JMAP_METADATA_URI: &str = "urn:ietf:params:jmap:metadata";

/// Per-data-type metadata capability advertisement
/// (draft-ietf-jmap-metadata-04 §1.2.1).
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
    /// (§1.2.1). Each value is a registered name (lowercase US-ASCII letters,
    /// digits, hyphens, underscores — no dot). Vendor domain-name namespaces MUST
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

    /// Maximum depth of a namespace value (§1.2.1, §2.1). `null` means
    /// no server-enforced limit; otherwise draft-ietf-jmap-metadata-04
    /// §1.2.1 says "the value MUST be at least 1."
    ///
    /// Per -04 §2.1, a scalar has depth 0, an array has the maximum depth
    /// of its elements (0 if empty), and an object has depth 1 plus the
    /// maximum depth of its member values (1 if empty). So depth 1 allows
    /// a flat namespace object and depth 2 one level of nested objects.
    pub max_depth: Option<u64>,
}
