//! [`jmap_types::JmapObject`] impls for the Scene data types.
//!
//! Defined here so that `jmap-scene-server` can use them without violating
//! the orphan rule. The type names are those of draft-atwood-jmap-scene-00
//! §Methods.

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

use crate::query::{SceneAssetComparator, SceneAssetFilter};
use crate::SceneAsset;

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
