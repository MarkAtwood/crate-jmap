//! `Blob/set` (blobext-03 §4) and the `expires` property (§7).

use std::collections::BTreeMap;

use jmap_types::{Id, JmapObject, PatchObject, SetError, SetObject, State, UTCDate};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::blob_ref::BlobRef;
use crate::digest::DigestAlgorithm;
use crate::serde_util::{empty_as_null, merge_digests, null_as_empty, present, take_digests};
use crate::source::DataSourceObject;

/// A blob to create in `Blob/set` (blobext-03 §4 BlobCreateObject).
///
/// Top-level `digest:<alg>` members (digests of the whole new blob) go to
/// [`digests`](Self::digests); other unknown members to
/// [`extra`](Self::extra).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlobCreateObject {
    /// `data`: the sources, concatenated in order. May be empty.
    pub data: Vec<DataSourceObject>,
    /// `type`: a media type hint.
    pub media_type: Option<String>,
    /// `size`: the expected size of the new blob; the server rejects a
    /// mismatch with `invalidProperties`.
    pub size: Option<u64>,
    /// `digest:<alg>`: expected digests of the new blob; each one is
    /// checked.
    pub digests: BTreeMap<DigestAlgorithm, String>,
    /// `noPersist`: the blob need only last for the request.
    pub no_persist: bool,
    /// Unknown members, kept for round-trip.
    pub extra: Map<String, Value>,
}

impl BlobCreateObject {
    /// A create from `data` with every optional property unset.
    pub fn new(data: Vec<DataSourceObject>) -> Self {
        Self {
            data,
            ..Self::default()
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRaw<T> {
    data: T,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    no_persist: bool,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

impl Serialize for BlobCreateObject {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        CreateRaw {
            data: &self.data,
            media_type: self.media_type.clone(),
            size: self.size,
            no_persist: self.no_persist,
            rest: merge_digests(&self.digests, &self.extra),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for BlobCreateObject {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut raw = CreateRaw::<Vec<DataSourceObject>>::deserialize(d)?;
        let digests = take_digests(&mut raw.rest).map_err(serde::de::Error::custom)?;
        Ok(Self {
            data: raw.data,
            media_type: raw.media_type,
            size: raw.size,
            digests,
            no_persist: raw.no_persist,
            extra: raw.rest,
        })
    }
}

/// A blob as reported in `Blob/set` `created` (blobext-03 §4 BlobObject,
/// §7 `expires`).
///
/// `type` and `size` are always written, `null` when `None`; a `null`
/// `size` is a deferred `Blob/convert` result whose size is not yet known
/// (§8). `expires` is tri-state: `None` leaves it out, `Some(None)` writes
/// `null` (no specific expiry), `Some(Some(_))` a date.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobObject {
    /// `id`: the blobId.
    pub id: Id,
    /// `type`: the media type, or `None` if unknown.
    #[serde(rename = "type", default)]
    pub media_type: Option<String>,
    /// `size` in octets, or `None` for a deferred conversion result.
    #[serde(default)]
    pub size: Option<u64>,
    /// `expires` (§7): absent, `null`, or a date.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires: Option<Option<UTCDate>>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobObject {
    /// A blob with the given id, type and size, and no `expires`.
    pub fn new(id: Id, media_type: Option<String>, size: Option<u64>) -> Self {
        Self {
            id,
            media_type,
            size,
            expires: None,
            extra: Map::new(),
        }
    }
}

/// `Blob` is a Foo/set type (blobext-03 §4), so Blob/set can opt in to
/// draft-ietf-jmap-conditional-02 (docs/design/DECISIONS.md decisions 1
/// and 17). `id` and `size` are server-set; `type` is given only on create
/// and `expires` only through create and update.
///
/// Blob/get never returns `type` or `expires`, so an `ifUnchangedBy`
/// pointer to either is `invalidArguments` (decision 19). Top-level
/// `digest:<alg>` names are open-ended and come from Blob/get.
impl JmapObject for BlobObject {
    const TYPE_NAME: &'static str = "Blob";
    type Property = ();
}

impl SetObject for BlobObject {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "size"];
    const CLIENT_SETTABLE: &'static [&'static str] = &["type", "expires"];
    const IMMUTABLE: &'static [&'static str] = &["type"];
}

/// An `updated` entry of `Blob/set` (blobext-03 §4): the server returns
/// the `expires` it applied when it differs from the requested one.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobUpdated {
    /// `expires` actually applied: absent, `null`, or a date.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires: Option<Option<UTCDate>>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobUpdated {
    /// An entry reporting the applied `expires`.
    pub fn with_expires(expires: Option<UTCDate>) -> Self {
        Self {
            expires: Some(expires),
            extra: Map::new(),
        }
    }
}

/// `Blob/set` arguments (blobext-03 §4).
///
/// `update` keys and `destroy` entries may be `#creationId` references
/// (RFC 8620 §5.3). Blobs have no state: a non-null `ifInState` is an
/// `invalidArguments` error.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobSetArgs {
    /// `accountId`.
    pub account_id: Id,
    /// `ifInState`: must be null or absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_in_state: Option<State>,
    /// `create`: creation id → blob to create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create: Option<BTreeMap<Id, BlobCreateObject>>,
    /// `update`: blob → patch; only `expires` may change ("touch").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<BTreeMap<BlobRef, PatchObject>>,
    /// `destroy`: blobs to destroy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destroy: Option<Vec<BlobRef>>,
    /// Other arguments (e.g. draft-ietf-jmap-conditional `ifUnchangedBy`),
    /// kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobSetArgs {
    /// Arguments for `account_id` with no create, update or destroy.
    pub fn new(account_id: Id) -> Self {
        Self {
            account_id,
            if_in_state: None,
            create: None,
            update: None,
            destroy: None,
            extra: Map::new(),
        }
    }
}

/// `Blob/set` response (blobext-03 §4).
///
/// The shape of `jmap_types::SetResponse` except that `oldState` and
/// `newState` are always null for blobs ("the response has oldState and
/// newState both null"); they are `Option` so that absent and null both
/// parse, and are written as `null`. Empty result maps are written as
/// `null`, and `null` or absent ones read as empty.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobSetResponse {
    /// `accountId`.
    pub account_id: Id,
    /// `oldState`: null for blobs.
    #[serde(default)]
    pub old_state: Option<State>,
    /// `newState`: null for blobs.
    #[serde(default)]
    pub new_state: Option<State>,
    /// `created`: creation id → created blob.
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub created: BTreeMap<String, BlobObject>,
    /// `updated`: blobId → `null` (requested `expires` honoured) or the
    /// applied `expires`.
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub updated: BTreeMap<Id, Option<BlobUpdated>>,
    /// `destroyed`: blobIds destroyed.
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub destroyed: Vec<Id>,
    /// `notCreated`: creation id → error.
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub not_created: BTreeMap<String, SetError>,
    /// `notUpdated`: blobId → error.
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub not_updated: BTreeMap<Id, SetError>,
    /// `notDestroyed`: blobId → error.
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub not_destroyed: BTreeMap<Id, SetError>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobSetResponse {
    /// An empty response for `account_id`: null states, no results.
    pub fn new(account_id: Id) -> Self {
        Self {
            account_id,
            old_state: None,
            new_state: None,
            created: BTreeMap::new(),
            updated: BTreeMap::new(),
            destroyed: Vec::new(),
            not_created: BTreeMap::new(),
            not_updated: BTreeMap::new(),
            not_destroyed: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}
