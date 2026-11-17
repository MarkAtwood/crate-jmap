//! RFC 8620 §6.3 `Blob/copy`: core (`urn:ietf:params:jmap:core`),
//! unchanged by RFC 9404 and blobext-03 (docs/design/blobext.md B23).

use std::collections::BTreeMap;

use jmap_types::{Id, SetError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::serde_util::{empty_as_null, null_as_empty};

/// `Blob/copy` arguments (RFC 8620 §6.3).
///
/// Blobs are copied "*between* two different accounts", so `accountId`
/// must differ from `fromAccountId` (as for Foo/copy, §5.4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobCopyArgs {
    /// `fromAccountId`: "The id of the account to copy blobs from."
    pub from_account_id: Id,
    /// `accountId`: "The id of the account to copy blobs to."
    pub account_id: Id,
    /// `blobIds`: "A list of ids of blobs to copy to the other account."
    pub blob_ids: Vec<Id>,
    /// Other arguments, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobCopyArgs {
    /// Copy `blob_ids` from `from_account_id` to `account_id`.
    pub fn new(from_account_id: Id, account_id: Id, blob_ids: Vec<Id>) -> Self {
        Self {
            from_account_id,
            account_id,
            blob_ids,
            extra: Map::new(),
        }
    }
}

/// `Blob/copy` response (RFC 8620 §6.3).
///
/// Empty maps are written as `null` ("or null if none"); `null` or absent
/// ones read as empty.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobCopyResponse {
    /// `fromAccountId`: "The id of the account blobs were copied from."
    pub from_account_id: Id,
    /// `accountId`: "The id of the account blobs were copied to."
    pub account_id: Id,
    /// `copied`: "A map of the blobId in the fromAccount to the id for the
    /// blob in the account it was copied to".
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub copied: BTreeMap<String, Id>,
    /// `notCopied`: "A map of blobId to a SetError object for each blob
    /// that failed to be copied"; `notFound` for a blob that "cannot be
    /// found".
    #[serde(
        default,
        deserialize_with = "null_as_empty",
        serialize_with = "empty_as_null"
    )]
    pub not_copied: BTreeMap<String, SetError>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl BlobCopyResponse {
    /// An empty response for a copy from `from_account_id` to
    /// `account_id`.
    pub fn new(from_account_id: Id, account_id: Id) -> Self {
        Self {
            from_account_id,
            account_id,
            copied: BTreeMap::new(),
            not_copied: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// RFC 8620 §6.3 argument names; `null` maps read as empty.
    #[test]
    fn round_trips_the_rfc_8620_names() {
        let args: BlobCopyArgs = serde_json::from_value(json!({
            "fromAccountId": "A1", "accountId": "A2", "blobIds": ["b1", "b2"]
        }))
        .unwrap();
        assert_eq!(args.from_account_id, Id::from("A1"));
        assert_eq!(args.blob_ids, [Id::from("b1"), Id::from("b2")]);

        let resp: BlobCopyResponse = serde_json::from_value(json!({
            "fromAccountId": "A1", "accountId": "A2",
            "copied": {"b1": "c1"}, "notCopied": null
        }))
        .unwrap();
        assert_eq!(resp.copied["b1"], Id::from("c1"));
        assert!(resp.not_copied.is_empty());
        assert_eq!(
            serde_json::to_value(&resp).unwrap(),
            json!({"fromAccountId": "A1", "accountId": "A2",
                   "copied": {"b1": "c1"}, "notCopied": null})
        );
    }
}
