//! [`DataSourceObject`] (blobext-03 §3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::blob_ref::BlobRef;
use crate::digest::DigestAlgorithm;
use crate::serde_util::{merge_digests, take_digests};

/// A source of data for a new blob in `Blob/set` and an entry of the
/// `chunks` array in `Blob/get` (blobext-03 §3).
///
/// Used in `Blob/set`, it "MUST contain exactly one of either data:asText
/// or data:asBase64 (for inline data) or blobId". This type does not
/// enforce that: the server checks it and answers `invalidProperties`
/// `["data"]` (§3), which a deserialize error could not express.
///
/// `digest:<alg>` members go to [`digests`](Self::digests); other unknown
/// members to [`extra`](Self::extra). Absent (`None`) properties are left
/// out on output, since a `chunks` entry carries only the requested
/// `dataSourceProperties`; a `null` input value reads as absent.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataSourceObject {
    /// `data:asText`: inline data as a UTF-8 string.
    pub data_as_text: Option<String>,
    /// `data:asBase64`: inline data in base64 (RFC 4648).
    pub data_as_base64: Option<String>,
    /// `blobId`: an existing blob whose octets are copied.
    pub blob_id: Option<BlobRef>,
    /// `offset`: start of the range within the source (null = 0).
    pub offset: Option<u64>,
    /// `length`: octets to copy (null = to the end).
    pub length: Option<u64>,
    /// `size`: the full size of the source (in `chunks`, of the chunk blob).
    pub size: Option<u64>,
    /// `position`: where this source starts in the containing blob.
    pub position: Option<u64>,
    /// `digest:<alg>` values over the octets this source contributes.
    pub digests: BTreeMap<DigestAlgorithm, String>,
    /// Unknown members, kept for round-trip.
    pub extra: Map<String, Value>,
}

impl DataSourceObject {
    /// Inline UTF-8 text (`data:asText`).
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            data_as_text: Some(text.into()),
            ..Self::default()
        }
    }

    /// Inline base64 data (`data:asBase64`).
    pub fn base64(data: impl Into<String>) -> Self {
        Self {
            data_as_base64: Some(data.into()),
            ..Self::default()
        }
    }

    /// The octets of an existing blob (`blobId`).
    pub fn blob(blob_id: BlobRef) -> Self {
        Self {
            blob_id: Some(blob_id),
            ..Self::default()
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    #[serde(
        rename = "data:asText",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    data_as_text: Option<String>,
    #[serde(
        rename = "data:asBase64",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    data_as_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blob_id: Option<BlobRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    position: Option<u64>,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

impl Serialize for DataSourceObject {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Raw {
            data_as_text: self.data_as_text.clone(),
            data_as_base64: self.data_as_base64.clone(),
            blob_id: self.blob_id.clone(),
            offset: self.offset,
            length: self.length,
            size: self.size,
            position: self.position,
            rest: merge_digests(&self.digests, &self.extra),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for DataSourceObject {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut raw = Raw::deserialize(d)?;
        let digests = take_digests(&mut raw.rest).map_err(serde::de::Error::custom)?;
        Ok(Self {
            data_as_text: raw.data_as_text,
            data_as_base64: raw.data_as_base64,
            blob_id: raw.blob_id,
            offset: raw.offset,
            length: raw.length,
            size: raw.size,
            position: raw.position,
            digests,
            extra: raw.rest,
        })
    }
}
