//! `Blob/lookup` (blobext-03 §6).

use std::collections::BTreeMap;

use jmap_types::Id;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `Blob/lookup` arguments (blobext-03 §6).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobLookupArgs {
    /// `accountId`.
    pub account_id: Id,
    /// `typeNames`: data types to search (JMAP Data Types registry names).
    pub type_names: Vec<String>,
    /// `ids`: the blobIds to look up.
    pub ids: Vec<Id>,
    /// Other arguments, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobLookupArgs {
    /// Look up `ids` among objects of `type_names` in `account_id`.
    pub fn new(account_id: Id, type_names: Vec<String>, ids: Vec<Id>) -> Self {
        Self {
            account_id,
            type_names,
            ids,
            extra: Map::new(),
        }
    }
}

/// One `list` entry of a `Blob/lookup` response (blobext-03 §6
/// BlobInfo).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobInfo {
    /// `id`: the blobId queried.
    pub id: Id,
    /// `matchedIds`: every requested type name → ids of objects of that
    /// type referencing the blob (empty when none, or when the blob is
    /// not visible).
    pub matched_ids: BTreeMap<String, Vec<Id>>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobInfo {
    /// An entry for `id` with the given matches.
    pub fn new(id: Id, matched_ids: BTreeMap<String, Vec<Id>>) -> Self {
        Self {
            id,
            matched_ids,
            extra: Map::new(),
        }
    }
}

/// `Blob/lookup` response (blobext-03 §6).
///
/// There is no `notFound`: an unknown or invisible blob gets an entry
/// with empty arrays, so the response does not reveal whether it exists.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobLookupResponse {
    /// `accountId`.
    pub account_id: Id,
    /// `list`: one entry per requested blobId, in request order.
    pub list: Vec<BlobInfo>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobLookupResponse {
    /// A response for `account_id` with `list`.
    pub fn new(account_id: Id, list: Vec<BlobInfo>) -> Self {
        Self {
            account_id,
            list,
            extra: Map::new(),
        }
    }
}
