//! `Blob/convert` (blobext-03 §8): conversion requests, the seven
//! recipes, [`ArchiveEntry`] and the response.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use jmap_types::{Id, SetError, UTCDate, impl_string_enum};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::blob_ref::BlobRef;
use crate::serde_util::{empty_as_null, null_as_empty, present};

/// `Blob/convert` arguments (blobext-03 §8).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobConvertArgs {
    /// `accountId`.
    pub account_id: Id,
    /// `create`: creation id → conversion request.
    pub create: BTreeMap<Id, ConvertRequest>,
    /// Other arguments, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobConvertArgs {
    /// Arguments running `create` in `account_id`.
    pub fn new(account_id: Id, create: BTreeMap<Id, ConvertRequest>) -> Self {
        Self {
            account_id,
            create,
            extra: Map::new(),
        }
    }
}

/// A conversion request object (blobext-03 §8): `noPersist` plus "exactly
/// one" recipe property.
///
/// Deserializing fails when the object has no recipe property or several.
/// A server that must answer that with a per-entry `invalidProperties`
/// SetError inspects the raw object with [`Recipe::KEYS`] first.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertRequest {
    /// `noPersist`: the result need only last for the request.
    pub no_persist: bool,
    /// The recipe.
    pub recipe: Recipe,
    /// Unknown members, kept for round-trip.
    pub extra: Map<String, Value>,
}

impl ConvertRequest {
    /// A persistent conversion running `recipe`.
    pub fn new(recipe: Recipe) -> Self {
        Self {
            no_persist: false,
            recipe,
            extra: Map::new(),
        }
    }
}

impl Serialize for ConvertRequest {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error as _, SerializeMap as _};
        let recipe = match &self.recipe {
            Recipe::ImageConvert(r) => serde_json::to_value(r),
            Recipe::Archive(r) => serde_json::to_value(r),
            Recipe::Extract(r) => serde_json::to_value(r),
            Recipe::Compress(r) => serde_json::to_value(r),
            Recipe::Decompress(r) => serde_json::to_value(r),
            Recipe::Delta(r) => serde_json::to_value(r),
            Recipe::Patch(r) => serde_json::to_value(r),
        }
        .map_err(S::Error::custom)?;
        let mut m = s.serialize_map(None)?;
        if self.no_persist {
            m.serialize_entry("noPersist", &true)?;
        }
        m.serialize_entry(self.recipe.key(), &recipe)?;
        for (k, v) in &self.extra {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl<'de> Deserialize<'de> for ConvertRequest {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let mut map = Map::<String, Value>::deserialize(d)?;
        let no_persist = match map.remove("noPersist") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => b,
            Some(_) => return Err(D::Error::custom("noPersist: expected a boolean")),
        };
        let present: Vec<&str> = Recipe::KEYS
            .iter()
            .copied()
            .filter(|k| map.contains_key(*k))
            .collect();
        let key = match present.as_slice() {
            [key] => *key,
            [] => {
                return Err(D::Error::custom(format!(
                    "a conversion request needs exactly one of {}",
                    Recipe::KEYS.join(", ")
                )));
            }
            several => {
                return Err(D::Error::custom(format!(
                    "a conversion request has several recipes: {}",
                    several.join(", ")
                )));
            }
        };
        let value = map.remove(key).unwrap_or(Value::Null);
        let recipe = Recipe::from_key_value(key, value)
            .map_err(|e| D::Error::custom(format!("{key}: {e}")))?;
        Ok(Self {
            no_persist,
            recipe,
            extra: map,
        })
    }
}

/// The conversion a [`ConvertRequest`] runs (blobext-03 §8).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipe {
    /// `imageConvert` (§8.1).
    ImageConvert(ImageConvertRecipe),
    /// `archive` (§8.2).
    Archive(ArchiveRecipe),
    /// `extract` (§8.4).
    Extract(ExtractRecipe),
    /// `compress` (§8.5).
    Compress(CompressRecipe),
    /// `decompress` (§8.6).
    Decompress(DecompressRecipe),
    /// `delta` (§8.7).
    Delta(DeltaRecipe),
    /// `patch` (§8.8).
    Patch(PatchRecipe),
}

impl Recipe {
    /// The seven recipe property names, in §8 order.
    pub const KEYS: [&'static str; 7] = [
        "imageConvert",
        "archive",
        "extract",
        "compress",
        "decompress",
        "delta",
        "patch",
    ];

    /// This recipe's property name in a conversion request.
    pub fn key(&self) -> &'static str {
        match self {
            Self::ImageConvert(_) => "imageConvert",
            Self::Archive(_) => "archive",
            Self::Extract(_) => "extract",
            Self::Compress(_) => "compress",
            Self::Decompress(_) => "decompress",
            Self::Delta(_) => "delta",
            Self::Patch(_) => "patch",
        }
    }

    /// Parse the value of recipe property `key`.
    ///
    /// # Errors
    ///
    /// A serde error when `key` is not one of [`KEYS`](Self::KEYS) or the
    /// value does not match that recipe.
    pub fn from_key_value(key: &str, value: Value) -> Result<Self, serde_json::Error> {
        use serde::de::Error as _;
        Ok(match key {
            "imageConvert" => Self::ImageConvert(serde_json::from_value(value)?),
            "archive" => Self::Archive(serde_json::from_value(value)?),
            "extract" => Self::Extract(serde_json::from_value(value)?),
            "compress" => Self::Compress(serde_json::from_value(value)?),
            "decompress" => Self::Decompress(serde_json::from_value(value)?),
            "delta" => Self::Delta(serde_json::from_value(value)?),
            "patch" => Self::Patch(serde_json::from_value(value)?),
            other => {
                return Err(serde_json::Error::custom(format!(
                    "unknown recipe {other:?}"
                )));
            }
        })
    }
}

/// Converts an image to another format or size (blobext-03 §8.1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageConvertRecipe {
    /// `blobId`: the source image.
    pub blob_id: BlobRef,
    /// `type`: the media type to create.
    #[serde(rename = "type")]
    pub media_type: String,
    /// `width`: maximum width in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u64>,
    /// `height`: maximum height in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u64>,
    /// `ignoreAspect`: resize to exactly `width` × `height`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_aspect: Option<bool>,
    /// `quality`: 1 to 100, for lossy formats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u64>,
    /// `colorSpace`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_space: Option<ColorSpace>,
    /// `background`: CSS-style hex fill colour, e.g. `#ffffff`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// `stripMetadata`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_metadata: Option<bool>,
    /// `autoOrient`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_orient: Option<bool>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl ImageConvertRecipe {
    /// Convert `blob_id` to `media_type`, every option unset.
    pub fn new(blob_id: BlobRef, media_type: impl Into<String>) -> Self {
        Self {
            blob_id,
            media_type: media_type.into(),
            width: None,
            height: None,
            ignore_aspect: None,
            quality: None,
            color_space: None,
            background: None,
            strip_metadata: None,
            auto_orient: None,
            extra: Map::new(),
        }
    }
}

/// Output colour space of an [`ImageConvertRecipe`] (blobext-03 §8.1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    /// `sRGB`.
    Srgb,
    /// `grayscale`.
    Grayscale,
    /// A value blobext-03 does not define.
    Other(String),
}

impl_string_enum!(ColorSpace, "a colour space name",
    "sRGB" => Srgb,
    "grayscale" => Grayscale,
);

/// Creates an archive from a list of entries (blobext-03 §8.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRecipe {
    /// `type`: the archive media type.
    #[serde(rename = "type")]
    pub media_type: String,
    /// `entries`.
    pub entries: Vec<ArchiveEntry>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl ArchiveRecipe {
    /// An archive of `media_type` holding `entries`.
    pub fn new(media_type: impl Into<String>, entries: Vec<ArchiveEntry>) -> Self {
        Self {
            media_type: media_type.into(),
            entries,
            extra: Map::new(),
        }
    }
}

/// One entry of an archive (blobext-03 §8.3), as [`ArchiveRecipe`] input
/// and in an [`ExtractRecipe`] result.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    /// `name`: the path in the archive; directories end with `/`.
    pub name: String,
    /// `blobId`: the content; required for files, null otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<BlobRef>,
    /// `entryType` (null = `file`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_type: Option<EntryType>,
    /// `modified` (null = now).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<UTCDate>,
    /// `created`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<UTCDate>,
    /// `linkTarget`: required for symlinks and hardlinks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
    /// `mode`: octal permission string, e.g. `"0644"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// `uid`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u64>,
    /// `gid`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gid: Option<u64>,
    /// `ownerName`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_name: Option<String>,
    /// `groupName`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    /// `devMajor`, for device entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_major: Option<u64>,
    /// `devMinor`, for device entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_minor: Option<u64>,
    /// `comment`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// `compressionMethod`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression_method: Option<CompressionMethod>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl ArchiveEntry {
    /// An entry named `name` with every other property unset.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            blob_id: None,
            entry_type: None,
            modified: None,
            created: None,
            link_target: None,
            mode: None,
            uid: None,
            gid: None,
            owner_name: None,
            group_name: None,
            dev_major: None,
            dev_minor: None,
            comment: None,
            compression_method: None,
            extra: Map::new(),
        }
    }

    /// A file entry named `name` with content `blob_id`.
    pub fn file(name: impl Into<String>, blob_id: BlobRef) -> Self {
        let mut e = Self::new(name);
        e.blob_id = Some(blob_id);
        e
    }
}

/// The type of an [`ArchiveEntry`] (blobext-03 §8.3 `entryType`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EntryType {
    /// `file`: a regular file (the default).
    File,
    /// `directory`.
    Directory,
    /// `symlink`.
    Symlink,
    /// `hardlink`.
    Hardlink,
    /// `fifo`: a named pipe.
    Fifo,
    /// `blockDevice`.
    BlockDevice,
    /// `charDevice`.
    CharDevice,
    /// A value blobext-03 does not define.
    Other(String),
}

impl_string_enum!(EntryType, "an archive entry type",
    "file" => File,
    "directory" => Directory,
    "symlink" => Symlink,
    "hardlink" => Hardlink,
    "fifo" => Fifo,
    "blockDevice" => BlockDevice,
    "charDevice" => CharDevice,
);

/// Per-entry compression (blobext-03 §8.3 `compressionMethod`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CompressionMethod {
    /// `store`: no compression.
    Store,
    /// `deflate`.
    Deflate,
    /// A value blobext-03 does not define.
    Other(String),
}

impl_string_enum!(CompressionMethod, "an archive compression method",
    "store" => Store,
    "deflate" => Deflate,
);

/// Lists the entries of an archive (blobext-03 §8.4). Creates no blob:
/// the result's id, type and size are the archive's.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractRecipe {
    /// `blobId`: the archive.
    pub blob_id: BlobRef,
    /// `type`: the archive format, or `None` to auto-detect.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl ExtractRecipe {
    /// Extract `blob_id`, as `media_type` or auto-detected.
    pub fn new(blob_id: BlobRef, media_type: Option<String>) -> Self {
        Self {
            blob_id,
            media_type,
            extra: Map::new(),
        }
    }
}

/// Compresses a blob (blobext-03 §8.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressRecipe {
    /// `blobId`: the data.
    pub blob_id: BlobRef,
    /// `type`: the compression media type.
    #[serde(rename = "type")]
    pub media_type: String,
    /// `level`; see [`compress_level_range`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u64>,
    /// `checksum`: include an integrity checksum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<bool>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl CompressRecipe {
    /// Compress `blob_id` as `media_type` with default options.
    pub fn new(blob_id: BlobRef, media_type: impl Into<String>) -> Self {
        Self {
            blob_id,
            media_type: media_type.into(),
            level: None,
            checksum: None,
            extra: Map::new(),
        }
    }
}

/// Decompresses a blob (blobext-03 §8.6).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecompressRecipe {
    /// `blobId`: the compressed data.
    pub blob_id: BlobRef,
    /// `type`: the compression format, or `None` to auto-detect.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl DecompressRecipe {
    /// Decompress `blob_id`, as `media_type` or auto-detected.
    pub fn new(blob_id: BlobRef, media_type: Option<String>) -> Self {
        Self {
            blob_id,
            media_type,
            extra: Map::new(),
        }
    }
}

/// Computes a delta between two blobs (blobext-03 §8.7).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaRecipe {
    /// `blobId`: the base blob.
    pub blob_id: BlobRef,
    /// `newBlobId`: the blob to compare against.
    pub new_blob_id: BlobRef,
    /// `type`: the delta format.
    #[serde(rename = "type")]
    pub media_type: String,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl DeltaRecipe {
    /// The `media_type` delta from `blob_id` to `new_blob_id`.
    pub fn new(blob_id: BlobRef, new_blob_id: BlobRef, media_type: impl Into<String>) -> Self {
        Self {
            blob_id,
            new_blob_id,
            media_type: media_type.into(),
            extra: Map::new(),
        }
    }
}

/// Applies a delta to a base blob (blobext-03 §8.8).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchRecipe {
    /// `blobId`: the base blob.
    pub blob_id: BlobRef,
    /// `deltaBlobId`: the delta.
    pub delta_blob_id: BlobRef,
    /// `deltaType`: the delta format.
    pub delta_type: String,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl PatchRecipe {
    /// Apply delta `delta_blob_id` of `delta_type` to `blob_id`.
    pub fn new(blob_id: BlobRef, delta_blob_id: BlobRef, delta_type: impl Into<String>) -> Self {
        Self {
            blob_id,
            delta_blob_id,
            delta_type: delta_type.into(),
            extra: Map::new(),
        }
    }
}

/// A `created` entry of `Blob/convert` (blobext-03 §8): a BlobObject
/// plus the conversion-result properties.
///
/// `type` and `size` are always written (`size` null for a deferred
/// result); `expires` is tri-state as in [`BlobObject`](crate::BlobObject);
/// `isIncomplete` is written only when true; `entries` only for an
/// [`ExtractRecipe`] result.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertedBlob {
    /// `id`: the result blob (for extract, the archive).
    pub id: Id,
    /// `type`.
    #[serde(rename = "type", default)]
    pub media_type: Option<String>,
    /// `size`, or `None` while a deferred result is not generated.
    #[serde(default)]
    pub size: Option<u64>,
    /// `expires` (§7): absent, `null`, or a date.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires: Option<Option<UTCDate>>,
    /// `isIncomplete`: the conversion did not complete cleanly.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_incomplete: bool,
    /// `description` of the problem, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `entries` of an extracted archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<ArchiveEntry>>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl ConvertedBlob {
    /// A result with the given id, type and size; nothing else set.
    pub fn new(id: Id, media_type: Option<String>, size: Option<u64>) -> Self {
        Self {
            id,
            media_type,
            size,
            expires: None,
            is_incomplete: false,
            description: None,
            entries: None,
            extra: Map::new(),
        }
    }
}

/// `Blob/convert` response (blobext-03 §8): `created` and `notCreated`.
/// Empty maps are written as `null`; `null` or absent read as empty.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobConvertResponse {
    /// `accountId`.
    pub account_id: Id,
    /// `created`: creation id → result.
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub created: BTreeMap<String, ConvertedBlob>,
    /// `notCreated`: creation id → error.
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub not_created: BTreeMap<String, SetError>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobConvertResponse {
    /// An empty response for `account_id`.
    pub fn new(account_id: Id) -> Self {
        Self {
            account_id,
            created: BTreeMap::new(),
            not_created: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

/// The valid compression `level` range of a [`CompressRecipe`] format
/// (blobext-03 §8.5.1 to §8.5.4, informative): gzip and x-bzip2 1..=9,
/// x-xz 0..=9, zstd 1..=22. `None` for other formats. The media type is
/// compared case-insensitively.
///
/// "If the requested level is outside the valid range, the server SHOULD
/// use the nearest valid value" (§8.5).
pub fn compress_level_range(media_type: &str) -> Option<RangeInclusive<u64>> {
    const RANGES: [(&str, u64, u64); 4] = [
        ("application/gzip", 1, 9),
        ("application/x-bzip2", 1, 9),
        ("application/x-xz", 0, 9),
        ("application/zstd", 1, 22),
    ];
    RANGES
        .iter()
        .find(|(t, _, _)| t.eq_ignore_ascii_case(media_type))
        .map(|&(_, lo, hi)| lo..=hi)
}
