//! [`jmap_types::JmapObject`] impls for the Scene data types.
//!
//! Defined here so that `jmap-scene-server` can use them without violating
//! the orphan rule. The type names are those of draft-atwood-jmap-scene-00
//! §Methods.

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

use crate::query::{
    SceneAssetComparator, SceneAssetFilter, SceneAvatarComparator, SceneAvatarFilter,
    SceneObjectComparator, SceneObjectFilter, SceneRegionComparator, SceneRegionFilter,
};
use crate::{SceneAsset, SceneAvatar, SceneObject, SceneRegion};

impl JmapObject for SceneAsset {
    const TYPE_NAME: &'static str = "SceneAsset";
    type Property = ();
}

impl GetObject for SceneAsset {}

/// draft-atwood-jmap-scene-00 §SceneAsset (lines 269-296): `id`, `ownerId`
/// and `createdAt` are "immutable, server-set"; `accountId` and `size` are
/// "server-set"; `blobId` and `mediaType` are "immutable".
impl SetObject for SceneAsset {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] =
        &["id", "accountId", "ownerId", "size", "createdAt"];
    const CLIENT_SETTABLE: &'static [&'static str] =
        &["blobId", "assetUri", "mediaType", "name", "sha256"];
    const IMMUTABLE: &'static [&'static str] = &["blobId", "mediaType"];
}

impl QueryObject for SceneAsset {
    type Filter = SceneAssetFilter;
    type Comparator = SceneAssetComparator;
}

impl JmapObject for SceneRegion {
    const TYPE_NAME: &'static str = "SceneRegion";
    type Property = ();
}

impl GetObject for SceneRegion {}

/// draft-atwood-jmap-scene-00 §SceneRegion Object Fields (lines 321-481):
/// `id`, `ownerId` and `createdAt` are "immutable, server-set";
/// `accountId`, `activeAvatarCount` and `updatedAt` are "server-set".
impl SetObject for SceneRegion {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "accountId",
        "ownerId",
        "activeAvatarCount",
        "createdAt",
        "updatedAt",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "name",
        "description",
        "bounds",
        "environment",
        "simulationUri",
        "inputUri",
        "spawnPosition",
        "spawnOrientation",
        "viewHint",
        "geoAnchor",
        "accessPolicy",
        "invitedUserIds",
        "worldState",
        "chatId",
        "spaceId",
        "channelId",
        "activeCallId",
    ];
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl QueryObject for SceneRegion {
    type Filter = SceneRegionFilter;
    type Comparator = SceneRegionComparator;
}

impl JmapObject for SceneObject {
    const TYPE_NAME: &'static str = "SceneObject";
    type Property = ();
}

impl GetObject for SceneObject {}

/// draft-atwood-jmap-scene-00 §SceneObject Object Fields (lines 529-577):
/// `id` and `createdAt` are "immutable, server-set"; `ownerId` and
/// `updatedAt` are "server-set"; `regionId` is "immutable".
impl SetObject for SceneObject {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "ownerId", "createdAt", "updatedAt"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "regionId",
        "parentId",
        "name",
        "position",
        "orientation",
        "scale",
        "visualRef",
        "visualType",
        "assetUri",
        "physicsMode",
        "interactable",
        "visible",
        "worldState",
    ];
    const IMMUTABLE: &'static [&'static str] = &["regionId"];
}

impl QueryObject for SceneObject {
    type Filter = SceneObjectFilter;
    type Comparator = SceneObjectComparator;
}

impl JmapObject for SceneAvatar {
    const TYPE_NAME: &'static str = "SceneAvatar";
    type Property = ();
}

impl GetObject for SceneAvatar {}

/// draft-atwood-jmap-scene-00 §SceneAvatar Object Fields (lines 616-646):
/// `id` and `userId` are "immutable, server-set"; `joinedAt` is
/// "server-set"; `regionId` is "immutable". §SceneAvatar/set also has the
/// server set `position`, `orientation` and `leftAt` on create.
impl SetObject for SceneAvatar {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "userId", "joinedAt"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "regionId",
        "displayName",
        "position",
        "orientation",
        "visualRef",
        "visualType",
        "leftAt",
        "worldState",
    ];
    const IMMUTABLE: &'static [&'static str] = &["regionId"];
}

impl QueryObject for SceneAvatar {
    type Filter = SceneAvatarFilter;
    type Comparator = SceneAvatarComparator;
}

#[cfg(test)]
mod tests {
    //! Each test builds a fully populated object with a struct literal, so a
    //! new field is a compile error here until it is set, then a
    //! classification failure until the `SetObject` lists name it.

    use super::*;
    use jmap_types::{check_property_classification, Id, UTCDate};
    use serde_json::{json, Map, Value};

    fn de<T: serde::de::DeserializeOwned>(v: Value) -> T {
        serde_json::from_value(v).expect("hand-written fixture deserializes")
    }

    fn date() -> UTCDate {
        UTCDate::from("2026-01-01T00:00:00Z")
    }

    #[test]
    fn scene_asset_properties_are_classified() {
        let obj = SceneAsset {
            id: Id::from("a1"),
            account_id: Id::from("acc"),
            owner_id: "u1".into(),
            blob_id: Id::from("b1"),
            asset_uri: Some("https://example.com/a.glb".into()),
            media_type: "model/gltf-binary".into(),
            name: Some("n".into()),
            size: 1,
            sha256: Some("00".into()),
            created_at: date(),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn scene_region_properties_are_classified() {
        let obj = SceneRegion {
            id: Id::from("r1"),
            account_id: Id::from("acc"),
            owner_id: "u1".into(),
            name: "n".into(),
            description: Some("d".into()),
            bounds: de(json!({"min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 1.0]})),
            environment: Some(Map::new()),
            simulation_uri: Some("https://example.com/sim".into()),
            input_uri: Some(Some("https://example.com/in".into())),
            spawn_position: [0.0; 3],
            spawn_orientation: [0.0, 0.0, 0.0, 1.0],
            active_avatar_count: 1,
            view_hint: Some(de(json!("immersive"))),
            geo_anchor: Some(Some(de(
                json!({"latitude": 0.0, "longitude": 0.0, "heading": 0.0}),
            ))),
            access_policy: de(json!("open")),
            invited_user_ids: vec!["u2".into()],
            created_at: date(),
            updated_at: date(),
            world_state: Some(Some(Map::new())),
            chat_id: Some(Some(Id::from("c1"))),
            space_id: Some(Some(Id::from("s1"))),
            channel_id: Some(Some(Id::from("ch1"))),
            active_call_id: Some(Some(Id::from("v1"))),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn scene_object_properties_are_classified() {
        let obj = SceneObject {
            id: Id::from("o1"),
            region_id: Id::from("r1"),
            parent_id: Some(Id::from("o0")),
            name: Some("n".into()),
            position: [0.0; 3],
            orientation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            visual_ref: Some(Id::from("a1")),
            visual_type: Some("model/gltf-binary".into()),
            asset_uri: Some("https://example.com/a.glb".into()),
            physics_mode: de(json!("static")),
            interactable: true,
            visible: true,
            owner_id: Some("u1".into()),
            created_at: date(),
            updated_at: date(),
            world_state: Some(Map::new()),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn scene_avatar_properties_are_classified() {
        let obj = SceneAvatar {
            id: Id::from("v1"),
            region_id: Id::from("r1"),
            user_id: "u1".into(),
            display_name: "U".into(),
            position: [0.0; 3],
            orientation: [0.0, 0.0, 0.0, 1.0],
            visual_ref: Some(Id::from("a1")),
            visual_type: Some("model/gltf-binary".into()),
            joined_at: date(),
            left_at: Some(date()),
            world_state: Some(Map::new()),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }
}
