//! draft-ietf-jmap-metadata-01 §1.2.1 — capability registration and
//! account-level capability object.
//!
//! Provides [`MetadataCapability`] and the capability URI constant
//! [`JMAP_METADATA_URI`].

use serde::{Deserialize, Serialize};

/// The JMAP capability URI for the Metadata extension
/// (draft-ietf-jmap-metadata-01 §1.2.1).
///
/// Present as a key in both the session-level `capabilities` object (value:
/// empty object) and in each account's `accountCapabilities` object (value:
/// a [`MetadataCapability`]).
pub const JMAP_METADATA_URI: &str = "urn:ietf:params:jmap:metadata";

/// Account-level capability for the JMAP Metadata extension
/// (draft-ietf-jmap-metadata-01 §1.2.1).
///
/// The value of the `urn:ietf:params:jmap:metadata` key in
/// `accountCapabilities`.
///
/// ## Nullable fields
///
/// Fields typed `Option<T>` with no `skip_serializing_if` are
/// **required-and-nullable**: they MUST appear in the wire JSON even when
/// the value is `null`. For [`data_types`](Self::data_types) and
/// [`max_depth`](Self::max_depth) a `null` wire value carries spec-defined
/// semantics ("all data types" / "no nesting limit"), so the `null` cannot
/// be elided.
///
/// ## `maySetPrivate` default
///
/// Per §1.2.1 the default is `true`. This crate represents the field as
/// `Option<bool>` so callers can distinguish an explicit `false` from
/// "absent" if needed; deserialising a JSON document without the key
/// leaves the field as `None`. A `None` value is wire-equivalent to the
/// spec default of `true`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataCapability {
    /// List of JMAP data types for which the server supports metadata
    /// operations. A `null` wire value means all data types are
    /// supported (§1.2.1).
    ///
    /// Always serialised (as `null` when `None`).
    pub data_types: Option<Vec<String>>,

    /// List of metadata type identifiers (`@type` values) for which the
    /// server supports metadata operations (§1.2.1). Only listed
    /// metadata types can be created or retrieved.
    pub metadata_types: Vec<String>,

    /// Maximum depth of nested vendor-specific metadata properties that
    /// can be set or retrieved (§1.2.1). A depth of `1` indicates only
    /// flat properties; `2` allows one level of nesting, and so forth.
    /// A `null` wire value means no server-enforced limit.
    ///
    /// Always serialised (as `null` when `None`).
    pub max_depth: Option<u64>,

    /// Whether the authenticated user has permission to create private
    /// Metadata objects (`isPrivate: true`) in this account (§1.2.1).
    /// Default `true` when absent on the wire.
    ///
    /// If `false`, the server MUST reject creation of private metadata
    /// with a `forbidden` SetError.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub may_set_private: Option<bool>,
}
