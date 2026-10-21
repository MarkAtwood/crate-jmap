//! draft-atwood-jmap-scene-00 §SceneAvatar.

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

use crate::{Orientation, Position};

/// A user's spatial presence within a SceneRegion
/// (draft-atwood-jmap-scene-00 §SceneAvatar).
///
/// `position` and `orientation` are the last known values, "Not a
/// real-time value". `T|null` properties are `Option<T>` and serialize
/// `None` as `null`. Not enforced on deserialize: `visualType` "MUST be
/// present when `visualRef` is non-null."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneAvatar {
    /// Server-assigned identifier: "A ULID assigned at creation, unique
    /// within the account like any JMAP id".
    pub id: Id,
    /// The SceneRegion this avatar is in.
    pub region_id: Id,
    /// The authenticated user identity string.
    pub user_id: String,
    /// Display name for this avatar.
    pub display_name: String,
    /// Last known position `[x, y, z]`.
    pub position: Position,
    /// Last known orientation as a unit quaternion `[x, y, z, w]`.
    pub orientation: Orientation,
    /// Blob reference for the avatar visual; `None` for the deployment
    /// default.
    pub visual_ref: Option<Id>,
    /// Media type of the avatar visual.
    pub visual_type: Option<String>,
    /// Time this avatar entered the region.
    pub joined_at: UTCDate,
    /// Time this avatar left the region; `None` while present.
    pub left_at: Option<UTCDate>,
    /// Deployment-defined avatar state, stored and relayed without
    /// interpretation.
    pub world_state: Option<serde_json::Map<String, serde_json::Value>>,
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

    /// An avatar that has left, hand-written from the field list of
    /// draft-atwood-jmap-scene-00 L550-581.
    fn departed() -> serde_json::Value {
        json!({
            "id": "user:bob@example.com",
            "regionId": "01J5ABC0000000000000000001",
            "userId": "user:bob@example.com",
            "displayName": "Bob",
            "position": [1.5, 0.0, 2.5],
            "orientation": [0.0, 0.0, 0.0, 1.0],
            "visualRef": null,
            "visualType": null,
            "joinedAt": "2026-06-05T14:00:00Z",
            "leftAt": "2026-06-05T15:00:00Z",
            "worldState": null
        })
    }

    #[test]
    fn departed_avatar_round_trips() {
        // Oracle: L578 leftAt "Time this avatar left the region. `null` if
        // currently present."
        let raw = departed();
        let a: SceneAvatar = serde_json::from_value(raw.clone()).unwrap();
        assert!(a.left_at.is_some());
        assert_eq!(a.visual_ref, None);
        assert_eq!(serde_json::to_value(&a).unwrap(), raw);
    }

    #[test]
    fn nullable_fields_serialize_as_null() {
        // Oracle: L568 visualRef, L571 visualType, L577 leftAt and L580
        // worldState are "|null".
        let mut raw = departed();
        raw["leftAt"] = json!(null);
        let a: SceneAvatar = serde_json::from_value(raw).unwrap();
        let out = serde_json::to_value(&a).unwrap();
        for key in ["visualRef", "visualType", "leftAt", "worldState"] {
            assert_eq!(out.get(key), Some(&json!(null)), "{key}");
        }
    }

    #[test]
    fn required_fields_rejected_when_missing() {
        // Oracle: L550-575 type these without "|null".
        for field in [
            "id",
            "regionId",
            "userId",
            "displayName",
            "position",
            "orientation",
            "joinedAt",
        ] {
            let mut raw = departed();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<SceneAvatar>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = departed();
        raw["vendor.example:badge"] = json!("gold");
        let a: SceneAvatar = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&a).unwrap(), raw);
    }
}
