//! `Blob/get` (blobext-03 §5).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::blob_ref::BlobRef;
use crate::digest::DigestAlgorithm;
use crate::serde_util::{merge_digests, present, take_digests};
use crate::source::DataSourceObject;

/// Implement wire-string serde, `Display` and `From<&str>` for a property
/// enum with a `Digest(DigestAlgorithm)` variant and an `Other` catch-all.
macro_rules! property_enum {
    ($ty:ident, $expecting:literal, $( $s:literal => $variant:ident ),+ $(,)?) => {
        impl $ty {
            /// The wire name: the listed name, `digest:<alg>`, or the
            /// stored `Other` string.
            pub fn name(&self) -> Cow<'_, str> {
                match self {
                    $( Self::$variant => Cow::Borrowed($s), )+
                    Self::Digest(alg) => Cow::Owned(alg.property_name()),
                    Self::Other(s) => Cow::Borrowed(s.as_str()),
                }
            }
        }
        impl From<&str> for $ty {
            fn from(s: &str) -> Self {
                match s {
                    $( $s => Self::$variant, )+
                    _ => match DigestAlgorithm::from_property_name(s) {
                        Some(alg) => Self::Digest(alg),
                        None => Self::Other(s.to_owned()),
                    },
                }
            }
        }
        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.name())
            }
        }
        impl Serialize for $ty {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.name())
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = Cow::<str>::deserialize(d)
                    .map_err(|_| serde::de::Error::custom(concat!("expected ", $expecting)))?;
                Ok(Self::from(s.as_ref()))
            }
        }
    };
}

/// A name in `Blob/get` `properties` (blobext-03 §5).
///
/// `id` is accepted and changes nothing (RFC 8620 §5.1: `id` is always
/// returned).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BlobProperty {
    /// `id`.
    Id,
    /// `data`: `data:asText` if valid UTF-8, else `data:asBase64`.
    Data,
    /// `data:asText`.
    DataAsText,
    /// `data:asBase64`.
    DataAsBase64,
    /// `size`: the whole blob's size.
    Size,
    /// `imageData`.
    ImageData,
    /// `chunks`.
    Chunks,
    /// `digest:<alg>` over the selected range.
    Digest(DigestAlgorithm),
    /// A name this crate does not know.
    Other(String),
}

property_enum!(BlobProperty, "a Blob/get property name",
    "id" => Id,
    "data" => Data,
    "data:asText" => DataAsText,
    "data:asBase64" => DataAsBase64,
    "size" => Size,
    "imageData" => ImageData,
    "chunks" => Chunks,
);

/// A name in `Blob/get` `dataSourceProperties` (blobext-03 §5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataSourceProperty {
    /// `blobId`.
    BlobId,
    /// `size`.
    Size,
    /// `offset`.
    Offset,
    /// `length`.
    Length,
    /// `position`.
    Position,
    /// `digest:<alg>` over the octets the chunk contributes.
    Digest(DigestAlgorithm),
    /// A name this crate does not know.
    Other(String),
}

property_enum!(DataSourceProperty, "a Blob/get dataSourceProperties name",
    "blobId" => BlobId,
    "size" => Size,
    "offset" => Offset,
    "length" => Length,
    "position" => Position,
);

impl DataSourceProperty {
    /// The default `dataSourceProperties`: `["blobId", "offset",
    /// "length"]` (blobext-03 §5).
    pub fn defaults() -> Vec<Self> {
        vec![Self::BlobId, Self::Offset, Self::Length]
    }
}

/// `Blob/get` arguments (blobext-03 §5).
///
/// `ids` entries may be `#creationId` references, as in RFC 9404 §4.2.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobGetArgs {
    /// `accountId`.
    pub account_id: Id,
    /// `ids`: the blobs to read. Required: `null` is `invalidArguments`.
    #[serde(default)]
    pub ids: Option<Vec<BlobRef>>,
    /// `properties`: null means `data` and `size`, allowed only when
    /// `offset` and `length` are both null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<BlobProperty>>,
    /// `offset`: start of the range (null = 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// `length`: octets in the range (null = to the end).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    /// `dataSourceProperties`: properties of each `chunks` entry
    /// (default [`DataSourceProperty::defaults`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_source_properties: Option<Vec<DataSourceProperty>>,
    /// Other arguments, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobGetArgs {
    /// Arguments reading `ids` in `account_id` with default properties.
    pub fn new(account_id: Id, ids: Vec<BlobRef>) -> Self {
        Self {
            account_id,
            ids: Some(ids),
            properties: None,
            offset: None,
            length: None,
            data_source_properties: None,
            extra: Map::new(),
        }
    }
}

/// One `list` entry of a `Blob/get` response (blobext-03 §5).
///
/// Only requested properties are present. `data:asText` is tri-state:
/// `Some(None)` is the `null` written when the range is not valid UTF-8
/// (with `isEncodingProblem`); `imageData` likewise (`null` when the server
/// cannot extract metadata). `digest:<alg>` members go to
/// [`digests`](Self::digests). `isEncodingProblem` and `isTruncated` are
/// written only when true.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct BlobGetItem {
    /// `id`: the blobId.
    pub id: Id,
    /// `data:asText`: absent, `null`, or the text.
    pub data_as_text: Option<Option<String>>,
    /// `data:asBase64`.
    pub data_as_base64: Option<String>,
    /// `size`: the whole blob's size.
    pub size: Option<u64>,
    /// `imageData`: absent, `null`, or the metadata.
    pub image_data: Option<Option<ImageData>>,
    /// `chunks`.
    pub chunks: Option<Vec<DataSourceObject>>,
    /// `digest:<alg>` values over the selected range.
    pub digests: BTreeMap<DigestAlgorithm, String>,
    /// `isEncodingProblem`.
    pub is_encoding_problem: bool,
    /// `isTruncated`.
    pub is_truncated: bool,
    /// Unknown members, kept for round-trip.
    pub extra: Map<String, Value>,
}

impl BlobGetItem {
    /// An entry for `id` with no properties set.
    pub fn new(id: Id) -> Self {
        Self {
            id,
            data_as_text: None,
            data_as_base64: None,
            size: None,
            image_data: None,
            chunks: None,
            digests: BTreeMap::new(),
            is_encoding_problem: false,
            is_truncated: false,
            extra: Map::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemRaw<C> {
    id: Id,
    #[serde(
        rename = "data:asText",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    data_as_text: Option<Option<String>>,
    #[serde(
        rename = "data:asBase64",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    data_as_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    image_data: Option<Option<ImageData>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chunks: Option<C>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_encoding_problem: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_truncated: bool,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

impl Serialize for BlobGetItem {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ItemRaw {
            id: self.id.clone(),
            data_as_text: self.data_as_text.clone(),
            data_as_base64: self.data_as_base64.clone(),
            size: self.size,
            image_data: self.image_data.clone(),
            chunks: self.chunks.as_ref(),
            is_encoding_problem: self.is_encoding_problem,
            is_truncated: self.is_truncated,
            rest: merge_digests(&self.digests, &self.extra),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for BlobGetItem {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut raw = ItemRaw::<Vec<DataSourceObject>>::deserialize(d)?;
        let digests = take_digests(&mut raw.rest).map_err(serde::de::Error::custom)?;
        Ok(Self {
            id: raw.id,
            data_as_text: raw.data_as_text,
            data_as_base64: raw.data_as_base64,
            size: raw.size,
            image_data: raw.image_data,
            chunks: raw.chunks,
            digests,
            is_encoding_problem: raw.is_encoding_problem,
            is_truncated: raw.is_truncated,
            extra: raw.rest,
        })
    }
}

/// `Blob/get` response (blobext-03 §5). It has no `state`: blobs have
/// none (§4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobGetResponse {
    /// `accountId`.
    pub account_id: Id,
    /// `list`: one entry per found blob.
    pub list: Vec<BlobGetItem>,
    /// `notFound`: blobIds not found.
    #[serde(default)]
    pub not_found: Vec<Id>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobGetResponse {
    /// A response for `account_id` with an empty `list` and `notFound`.
    pub fn new(account_id: Id) -> Self {
        Self {
            account_id,
            list: Vec::new(),
            not_found: Vec::new(),
            extra: Map::new(),
        }
    }
}

/// Image or video metadata (blobext-03 §5 `imageData`).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageData {
    /// `width` in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u64>,
    /// `height` in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u64>,
    /// `orientation`: EXIF orientation, 1 to 8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<u64>,
    /// `date` the image or video was captured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<UTCDate>,
    /// `gps` coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gps: Option<ImageGps>,
    /// `duration` in seconds, for video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// `comment`: the embedded EXIF comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// GPS coordinates of an image (blobext-03 §5 `ImageGPS`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageGps {
    /// `latitude` in decimal degrees.
    pub latitude: f64,
    /// `longitude` in decimal degrees.
    pub longitude: f64,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl ImageGps {
    /// Coordinates in decimal degrees.
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
            extra: Map::new(),
        }
    }
}
