//! RFC 9404 `Blob/upload` (§4.1), served under `urn:ietf:params:jmap:blob`
//! (docs/design/DECISIONS.md decision 11).
//!
//! RFC 9404 `Blob/get` and `Blob/lookup` use the blobext types
//! ([`BlobGetArgs`](crate::BlobGetArgs),
//! [`BlobLookupArgs`](crate::BlobLookupArgs)): their RFC 9404 forms are
//! subsets. The one RFC 9404 response shape that is not a subset is the
//! `Blob/lookup` response, whose §4.3 has no `accountId`
//! ([`Rfc9404LookupResponse`]).

use std::collections::BTreeMap;

use jmap_types::{Id, SetError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::lookup::BlobInfo;
use crate::serde_util::{empty_as_null, null_as_empty};
use crate::set::BlobObject;
use crate::source::DataSourceObject;

/// `Blob/upload` arguments (RFC 9404 §4.1).
///
/// "blobs cannot be updated or deleted, so only create is allowed": a
/// server answers a non-empty `update` or `destroy` (kept in
/// [`extra`](Self::extra)) with `invalidArguments` (decision 21(iv)).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobUploadArgs {
    /// `accountId`.
    pub account_id: Id,
    /// `create`: creation id → UploadObject.
    pub create: BTreeMap<Id, UploadObject>,
    /// Other arguments, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobUploadArgs {
    /// Upload `create` into `account_id`.
    pub fn new(account_id: Id, create: BTreeMap<Id, UploadObject>) -> Self {
        Self {
            account_id,
            create,
            extra: Map::new(),
        }
    }
}

/// An UploadObject (RFC 9404 §4.1): sources and a media type hint only.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadObject {
    /// `data`: "An array of zero or more octet sources in order (zero to
    /// create an empty blob)."
    pub data: Vec<DataSourceObject>,
    /// `type`: "A hint for media type of the data."
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl UploadObject {
    /// An upload of `data` with no type hint.
    pub fn new(data: Vec<DataSourceObject>) -> Self {
        Self {
            data,
            media_type: None,
            extra: Map::new(),
        }
    }
}

/// `Blob/upload` response (RFC 9404 §4.1).
///
/// `accountId`, `created` and `notCreated` only: blobs have no state, and
/// the §4.1 text ("there is no state field present in the method
/// response") wins over the `oldState: null` in the §4.1.2 and §4.2.2
/// examples (decision 21(iii)); such members land in
/// [`extra`](Self::extra). Every `created` entry has a `size`. Empty maps
/// are written as `null`; `null` or absent ones read as empty.
///
/// Named apart from the base client's `BlobUploadResponse`, which is the
/// RFC 8620 §6.1 upload endpoint's HTTP response.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobUploadCallResponse {
    /// `accountId`.
    pub account_id: Id,
    /// `created`: creation id → created blob.
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub created: BTreeMap<String, BlobObject>,
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

impl BlobUploadCallResponse {
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

/// `Blob/lookup` response as RFC 9404 §4.3 defines it: `list` only.
///
/// §4.3 lists no `accountId` in the response (its §4.3.1 example has none),
/// so it is optional here; a server that sends it, as this workspace's
/// does, fills it. There is no `notFound`: §4.3 has an unknown or invisible
/// blob listed with empty arrays, so the §4.3.1 example's `notFound`
/// (decision 21(ii)) lands in [`extra`](Self::extra).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rfc9404LookupResponse {
    /// `accountId`, when the server sends it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<Id>,
    /// `list`: one BlobInfo per requested blobId.
    pub list: Vec<BlobInfo>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl Rfc9404LookupResponse {
    /// A response with `list` and no `accountId`.
    pub fn new(list: Vec<BlobInfo>) -> Self {
        Self {
            account_id: None,
            list,
            extra: Map::new(),
        }
    }
}
