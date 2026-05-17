//! Blob/lookup and Blob/convert — urn:ietf:params:jmap:blob2
//!
//! Spec: draft-ietf-jmap-blobext-01 §6 (Blob/lookup), §8 (Blob/convert)
//!
//! These methods use the blob2 capability, NOT USING_CHAT.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use jmap_types::Id;

/// Capability URIs for Blob extension method calls.
const USING_BLOB: &[&str] = &["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:blob2"];

/// A single entry in a `Blob/lookup` response.
///
/// Spec: draft-ietf-jmap-blobext-01 §6
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobLookupEntry {
    /// The blobId that was queried.
    pub id: Id,
    /// Per-type reverse lookup: keys are JMAP data type names (e.g.
    /// `"Message"`, `"Chat"`); values are the typed [`Id`]s of objects
    /// that reference this blob. The keys stay `String` because data
    /// type names are externally-defined spec strings and not
    /// uniformly enumerable across extensions.
    pub matched_ids: HashMap<String, Vec<Id>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response to a `Blob/lookup` call.
///
/// Spec: draft-ietf-jmap-blobext-01 §6
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobLookupResponse {
    /// Account the query was run against.
    pub account_id: Id,
    /// One entry per queried blobId.
    pub list: Vec<BlobLookupEntry>,
    /// blobIds that were not found or not accessible (access-control safe).
    /// An absent field and an empty array are semantically identical.
    #[serde(default)]
    pub not_found: Vec<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A blob object returned in a `Blob/convert` (or `Blob/set`) response.
///
/// Spec: draft-ietf-jmap-blobext-01 §4 (BlobObject properties)
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobObject {
    /// The blobId of the created blob.
    pub id: Id,
    /// MIME type of the blob, or `None` if the server did not report it.
    #[serde(rename = "type")]
    pub content_type: Option<String>,
    /// Size in octets.  `None` if the server deferred generation and does
    /// not yet know the final size (spec §8 allows omitting size for
    /// deferred conversions).
    pub size: Option<u64>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response to a `Blob/convert` call.
///
/// Spec: draft-ietf-jmap-blobext-01 §8 — response has the same structure as
/// Blob/set: a `created` map of creation id → `BlobObject` for each
/// successful conversion, and a `notCreated` map for failures.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobConvertResponse {
    /// Account the conversion was run against.
    pub account_id: Id,
    /// Successful conversions: maps each creation id to the resulting blob.
    #[serde(default)]
    pub created: Option<HashMap<String, BlobObject>>,
    /// Failed conversions: maps each creation id to a SetError.
    #[serde(default)]
    pub not_created: Option<HashMap<String, super::SetError>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// `imageConvert` recipe for a `Blob/convert` request.
///
/// Spec: draft-ietf-jmap-blobext-01 §8.1 (ImageConvertRecipe)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImageConvertRecipe<'a> {
    blob_id: &'a Id,
    #[serde(rename = "type")]
    content_type: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
}

/// One entry in the `create` map of a `Blob/convert` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BlobConvertCreate<'a> {
    image_convert: ImageConvertRecipe<'a>,
}

impl super::SessionClient {
    /// Reverse-lookup blobs: given a list of blob IDs and data type names,
    /// returns which objects of those types reference each blob.
    ///
    /// Uses capability `urn:ietf:params:jmap:blob2`; the server MUST advertise
    /// it in the Session for this method to succeed (RFC 8620 §3.3).
    ///
    /// `type_names` filters which data types to search. `None` queries all
    /// types registered on the server. For JMAP Chat, `"Message"` is the
    /// expected type.
    ///
    /// Security: blobs that are inaccessible or nonexistent are returned with
    /// empty `matchedIds` arrays rather than an error (draft-ietf-jmap-blobext
    /// §6), to avoid information leakage.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `blob_ids` is empty (caller-precondition guard — a no-op
    ///   lookup is never useful).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call):
    ///   [`Http`](jmap_base_client::ClientError::Http),
    ///   [`Parse`](jmap_base_client::ClientError::Parse),
    ///   [`AuthFailed`](jmap_base_client::ClientError::AuthFailed),
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError)
    ///   (wraps RFC 8620 §3.6.2 method-level errors; servers that do
    ///   not advertise `urn:ietf:params:jmap:blob2` return
    ///   `unknownCapability` here),
    ///   [`MethodNotFound`](jmap_base_client::ClientError::MethodNotFound),
    ///   [`ResponseTooLarge`](jmap_base_client::ClientError::ResponseTooLarge),
    ///   or
    ///   [`UnexpectedResponse`](jmap_base_client::ClientError::UnexpectedResponse).
    pub async fn blob_lookup(
        &self,
        blob_ids: &[Id],
        type_names: Option<&[&str]>,
    ) -> Result<BlobLookupResponse, jmap_base_client::ClientError> {
        if blob_ids.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "blob_lookup: blob_ids may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "ids": blob_ids,
            "typeNames": type_names,
        });
        let req = super::build_request("Blob/lookup", args, USING_BLOB);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Convert a blob to a different MIME type via an `imageConvert` recipe
    /// (JMAP-BLOBEXT §8 / blob2 capability).
    ///
    /// Typical use: request a thumbnail (`image/webp`) from an image blob
    /// without downloading the original. The server MUST advertise
    /// `urn:ietf:params:jmap:blob2` in Session capabilities.
    ///
    /// `width` and `height` are optional maximum-dimension hints; the server
    /// may ignore or clamp them. Pass `None` to omit.
    ///
    /// On success the converted blob is in
    /// `response.created[CALL_ID]`.  On failure the error is in
    /// `response.not_created[CALL_ID]`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `content_type` is empty (caller-precondition guard — a
    ///   conversion target without a MIME type is meaningless).
    /// - [`ClientError::Parse`](jmap_base_client::ClientError::Parse) if
    ///   serializing the typed `ImageConvertRecipe` fails (pathological
    ///   conditions only).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::blob_lookup`]. Per-creation
    ///   failures (e.g. `invalidArguments`, `unsupportedMediaType`)
    ///   appear in [`BlobConvertResponse::not_created`] rather than
    ///   as [`Err`].
    pub async fn blob_convert(
        &self,
        from_blob_id: &Id,
        content_type: &str,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<BlobConvertResponse, jmap_base_client::ClientError> {
        if content_type.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "blob_convert: content_type may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let create_entry = BlobConvertCreate {
            image_convert: ImageConvertRecipe {
                blob_id: from_blob_id,
                content_type,
                width,
                height,
            },
        };
        let create_map = {
            let mut m = serde_json::Map::new();
            m.insert(
                super::CALL_ID.to_owned(),
                serde_json::to_value(&create_entry)
                    .map_err(jmap_base_client::ClientError::Parse)?,
            );
            m
        };
        let args = serde_json::json!({
            "accountId": account_id,
            "create": serde_json::Value::Object(create_map),
        });
        let req = super::build_request("Blob/convert", args, USING_BLOB);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: the `create` map serialises to the correct wire format.
    ///
    /// Expected JSON is hand-derived from draft-ietf-jmap-blobext-01 §8.1.
    /// We do NOT use blob_convert() itself as the oracle — that would make the
    /// test circular.
    #[test]
    fn image_convert_recipe_wire_format() {
        let blob_id = Id::from("Bxxx");
        let recipe = ImageConvertRecipe {
            blob_id: &blob_id,
            content_type: "image/webp",
            width: Some(100),
            height: None,
        };
        let entry = BlobConvertCreate {
            image_convert: recipe,
        };
        let v = serde_json::to_value(&entry).expect("serialization must not fail");
        let ic = v.get("imageConvert").expect("must have imageConvert key");
        assert_eq!(ic.get("blobId").and_then(|v| v.as_str()), Some("Bxxx"));
        assert_eq!(ic.get("type").and_then(|v| v.as_str()), Some("image/webp"));
        assert_eq!(ic.get("width").and_then(|v| v.as_u64()), Some(100));
        // height must be absent when None (skip_serializing_if)
        assert!(
            ic.get("height").is_none(),
            "height must be omitted when None"
        );
    }

    /// Oracle: `BlobConvertResponse` deserialises the spec-shaped response correctly.
    ///
    /// Expected JSON is hand-written from the draft-ietf-jmap-blobext-01 §8 example.
    #[test]
    fn blob_convert_response_deserialise() {
        let json = serde_json::json!({
            "accountId": "abc",
            "created": {
                "r1": {
                    "id": "Bnew",
                    "type": "image/webp",
                    "size": 12345
                }
            },
            "notCreated": {}
        });
        let resp: BlobConvertResponse =
            serde_json::from_value(json).expect("deserialisation must not fail");
        assert_eq!(resp.account_id, "abc");
        let created = resp.created.expect("created must be Some");
        let obj = created.get("r1").expect("r1 key must be present");
        assert_eq!(obj.id.as_ref(), "Bnew");
        assert_eq!(obj.content_type.as_deref(), Some("image/webp"));
        assert_eq!(obj.size, Some(12345));
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // Each test deserialises wire JSON containing a synthetic `acmeCorp*`
    // vendor field and asserts it survives in `extra`. The vendor field
    // names cannot collide with any field defined in
    // draft-ietf-jmap-blobext-01 §4, §6, or §8, so the tests are
    // independent of the code under test (workspace test-integrity rule).

    /// `BlobLookupEntry.extra` captures unknown fields on deserialize.
    #[test]
    fn blob_lookup_entry_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "B1",
            "matchedIds": {
                "Message": ["M1", "M2"]
            },
            "acmeCorpCacheHit": true
        });
        let obj: BlobLookupEntry =
            serde_json::from_value(raw).expect("BlobLookupEntry must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpCacheHit").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    /// `BlobLookupResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn blob_lookup_response_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "acc1",
            "list": [],
            "acmeCorpRequestId": "req-42"
        });
        let obj: BlobLookupResponse =
            serde_json::from_value(raw).expect("BlobLookupResponse must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpRequestId").and_then(|v| v.as_str()),
            Some("req-42")
        );
    }

    /// `BlobObject.extra` captures unknown fields on deserialize.
    #[test]
    fn blob_object_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "Bnew",
            "type": "image/webp",
            "size": 12345,
            "acmeCorpCdnUrl": "https://cdn.example.com/Bnew"
        });
        let obj: BlobObject = serde_json::from_value(raw).expect("BlobObject must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpCdnUrl").and_then(|v| v.as_str()),
            Some("https://cdn.example.com/Bnew")
        );
    }

    /// `BlobConvertResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn blob_convert_response_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "acc1",
            "created": {},
            "notCreated": {},
            "acmeCorpJobId": "job-7"
        });
        let obj: BlobConvertResponse =
            serde_json::from_value(raw).expect("BlobConvertResponse must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpJobId").and_then(|v| v.as_str()),
            Some("job-7")
        );
    }
}
