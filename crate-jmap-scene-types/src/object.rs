//! draft-atwood-jmap-scene-00 §SceneObject.

use jmap_types::{Id, UTCDate, impl_string_enum};
use serde::{Deserialize, Serialize};

use crate::coordinates::identity_scale;
use crate::{Orientation, Position, Scale};

/// How the simulation layer treats a [`SceneObject`]'s physics: its
/// `physicsMode` (draft-atwood-jmap-scene-00 §SceneObject Object Fields).
/// Draft default `"static"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum PhysicsMode {
    /// `"static"`: immovable collider.
    #[default]
    Static,
    /// `"dynamic"`: physics-simulated.
    Dynamic,
    /// `"kinematic"`: moves via script or server, not physics.
    Kinematic,
    /// `"none"`: no collision.
    None,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(PhysicsMode, "a Scene physics mode string",
    "static" => Static,
    "dynamic" => Dynamic,
    "kinematic" => Kinematic,
    "none" => None,
);

/// A positioned entity within a SceneRegion
/// (draft-atwood-jmap-scene-00 §SceneObject).
///
/// `T|null` properties are `Option<T>` and serialize `None` as `null`.
/// `position`, `orientation` and `scale` are relative to the parent's
/// transform when `parentId` is set, otherwise to the region origin.
///
/// Not enforced on deserialize: `visualType` "MUST be present when
/// `visualRef` is non-null. MUST be null when `visualRef` is null."
/// `assetUri` is untrusted; nothing here validates it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneObject {
    /// A ULID assigned at creation.
    pub id: Id,
    /// The SceneRegion this object belongs to.
    pub region_id: Id,
    /// Parent SceneObject in the scene graph; `None` for a root object.
    pub parent_id: Option<Id>,
    /// Human-readable label.
    pub name: Option<String>,
    /// Position `[x, y, z]` in meters.
    pub position: Position,
    /// Orientation as a unit quaternion `[x, y, z, w]`.
    pub orientation: Orientation,
    /// Scale factors. Draft default `[1, 1, 1]`.
    #[serde(default = "identity_scale")]
    pub scale: Scale,
    /// Blob reference for the visual; `None` for invisible objects.
    pub visual_ref: Option<Id>,
    /// Media type of the visual, e.g. `"model/gltf-binary"`.
    pub visual_type: Option<String>,
    /// CDN or static-server URI for the visual asset.
    pub asset_uri: Option<String>,
    /// Physics treatment. Draft default `"static"`.
    #[serde(default)]
    pub physics_mode: PhysicsMode,
    /// `true` if users may interact with the object. Draft default
    /// `false`.
    #[serde(default)]
    pub interactable: bool,
    /// `true` if the object should be rendered. Draft default `true`.
    #[serde(default = "default_true")]
    pub visible: bool,
    /// userId of the creator; `None` for system-placed objects.
    pub owner_id: Option<String>,
    /// Time the object was created.
    pub created_at: UTCDate,
    /// Time the object was last modified.
    pub updated_at: UTCDate,
    /// Deployment-defined extension properties, stored and relayed
    /// without interpretation.
    pub world_state: Option<serde_json::Map<String, serde_json::Value>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IDENTITY_SCALE;
    use serde_json::json;

    /// An invisible trigger, hand-written from the field list of
    /// draft-atwood-jmap-scene-00 L463-512.
    fn trigger() -> serde_json::Value {
        json!({
            "id": "01J5OBJ0000000000000000009",
            "regionId": "01J5ABC0000000000000000001",
            "parentId": "01J5OBJ0000000000000000001",
            "name": null,
            "position": [0.0, 1.0, 0.0],
            "orientation": [0.0, 0.0, 0.0, 1.0],
            "scale": [-1.0, 1.0, 1.0],
            "visualRef": null,
            "visualType": null,
            "assetUri": null,
            "physicsMode": "none",
            "interactable": false,
            "visible": false,
            "ownerId": null,
            "createdAt": "2026-06-01T12:00:00Z",
            "updatedAt": "2026-06-01T12:00:00Z",
            "worldState": null
        })
    }

    #[test]
    fn physics_mode_wire_values_and_default() {
        // Oracle: L494 "Values: `\"static\"` ..., `\"dynamic\"` ...,
        // `\"kinematic\"` ..., `\"none\"` ... Default: `\"static\"`."
        let v: Vec<PhysicsMode> =
            serde_json::from_value(json!(["static", "dynamic", "kinematic", "none"])).unwrap();
        assert_eq!(
            v,
            [
                PhysicsMode::Static,
                PhysicsMode::Dynamic,
                PhysicsMode::Kinematic,
                PhysicsMode::None
            ]
        );
        assert_eq!(PhysicsMode::default(), PhysicsMode::Static);
        let m: PhysicsMode = serde_json::from_value(json!("x-soft-body")).unwrap();
        assert_eq!(serde_json::to_value(&m).unwrap(), json!("x-soft-body"));
    }

    #[test]
    fn missing_defaulted_fields_take_draft_defaults() {
        // Oracle: L482 scale "Default: `[1, 1, 1]`"; L494 physicsMode
        // "Default: `\"static\"`"; L497 interactable "Default: `false`";
        // L500 visible "Default: `true`".
        let mut raw = trigger();
        for key in ["scale", "physicsMode", "interactable", "visible"] {
            raw.as_object_mut().unwrap().remove(key);
        }
        let o: SceneObject = serde_json::from_value(raw).unwrap();
        assert_eq!(o.scale, IDENTITY_SCALE);
        assert_eq!(o.physics_mode, PhysicsMode::Static);
        assert!(!o.interactable);
        assert!(o.visible);
    }

    #[test]
    fn invisible_child_object_round_trips() {
        // Oracle: L470 parentId "When non-null, `position`, `orientation`,
        // and `scale` are relative to the parent's transform"; L485
        // visualRef "`null` for invisible objects (triggers, spawn points,
        // spatial audio sources)".
        let raw = trigger();
        let o: SceneObject = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(o.parent_id, Some(Id::from("01J5OBJ0000000000000000001")));
        assert_eq!(o.visual_ref, None);
        assert_eq!(o.scale, [-1.0, 1.0, 1.0]);
        assert_eq!(serde_json::to_value(&o).unwrap(), raw);
    }

    #[test]
    fn required_fields_rejected_when_missing() {
        // Oracle: L463-509 type these without "|null" and without a
        // default.
        for field in [
            "id",
            "regionId",
            "position",
            "orientation",
            "createdAt",
            "updatedAt",
        ] {
            let mut raw = trigger();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<SceneObject>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = trigger();
        raw["vendor.example:script"] = json!("door.lua");
        let o: SceneObject = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&o).unwrap(), raw);
    }
}
