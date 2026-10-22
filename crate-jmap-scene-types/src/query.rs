//! `SceneAsset/query`, `SceneRegion/query`, `SceneObject/query` and
//! `SceneAvatar/query` filter and comparator types
//! (draft-atwood-jmap-scene-00 §SceneAsset/query, §SceneRegion/query,
//! §SceneObject/query, §Spatial Filters, §SceneAvatar/query; RFC 8620
//! §5.5).
//!
//! # Excluded from extras preservation
//!
//! Like every filter and comparator type in the workspace, these carry no
//! flatten-extras `extra` field and no `Other(String)` sort property: a
//! server that silently drops a filter clause or sort key it does not
//! understand returns the wrong result set. The conditions and the spatial
//! filter objects carry `#[serde(deny_unknown_fields)]`, so an unknown
//! clause fails the decode and servers map that to `unsupportedFilter`.
//! See the workspace AGENTS.md "Filter algebra and control enums are
//! explicitly EXCLUDED".
//!
//! # Not enforced on deserialize
//!
//! The `invalidArguments` conditions are server checks: an absent
//! `regionId` on `SceneObject/query` and `SceneAvatar/query`, a zero or
//! negative `withinRadius.radius`, a `withinBounds.max` component below
//! `min`, and an `accessPolicy` or `physicsMode` outside the defined set
//! (decoded as the enum's `Other` arm).
//!
//! # Construction from outside this crate
//!
//! ```rust
//! use jmap_scene_types::query::{SceneObjectFilter, SceneObjectFilterCondition, WithinRadius};
//! use jmap_types::Id;
//!
//! let mut cond = SceneObjectFilterCondition::default();
//! cond.region_id = Some(Id::from("01J5ABC0000000000000000001"));
//! cond.within_radius = Some(WithinRadius::new([5.0, 0.0, -2.0], 20.0));
//! let filter = SceneObjectFilter::Condition(cond);
//! ```

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Serialize};

use crate::{AccessPolicy, PhysicsMode, Position, ViewHint};

pub use jmap_types::query::{Filter, FilterOperator, Operator};

/// The `withinRadius` spatial filter: position inside a sphere
/// (draft-atwood-jmap-scene-00 §Spatial Filters).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WithinRadius {
    /// Center point `[x, y, z]` in region-local coordinates.
    pub center: Position,
    /// Radius in meters. "MUST be positive."
    pub radius: f64,
}

impl WithinRadius {
    /// A sphere filter of `radius` meters around `center`.
    pub fn new(center: Position, radius: f64) -> Self {
        Self { center, radius }
    }
}

/// The `withinBounds` spatial filter: position inside an axis-aligned
/// bounding box (draft-atwood-jmap-scene-00 §Spatial Filters).
///
/// The same shape as [`SceneBounds`](crate::SceneBounds) without its
/// `extra`, since a filter must not drop unknown keys.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WithinBounds {
    /// Minimum corner `[x, y, z]`.
    pub min: Position,
    /// Maximum corner `[x, y, z]`; each component "MUST be greater than
    /// or equal to the corresponding component of `min`".
    pub max: Position,
}

impl WithinBounds {
    /// A bounding-box filter from `min` to `max`.
    pub fn new(min: Position, max: Position) -> Self {
        Self { min, max }
    }
}

/// Filter for `SceneAsset/query`.
pub type SceneAssetFilter = Filter<SceneAssetFilterCondition>;

/// Filter condition for `SceneAsset/query`
/// (draft-atwood-jmap-scene-00 §SceneAsset/query).
///
/// "All filter properties are combined with logical AND."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneAssetFilterCondition {
    /// The asset's `mediaType` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// "Substring match on asset name."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// "Assets created at or after this timestamp."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<UTCDate>,
    /// "Assets created before this timestamp."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_before: Option<UTCDate>,
}

/// Sort property for `SceneAsset/query`: "Sort properties: `createdAt`,
/// `name`, `size`. Default sort: `createdAt` descending."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SceneAssetSortProperty {
    /// Sort by `createdAt`.
    CreatedAt,
    /// Sort by `name`.
    Name,
    /// Sort by `size`.
    Size,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(SceneAssetSortProperty, "a sort property string",
    "createdAt" => CreatedAt,
    "name" => Name,
    "size" => Size,
);

/// RFC 8620 §5.5 `Comparator` for `SceneAsset/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneAssetComparator {
    /// The property to sort by.
    pub property: SceneAssetSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl SceneAssetComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: SceneAssetSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

pub(crate) fn default_true() -> bool {
    true
}

/// Filter for `SceneRegion/query`.
pub type SceneRegionFilter = Filter<SceneRegionFilterCondition>;

/// Filter condition for `SceneRegion/query`
/// (draft-atwood-jmap-scene-00 §SceneRegion/query).
///
/// "All filter properties are combined with logical AND."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneRegionFilterCondition {
    /// "Substring match on region name."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// "Exact match on access policy."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_policy: Option<AccessPolicy>,
    /// "Exact match on the `viewHint` field", reverse-domain values
    /// included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_hint: Option<ViewHint>,
    /// `true`: regions whose `simulationUri` is non-null; `false`: null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_simulation_uri: Option<bool>,
    /// `true`: regions with `activeAvatarCount > 0`; `false`: empty
    /// regions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_active_avatars: Option<bool>,
    /// "Regions created at or after this timestamp."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<UTCDate>,
    /// "Regions created before this timestamp."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_before: Option<UTCDate>,
}

/// Sort property for `SceneRegion/query`: "Sort properties: `createdAt`,
/// `name`, `activeAvatarCount`. Default sort: `createdAt` descending."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SceneRegionSortProperty {
    /// Sort by `createdAt`.
    CreatedAt,
    /// Sort by `name`.
    Name,
    /// Sort by `activeAvatarCount`.
    ActiveAvatarCount,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(SceneRegionSortProperty, "a sort property string",
    "createdAt" => CreatedAt,
    "name" => Name,
    "activeAvatarCount" => ActiveAvatarCount,
);

/// RFC 8620 §5.5 `Comparator` for `SceneRegion/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneRegionComparator {
    /// The property to sort by.
    pub property: SceneRegionSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl SceneRegionComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: SceneRegionSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

/// Filter for `SceneObject/query`.
pub type SceneObjectFilter = Filter<SceneObjectFilterCondition>;

/// Filter condition for `SceneObject/query`
/// (draft-atwood-jmap-scene-00 §SceneObject/query, §Spatial Filters).
///
/// "All filter properties are combined with logical AND." `regionId` is
/// required by the draft ("Servers MUST return `invalidArguments` when
/// this property is absent") but is an `Option` here so that the server,
/// not the decoder, makes that call.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneObjectFilterCondition {
    /// "The region to query."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_id: Option<Id>,
    /// "Full-text search over object name."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// "Exact match on the visual media type."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual_type: Option<String>,
    /// "Filter to objects owned by this userId."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    /// The object's `interactable` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interactable: Option<bool>,
    /// The object's `visible` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// The object's `physicsMode` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physics_mode: Option<PhysicsMode>,
    /// `None` when absent; `Some(Some(id))` "returns only objects whose
    /// `parentId` matches"; `Some(None)` (`null`) "returns only root-level
    /// objects".
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_id: Option<Option<Id>>,
    /// Spatial proximity filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within_radius: Option<WithinRadius>,
    /// Spatial bounding-box filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within_bounds: Option<WithinBounds>,
}

/// Sort property for `SceneObject/query`: "Sort properties: `createdAt`,
/// `name`. Default sort: `createdAt` ascending."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SceneObjectSortProperty {
    /// Sort by `createdAt`.
    CreatedAt,
    /// Sort by `name`.
    Name,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(SceneObjectSortProperty, "a sort property string",
    "createdAt" => CreatedAt,
    "name" => Name,
);

/// RFC 8620 §5.5 `Comparator` for `SceneObject/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneObjectComparator {
    /// The property to sort by.
    pub property: SceneObjectSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl SceneObjectComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: SceneObjectSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

/// Filter for `SceneAvatar/query`.
pub type SceneAvatarFilter = Filter<SceneAvatarFilterCondition>;

/// Filter condition for `SceneAvatar/query`
/// (draft-atwood-jmap-scene-00 §SceneAvatar/query).
///
/// "All filter properties are combined with logical AND." `regionId` is
/// required by the draft but is an `Option`, as on
/// [`SceneObjectFilterCondition`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneAvatarFilterCondition {
    /// "The region to query."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_id: Option<Id>,
    /// "Filter to avatars belonging to this user account."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// `true`: avatars with `leftAt == null`; `false`: avatars who have
    /// left.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    /// Spatial proximity filter over the last known position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within_radius: Option<WithinRadius>,
    /// Spatial bounding-box filter over the last known position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within_bounds: Option<WithinBounds>,
}

/// Sort property for `SceneAvatar/query`: "Sort properties: `joinedAt`,
/// `displayName`. Default sort: `joinedAt` ascending."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SceneAvatarSortProperty {
    /// Sort by `joinedAt`.
    JoinedAt,
    /// Sort by `displayName`.
    DisplayName,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(SceneAvatarSortProperty, "a sort property string",
    "joinedAt" => JoinedAt,
    "displayName" => DisplayName,
);

/// RFC 8620 §5.5 `Comparator` for `SceneAvatar/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneAvatarComparator {
    /// The property to sort by.
    pub property: SceneAvatarSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl SceneAvatarComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: SceneAvatarSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn asset_condition_carries_every_spec_filter_property() {
        // Oracle: draft-atwood-jmap-scene-00 L806-816 lists `mediaType`,
        // `name`, `createdAfter`, `createdBefore`.
        let wire = json!({
            "mediaType": "model/gltf-binary",
            "name": "sculpture",
            "createdAfter": "2026-03-01T00:00:00Z",
            "createdBefore": "2026-06-01T00:00:00Z"
        });
        let cond: SceneAssetFilterCondition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(cond.name.as_deref(), Some("sculpture"));
        assert_eq!(serde_json::to_value(&cond).unwrap(), wire);
    }

    #[test]
    fn region_condition_carries_every_spec_filter_property() {
        // Oracle: L1253-1272 lists `name`, `accessPolicy`, `viewHint`,
        // `hasSimulationUri`, `hasActiveAvatars`, `createdAfter`,
        // `createdBefore`.
        let wire = json!({
            "name": "Plaza",
            "accessPolicy": "space",
            "viewHint": "com.example.isometric",
            "hasSimulationUri": false,
            "hasActiveAvatars": true,
            "createdAfter": "2026-01-01T00:00:00Z",
            "createdBefore": "2026-02-01T00:00:00Z"
        });
        let cond: SceneRegionFilterCondition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(cond.access_policy, Some(AccessPolicy::Space));
        assert_eq!(
            cond.view_hint,
            Some(ViewHint::Other("com.example.isometric".to_owned()))
        );
        assert_eq!(serde_json::to_value(&cond).unwrap(), wire);
    }

    #[test]
    fn object_condition_carries_every_spec_filter_property() {
        // Oracle: L1758-1794 lists `regionId`, `name`, `visualType`,
        // `ownerId`, `interactable`, `visible`, `physicsMode`, `parentId`,
        // `withinRadius` (`center`, `radius`) and `withinBounds` (`min`,
        // `max`).
        let wire = json!({
            "regionId": "01J5ABC0000000000000000001",
            "name": "door",
            "visualType": "model/gltf-binary",
            "ownerId": "user:admin@example.com",
            "interactable": true,
            "visible": false,
            "physicsMode": "kinematic",
            "parentId": "01J5OBJ0000000000000000001",
            "withinRadius": {"center": [5.5, 0.0, -2.5], "radius": 20.5},
            "withinBounds": {"min": [-15.0, 0.0, -15.0], "max": [15.0, 10.0, 15.0]}
        });
        let cond: SceneObjectFilterCondition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(cond.physics_mode, Some(PhysicsMode::Kinematic));
        assert_eq!(
            cond.within_radius,
            Some(WithinRadius::new([5.5, 0.0, -2.5], 20.5))
        );
        assert_eq!(
            cond.within_bounds,
            Some(WithinBounds::new([-15.0, 0.0, -15.0], [15.0, 10.0, 15.0]))
        );
        assert_eq!(serde_json::to_value(&cond).unwrap(), wire);
    }

    #[test]
    fn object_parent_id_null_distinct_from_absent() {
        // Oracle: L1779-1780 parentId "(Id|null, optional) ... When set to
        // `null`, returns only root-level objects".
        let root: SceneObjectFilterCondition =
            serde_json::from_value(json!({"regionId": "R1", "parentId": null})).unwrap();
        assert_eq!(root.parent_id, Some(None));
        assert_eq!(
            serde_json::to_value(&root).unwrap(),
            json!({"regionId": "R1", "parentId": null})
        );
        let any: SceneObjectFilterCondition =
            serde_json::from_value(json!({"regionId": "R1"})).unwrap();
        assert_eq!(any.parent_id, None);
        assert_eq!(
            serde_json::to_value(&any).unwrap(),
            json!({"regionId": "R1"})
        );
    }

    #[test]
    fn spatial_filters_require_their_properties() {
        // Oracle: L1786-1794 mark `center`, `radius`, `min` and `max`
        // "required".
        for bad in [
            json!({"center": [0.0, 0.0, 0.0]}),
            json!({"radius": 1.0}),
            json!({"center": [0.0, 0.0], "radius": 1.0}),
        ] {
            assert!(
                serde_json::from_value::<WithinRadius>(bad.clone()).is_err(),
                "{bad}"
            );
        }
        for bad in [
            json!({"min": [0.0, 0.0, 0.0]}),
            json!({"max": [0.0, 0.0, 0.0]}),
        ] {
            assert!(
                serde_json::from_value::<WithinBounds>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn spatial_filters_reject_unknown_properties() {
        // Oracle: RFC 8620 §5.5 "unsupportedFilter"; workspace policy maps
        // an unknown clause to a decode failure.
        assert!(serde_json::from_value::<WithinRadius>(
            json!({"center": [0.0, 0.0, 0.0], "radius": 1.0, "falloff": 2})
        )
        .is_err());
        assert!(serde_json::from_value::<WithinBounds>(
            json!({"min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 1.0], "y": 1})
        )
        .is_err());
    }

    #[test]
    fn avatar_condition_carries_every_spec_filter_property() {
        // Oracle: L2234-2251 lists `regionId`, `userId`, `isActive`,
        // `withinRadius` and `withinBounds`.
        let wire = json!({
            "regionId": "01J5ABC0000000000000000001",
            "userId": "user:alice@example.com",
            "isActive": true,
            "withinRadius": {"center": [10.5, 0.0, -5.5], "radius": 30.5},
            "withinBounds": {"min": [-1.0, -1.0, -1.0], "max": [1.0, 1.0, 1.0]}
        });
        let cond: SceneAvatarFilterCondition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(cond.is_active, Some(true));
        assert_eq!(serde_json::to_value(&cond).unwrap(), wire);
    }

    #[test]
    fn unknown_filter_property_is_rejected() {
        // Oracle: L822, L1278 "If a client provides a filter property the
        // server does not support, the server MUST return an
        // `unsupportedFilter` error."
        assert!(serde_json::from_value::<SceneAssetFilter>(json!({"size": 1})).is_err());
        assert!(serde_json::from_value::<SceneRegionFilter>(json!({"bogus": 1})).is_err());
        assert!(serde_json::from_value::<SceneObjectFilter>(json!({
            "operator": "AND",
            "conditions": [{"regionId": "R1"}, {"bogus": 1}]
        }))
        .is_err());
        assert!(serde_json::from_value::<SceneAvatarFilter>(
            json!({"regionId": "R1", "displayName": "x"})
        )
        .is_err());
    }

    #[test]
    fn comparators_accept_only_spec_sort_properties() {
        // Oracle: L824 "Sort properties: `createdAt`, `name`, `size`.";
        // L1280 "Sort properties: `createdAt`, `name`,
        // `activeAvatarCount`."; L1802 "Sort properties: `createdAt`,
        // `name`."; L2259 "Sort properties: `joinedAt`, `displayName`."
        for (wire, want) in [
            ("createdAt", SceneAssetSortProperty::CreatedAt),
            ("name", SceneAssetSortProperty::Name),
            ("size", SceneAssetSortProperty::Size),
        ] {
            let c: SceneAssetComparator =
                serde_json::from_value(json!({ "property": wire })).unwrap();
            assert_eq!(c.property, want);
            // RFC 8620 §5.5 isAscending "default: true".
            assert!(c.is_ascending);
        }
        for (wire, want) in [
            ("createdAt", SceneRegionSortProperty::CreatedAt),
            ("name", SceneRegionSortProperty::Name),
            (
                "activeAvatarCount",
                SceneRegionSortProperty::ActiveAvatarCount,
            ),
        ] {
            let c: SceneRegionComparator =
                serde_json::from_value(json!({ "property": wire })).unwrap();
            assert_eq!(c.property, want);
        }
        for (wire, want) in [
            ("createdAt", SceneObjectSortProperty::CreatedAt),
            ("name", SceneObjectSortProperty::Name),
        ] {
            let c: SceneObjectComparator =
                serde_json::from_value(json!({ "property": wire })).unwrap();
            assert_eq!(c.property, want);
        }
        for (wire, want) in [
            ("joinedAt", SceneAvatarSortProperty::JoinedAt),
            ("displayName", SceneAvatarSortProperty::DisplayName),
        ] {
            let c: SceneAvatarComparator =
                serde_json::from_value(json!({ "property": wire })).unwrap();
            assert_eq!(c.property, want);
        }
        assert_eq!(
            serde_json::from_value::<SceneAssetComparator>(json!({"property": "mediaType"}))
                .unwrap()
                .property,
            SceneAssetSortProperty::Other("mediaType".to_owned())
        );
        assert_eq!(
            serde_json::from_value::<SceneRegionComparator>(json!({"property": "updatedAt"}))
                .unwrap()
                .property,
            SceneRegionSortProperty::Other("updatedAt".to_owned())
        );
        assert_eq!(
            serde_json::from_value::<SceneObjectComparator>(
                json!({"property": "activeAvatarCount"})
            )
            .unwrap()
            .property,
            SceneObjectSortProperty::Other("activeAvatarCount".to_owned())
        );
        assert_eq!(
            serde_json::from_value::<SceneAvatarComparator>(json!({"property": "leftAt"}))
                .unwrap()
                .property,
            SceneAvatarSortProperty::Other("leftAt".to_owned())
        );
    }
}
