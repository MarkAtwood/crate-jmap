//! Comparator `isAscending` wire form matches `EmailComparator`.
//!
//! Oracle: RFC 8620 §5.5 — "isAscending: `Boolean` (optional; default:
//! true)". An ascending comparator omits the default; a descending one
//! carries `"isAscending": false`; an absent key decodes as ascending.
//! Expected JSON is hand-written.

use jmap_scene_types::{
    SceneAssetComparator, SceneAssetSortProperty, SceneAvatarComparator, SceneAvatarSortProperty,
    SceneObjectComparator, SceneObjectSortProperty, SceneRegionComparator, SceneRegionSortProperty,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn assert_default_omitted<C>(ascending: C, descending: C, property: &str)
where
    C: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let bare = json!({ "property": property });
    assert_eq!(serde_json::to_value(&ascending).unwrap(), bare);
    assert_eq!(serde_json::from_value::<C>(bare).unwrap(), ascending);
    let desc: Value = json!({ "property": property, "isAscending": false });
    assert_eq!(serde_json::to_value(&descending).unwrap(), desc);
    assert_eq!(serde_json::from_value::<C>(desc).unwrap(), descending);
}

#[test]
fn scene_asset_comparator_omits_default_is_ascending() {
    let up = SceneAssetComparator::new(SceneAssetSortProperty::CreatedAt);
    let mut down = up.clone();
    down.is_ascending = false;
    assert_default_omitted(up, down, "createdAt");
}

#[test]
fn scene_region_comparator_omits_default_is_ascending() {
    let up = SceneRegionComparator::new(SceneRegionSortProperty::CreatedAt);
    let mut down = up.clone();
    down.is_ascending = false;
    assert_default_omitted(up, down, "createdAt");
}

#[test]
fn scene_object_comparator_omits_default_is_ascending() {
    let up = SceneObjectComparator::new(SceneObjectSortProperty::CreatedAt);
    let mut down = up.clone();
    down.is_ascending = false;
    assert_default_omitted(up, down, "createdAt");
}

#[test]
fn scene_avatar_comparator_omits_default_is_ascending() {
    let up = SceneAvatarComparator::new(SceneAvatarSortProperty::JoinedAt);
    let mut down = up.clone();
    down.is_ascending = false;
    assert_default_omitted(up, down, "joinedAt");
}
