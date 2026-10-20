//! draft-atwood-jmap-scene-00 §Capability — capability URI and the
//! account-level capability object.

use serde::{Deserialize, Serialize};

/// Capability URI for JMAP Scene (draft-atwood-jmap-scene-00 §Capability).
///
/// "Servers supporting this specification MUST advertise the
/// `urn:ietf:params:jmap:scene` capability in the JMAP Session object." The
/// session-level value is an empty object `{}` and is not modelled; the
/// account-level value is [`SceneCapability`].
pub const JMAP_SCENE_URI: &str = "urn:ietf:params:jmap:scene";

/// Account-level capability object for `"urn:ietf:params:jmap:scene"`.
///
/// Found at `accounts[id].accountCapabilities["urn:ietf:params:jmap:scene"]`.
///
/// Spec: draft-atwood-jmap-scene-00 §Account-Level Capability Object.
///
/// `supportedVisualTypes` "MUST include at least `\"model/gltf-binary\"`";
/// this is documented, not enforced on deserialize.
///
/// # Example
///
/// ```
/// use jmap_scene_types::SceneCapability;
///
/// let cap: SceneCapability = serde_json::from_str(r#"{
///     "mayCreateRegion": false,
///     "mayCreateObject": true,
///     "supportedVisualTypes": ["model/gltf-binary"],
///     "maxRegionsPerAccount": 10,
///     "maxObjectsPerRegion": null,
///     "maxAvatarsPerRegion": 100,
///     "maxAssetSizeBytes": null
/// }"#)?;
/// assert_eq!(cap.max_avatars_per_region, Some(100));
/// assert_eq!(cap.max_objects_per_region, None);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneCapability {
    /// `true` if the authenticated user may create new SceneRegion
    /// objects.
    pub may_create_region: bool,
    /// `true` if the authenticated user may create new SceneObject
    /// objects.
    pub may_create_object: bool,
    /// Media types the deployment supports for visual representations.
    pub supported_visual_types: Vec<String>,
    /// Maximum regions per account; `None` (`null`) means no
    /// server-imposed limit.
    pub max_regions_per_account: Option<u64>,
    /// Maximum objects per region; `None` (`null`) means no limit.
    pub max_objects_per_region: Option<u64>,
    /// Maximum concurrent avatars per region; `None` (`null`) means no
    /// limit.
    pub max_avatars_per_region: Option<u64>,
    /// Maximum asset file size in bytes; `None` (`null`) means no limit.
    pub max_asset_size_bytes: Option<u64>,
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
    /// L160-181.
    fn sample() -> serde_json::Value {
        json!({
            "mayCreateRegion": true,
            "mayCreateObject": true,
            "supportedVisualTypes": ["model/gltf-binary", "model/gltf+json"],
            "maxRegionsPerAccount": null,
            "maxObjectsPerRegion": 5000,
            "maxAvatarsPerRegion": null,
            "maxAssetSizeBytes": 52428800
        })
    }

    #[test]
    fn capability_uri_matches_draft() {
        // Oracle: L152 "MUST advertise the `urn:ietf:params:jmap:scene`
        // capability in the JMAP Session object."
        assert_eq!(JMAP_SCENE_URI, "urn:ietf:params:jmap:scene");
    }

    #[test]
    fn account_capability_round_trips() {
        let raw = sample();
        let cap: SceneCapability = serde_json::from_value(raw.clone()).unwrap();
        assert!(cap.may_create_region);
        assert_eq!(
            cap.supported_visual_types,
            ["model/gltf-binary", "model/gltf+json"]
        );
        assert_eq!(cap.max_objects_per_region, Some(5000));
        assert_eq!(cap.max_asset_size_bytes, Some(52_428_800));
        assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
    }

    #[test]
    fn null_limits_serialize_as_null() {
        // Oracle: L171-172 maxRegionsPerAccount "(UnsignedInt|null) ...
        // `null` means no server-imposed limit": a present property with a
        // `null` value.
        let cap: SceneCapability = serde_json::from_value(sample()).unwrap();
        assert_eq!(cap.max_regions_per_account, None);
        let out = serde_json::to_value(&cap).unwrap();
        assert_eq!(out["maxRegionsPerAccount"], json!(null));
        assert_eq!(out["maxAvatarsPerRegion"], json!(null));
    }

    #[test]
    fn limits_reject_negative_values() {
        // Oracle: L171-181 type every limit "UnsignedInt|null".
        let mut raw = sample();
        raw["maxObjectsPerRegion"] = json!(-1);
        assert!(serde_json::from_value::<SceneCapability>(raw).is_err());
    }

    #[test]
    fn required_fields_rejected_when_missing() {
        // Oracle: L162-169 list these fields without an "optional" marker.
        for field in ["mayCreateRegion", "mayCreateObject", "supportedVisualTypes"] {
            let mut raw = sample();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<SceneCapability>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = sample();
        raw["vendor.example:tickRate"] = json!(30);
        let cap: SceneCapability = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(cap.extra.get("vendor.example:tickRate"), Some(&json!(30)));
        assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
    }
}
