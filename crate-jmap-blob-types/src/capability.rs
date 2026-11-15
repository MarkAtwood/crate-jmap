//! Capability URIs and objects (blobext-03 §2.1, RFC 9404 §3.1).

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::digest::DigestAlgorithm;

/// Capability URI for blobext-03 (§2.1): `urn:ietf:params:jmap:blob2`.
pub const JMAP_BLOB2_URI: &str = "urn:ietf:params:jmap:blob2";

/// Capability URI for RFC 9404 (§3.1): `urn:ietf:params:jmap:blob`.
///
/// A server MAY advertise it next to `blob2` for older clients; a client
/// MUST NOT put both in one request's `using` (blobext-03 §2.1).
pub const JMAP_BLOB_URI: &str = "urn:ietf:params:jmap:blob";

/// The minimum `maxDataSources` a server may advertise: "Servers MUST
/// support at least 64 DataSourceObjects per creation" (blobext-03 §2.1).
pub const MIN_MAX_DATA_SOURCES: u64 = 64;

/// Session-level value of both blob capabilities: "MUST be an empty
/// object" (blobext-03 §2.1; RFC 9404 §3.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobCapability {
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// Account-level value of `urn:ietf:params:jmap:blob2` (blobext-03 §2.1).
///
/// The object "MUST contain" all 17 properties, so every one is written,
/// `null` included. `None` in a `*_types` list means the server does not
/// support that conversion; `None` in a `max_*` limit means no advertised
/// limit.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobAccountCapability {
    /// `maxSizeBlobSet`: largest blob `Blob/set` may create.
    #[serde(default)]
    pub max_size_blob_set: Option<u64>,
    /// `maxDataSources`: most DataSourceObjects in one creation (≥ 64).
    pub max_data_sources: u64,
    /// `supportedTypeNames`: data types `Blob/lookup` supports.
    #[serde(default)]
    pub supported_type_names: Vec<String>,
    /// `supportedDigestAlgorithms`, in the server's order of preference.
    #[serde(default)]
    pub supported_digest_algorithms: Vec<DigestAlgorithm>,
    /// `uploadUrl`: a per-account upload URI template.
    #[serde(default)]
    pub upload_url: Option<String>,
    /// `chunkSize`: preferred chunk size hint.
    #[serde(default)]
    pub chunk_size: Option<u64>,
    /// `supportedImageReadTypes`.
    #[serde(default)]
    pub supported_image_read_types: Option<Vec<String>>,
    /// `supportedImageWriteTypes`.
    #[serde(default)]
    pub supported_image_write_types: Option<Vec<String>>,
    /// `supportedArchiveTypes`.
    #[serde(default)]
    pub supported_archive_types: Option<Vec<String>>,
    /// `supportedExtractTypes`.
    #[serde(default)]
    pub supported_extract_types: Option<Vec<String>>,
    /// `supportedCompressTypes`.
    #[serde(default)]
    pub supported_compress_types: Option<Vec<String>>,
    /// `supportedDecompressTypes`.
    #[serde(default)]
    pub supported_decompress_types: Option<Vec<String>>,
    /// `supportedDeltaTypes`.
    #[serde(default)]
    pub supported_delta_types: Option<Vec<String>>,
    /// `supportedPatchTypes`.
    #[serde(default)]
    pub supported_patch_types: Option<Vec<String>>,
    /// `maxConvertSize`: largest input blob to `Blob/convert`.
    #[serde(default)]
    pub max_convert_size: Option<u64>,
    /// `maxArchiveEntries`.
    #[serde(default)]
    pub max_archive_entries: Option<u64>,
    /// `maxImageDimension`.
    #[serde(default)]
    pub max_image_dimension: Option<u64>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl Default for BlobAccountCapability {
    /// `maxDataSources` at the required minimum of 64; every other
    /// property empty or `null` (no lookup types, no digests, no
    /// conversions, no advertised limits).
    fn default() -> Self {
        Self {
            max_size_blob_set: None,
            max_data_sources: MIN_MAX_DATA_SOURCES,
            supported_type_names: Vec::new(),
            supported_digest_algorithms: Vec::new(),
            upload_url: None,
            chunk_size: None,
            supported_image_read_types: None,
            supported_image_write_types: None,
            supported_archive_types: None,
            supported_extract_types: None,
            supported_compress_types: None,
            supported_decompress_types: None,
            supported_delta_types: None,
            supported_patch_types: None,
            max_convert_size: None,
            max_archive_entries: None,
            max_image_dimension: None,
            extra: Map::new(),
        }
    }
}

impl BlobAccountCapability {
    /// Check the §2.1 rules a value must meet before a server advertises
    /// it: `maxDataSources` at least 64, and every digest algorithm name
    /// "expressed in lowercase".
    ///
    /// # Errors
    ///
    /// The first rule broken, as a [`CapabilityError`].
    pub fn check(&self) -> Result<(), CapabilityError> {
        if self.max_data_sources < MIN_MAX_DATA_SOURCES {
            return Err(CapabilityError::TooFewDataSources(self.max_data_sources));
        }
        for alg in &self.supported_digest_algorithms {
            let name = alg.as_str();
            if name.is_empty() || name.chars().any(|c| c.is_ascii_uppercase()) {
                return Err(CapabilityError::DigestNotLowercase(name.to_owned()));
            }
        }
        Ok(())
    }

    /// The RFC 9404 view of this capability, for advertising
    /// `urn:ietf:params:jmap:blob` next to `blob2`: the four properties
    /// RFC 9404 §3.1 defines, with `adler` removed from the digest list.
    pub fn rfc9404(&self) -> Rfc9404AccountCapability {
        Rfc9404AccountCapability {
            max_size_blob_set: self.max_size_blob_set,
            max_data_sources: self.max_data_sources,
            supported_type_names: self.supported_type_names.clone(),
            supported_digest_algorithms: self
                .supported_digest_algorithms
                .iter()
                .filter(|a| **a != DigestAlgorithm::Adler)
                .cloned()
                .collect(),
            extra: Map::new(),
        }
    }
}

/// Account-level value of `urn:ietf:params:jmap:blob` (RFC 9404 §3.1).
/// All four properties are always written.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rfc9404AccountCapability {
    /// `maxSizeBlobSet`.
    #[serde(default)]
    pub max_size_blob_set: Option<u64>,
    /// `maxDataSources`.
    pub max_data_sources: u64,
    /// `supportedTypeNames`.
    #[serde(default)]
    pub supported_type_names: Vec<String>,
    /// `supportedDigestAlgorithms`.
    #[serde(default)]
    pub supported_digest_algorithms: Vec<DigestAlgorithm>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// A [`BlobAccountCapability`] that breaks a blobext-03 §2.1 rule.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityError {
    /// `maxDataSources` is below 64 (the value advertised).
    TooFewDataSources(u64),
    /// A `supportedDigestAlgorithms` name is empty or not lowercase.
    DigestNotLowercase(String),
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewDataSources(n) => write!(
                f,
                "maxDataSources {n} is below the required minimum of {MIN_MAX_DATA_SOURCES}"
            ),
            Self::DigestNotLowercase(name) => {
                write!(f, "digest algorithm {name:?} is not a lowercase name")
            }
        }
    }
}

impl std::error::Error for CapabilityError {}
