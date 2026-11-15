//! Error codes blobext-03 registers or updates (§11.3, §11.4).
//!
//! The method-level code is for `jmap_types::JmapError::custom`; the
//! SetError codes for `jmap_types::SetError::new`.

/// Method-level error: a `Blob/lookup` type name is unknown, or its
/// capability is not in `using` (blobext-03 §6, §11.3; RFC 9404 §6.2).
pub const UNKNOWN_DATA_TYPE: &str = "unknownDataType";

/// SetError: the server could not determine or does not support the
/// format (blobext-03 §11.4.1).
pub const UNKNOWN_FORMAT: &str = "unknownFormat";

/// SetError: `Blob/set` destroy of a blob another object still references
/// (blobext-03 §4, §11.4.2).
pub const BLOB_HAS_REFERENCE: &str = "blobHasReference";

/// SetError: a `Blob/convert` produced no usable output (blobext-03 §8,
/// §11.4.3).
pub const CONVERSION_FAILED: &str = "conversionFailed";
