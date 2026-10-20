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

impl SetObject for SceneAsset {
    type Patch = PatchObject;
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

impl SetObject for SceneRegion {
    type Patch = PatchObject;
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

impl SetObject for SceneObject {
    type Patch = PatchObject;
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

impl SetObject for SceneAvatar {
    type Patch = PatchObject;
}

impl QueryObject for SceneAvatar {
    type Filter = SceneAvatarFilter;
    type Comparator = SceneAvatarComparator;
}
