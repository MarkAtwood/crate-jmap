//! draft-atwood-jmap-scene-00 §SceneAsset.

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// Metadata about a visual or audio resource stored as a JMAP blob
/// (draft-atwood-jmap-scene-00 §SceneAsset).
///
/// `T|null` properties are `Option<T>` and serialize `None` as `null`.
/// `assetUri` is peer-supplied and untrusted; nothing here validates it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneAsset {
    /// A ULID assigned at creation.
    pub id: Id,
    /// The account that owns this asset.
    pub account_id: Id,
    /// The JMAP blob reference for the asset file.
    pub blob_id: Id,
    /// Deployment-provided URI for retrieving the asset, typically a CDN
    /// endpoint. "MUST be treated as untrusted."
    pub asset_uri: Option<String>,
    /// The media type of the asset, e.g. `"model/gltf-binary"`.
    pub media_type: String,
    /// Human-readable label.
    pub name: Option<String>,
    /// File size in bytes.
    pub size: u64,
    /// SHA-256 hash of the asset file, lowercase hexadecimal.
    pub sha256: Option<String>,
    /// Time the asset was created.
    pub created_at: UTCDate,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Hand-written from the field list of draft-atwood-jmap-scene-00
    /// L214-239, with the create values of L656-662 and the server-set
    /// values of L677-682.
    fn sample() -> serde_json::Value {
        json!({
            "id": "01J5AST0000000000000000001",
            "accountId": "acct-001",
            "blobId": "Gc0f032d390a5d5fa8a35",
            "assetUri": "https://cdn.example.com/assets/sculpture-001.glb",
            "mediaType": "model/gltf-binary",
            "name": "Sculpture: Convergence",
            "size": 2483712,
            "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "createdAt": "2026-06-06T10:05:00Z"
        })
    }

    #[test]
    fn asset_round_trips() {
        let raw = sample();
        let asset: SceneAsset = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(asset.blob_id, Id::from("Gc0f032d390a5d5fa8a35"));
        assert_eq!(asset.size, 2_483_712);
        assert_eq!(serde_json::to_value(&asset).unwrap(), raw);
    }

    #[test]
    fn nullable_fields_serialize_as_null() {
        // Oracle: L223 assetUri "(String|null)", L229 name "(String|null)",
        // L235 sha256 "(String|null)".
        let mut raw = sample();
        for key in ["assetUri", "name", "sha256"] {
            raw[key] = json!(null);
        }
        let asset: SceneAsset = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(asset.asset_uri, None);
        assert_eq!(serde_json::to_value(&asset).unwrap(), raw);
    }

    #[test]
    fn required_fields_rejected_when_missing() {
        // Oracle: L214-239 type these without "|null".
        for field in [
            "id",
            "accountId",
            "blobId",
            "mediaType",
            "size",
            "createdAt",
        ] {
            let mut raw = sample();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<SceneAsset>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = sample();
        raw["vendor.example:levels"] = json!([0, 1, 2]);
        let asset: SceneAsset = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&asset).unwrap(), raw);
    }
}
