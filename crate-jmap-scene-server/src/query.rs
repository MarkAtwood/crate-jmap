//! `/query` filter evaluation and ordering for SceneAsset, SceneRegion and
//! SceneObject (draft-atwood-jmap-scene-00 §SceneAsset/query,
//! §SceneRegion/query, §SceneObject/query).
//!
//! Pure functions a backend's `query_objects` can apply to its stored
//! objects; the reference `MemoryBackend` uses them.

use std::cmp::Ordering;

use jmap_scene_types::query::{
    Filter, Operator, SceneAssetComparator, SceneAssetFilter, SceneAssetSortProperty,
    SceneObjectComparator, SceneObjectFilter, SceneObjectSortProperty, SceneRegionComparator,
    SceneRegionFilter, SceneRegionSortProperty,
};
use jmap_scene_types::{Position, SceneAsset, SceneObject, SceneRegion};
use jmap_types::Id;

use crate::helpers::utc_key;

/// RFC 8620 §5.5 filter algebra over `condition`. A filter operator this
/// version does not know matches nothing.
fn filter_matches<C>(filter: &Filter<C>, condition: &impl Fn(&C) -> bool) -> bool {
    match filter {
        Filter::Condition(cond) => condition(cond),
        Filter::Operator(op) => {
            let mut each = op.conditions.iter().map(|f| filter_matches(f, condition));
            match op.operator {
                Operator::And => each.all(|m| m),
                Operator::Or => each.any(|m| m),
                Operator::Not => !each.any(|m| m),
                _ => false,
            }
        }
        _ => false,
    }
}

/// Case-insensitive substring match of `query` in `name`.
fn name_contains(name: &str, query: &str) -> bool {
    name.to_lowercase().contains(&query.to_lowercase())
}

/// `true` if `asset` matches `filter` (§SceneAsset/query): "All filter
/// properties are combined with logical AND."
///
/// `mediaType` is an exact match, ignoring ASCII case (media type names
/// are case-insensitive, RFC 6838 §4.2). `name` is a "Substring match on
/// asset name", ignoring case as the draft's example does: the filter
/// `"sculpture"` selects the asset named `"Sculpture: Convergence"`. An
/// asset with a `null` name matches no `name` filter. `createdAfter` is
/// inclusive ("at or after"), `createdBefore` exclusive ("before").
pub fn asset_matches(filter: &SceneAssetFilter, asset: &SceneAsset) -> bool {
    let created = utc_key(&asset.created_at);
    filter_matches(filter, &|c| {
        c.media_type
            .as_ref()
            .is_none_or(|t| t.eq_ignore_ascii_case(&asset.media_type))
            && c.name
                .as_ref()
                .is_none_or(|q| asset.name.as_ref().is_some_and(|n| name_contains(n, q)))
            && c.created_after
                .as_ref()
                .is_none_or(|b| created >= utc_key(b))
            && c.created_before
                .as_ref()
                .is_none_or(|b| created < utc_key(b))
    })
}

/// Order two assets by `sort` ("Sort properties: `createdAt`, `name`,
/// `size`"; RFC 8620 §5.5: later comparators break ties of earlier ones),
/// then by `id` so the order is total.
///
/// ponytail: `name` compares by Unicode code point and ignores the
/// comparator's `collation`; the draft does not say where a `null` name
/// sorts, so assets without one go last in either direction.
pub fn compare_assets(a: &SceneAsset, b: &SceneAsset, sort: &[SceneAssetComparator]) -> Ordering {
    sort.iter()
        .map(|c| {
            let directed = |ord: Ordering| if c.is_ascending { ord } else { ord.reverse() };
            match c.property {
                SceneAssetSortProperty::CreatedAt => {
                    directed(utc_key(&a.created_at).cmp(utc_key(&b.created_at)))
                }
                SceneAssetSortProperty::Size => directed(a.size.cmp(&b.size)),
                SceneAssetSortProperty::Name => match (&a.name, &b.name) {
                    (Some(x), Some(y)) => directed(x.cmp(y)),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                },
                _ => Ordering::Equal,
            }
        })
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.id.as_ref().cmp(b.id.as_ref()))
}

/// `true` if `region` matches `filter` (§SceneRegion/query): "All filter
/// properties are combined with logical AND."
///
/// `name` is a "Substring match on region name", ignoring case as
/// [`asset_matches`] does. `accessPolicy` and `viewHint` are exact
/// matches; a region whose `viewHint` is `null` matches no `viewHint`
/// filter. `hasSimulationUri` tests `simulationUri` for non-null,
/// `hasActiveAvatars` tests "`activeAvatarCount > 0`". `createdAfter` is
/// inclusive ("at or after"), `createdBefore` exclusive ("before").
pub fn region_matches(filter: &SceneRegionFilter, region: &SceneRegion) -> bool {
    let created = utc_key(&region.created_at);
    filter_matches(filter, &|c| {
        c.name
            .as_ref()
            .is_none_or(|q| name_contains(&region.name, q))
            && c.access_policy
                .as_ref()
                .is_none_or(|p| *p == region.access_policy)
            && c.view_hint
                .as_ref()
                .is_none_or(|h| region.view_hint.as_ref() == Some(h))
            && c.has_simulation_uri
                .is_none_or(|want| region.simulation_uri.is_some() == want)
            && c.has_active_avatars
                .is_none_or(|want| (region.active_avatar_count > 0) == want)
            && c.created_after
                .as_ref()
                .is_none_or(|b| created >= utc_key(b))
            && c.created_before
                .as_ref()
                .is_none_or(|b| created < utc_key(b))
    })
}

/// Order two regions by `sort` ("Sort properties: `createdAt`, `name`,
/// `activeAvatarCount`"; RFC 8620 §5.5: later comparators break ties of
/// earlier ones), then by `id` so the order is total.
///
/// ponytail: `name` compares by Unicode code point and ignores the
/// comparator's `collation`, as [`compare_assets`] does.
pub fn compare_regions(
    a: &SceneRegion,
    b: &SceneRegion,
    sort: &[SceneRegionComparator],
) -> Ordering {
    sort.iter()
        .map(|c| {
            let ord = match c.property {
                SceneRegionSortProperty::CreatedAt => {
                    utc_key(&a.created_at).cmp(utc_key(&b.created_at))
                }
                SceneRegionSortProperty::Name => a.name.cmp(&b.name),
                SceneRegionSortProperty::ActiveAvatarCount => {
                    a.active_avatar_count.cmp(&b.active_avatar_count)
                }
                _ => Ordering::Equal,
            };
            if c.is_ascending {
                ord
            } else {
                ord.reverse()
            }
        })
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.id.as_ref().cmp(b.id.as_ref()))
}

/// The position of `object` in region-local coordinates.
///
/// "When non-null, `position`, `orientation`, and `scale` are relative to
/// the parent's transform", while the spatial filters take region-local
/// points. Each ancestor maps the point into its own parent's frame as
/// `t + R(q) (s * p)` (glTF 2.0 TRS order: scale, then rotate, then
/// translate). `parent_of` looks an ancestor up by id; the walk stops at a
/// root, at an ancestor `parent_of` does not know, and at an id it has
/// already visited, so a corrupt cycle cannot loop.
pub fn region_local_position<'a>(
    object: &'a SceneObject,
    parent_of: impl Fn(&Id) -> Option<&'a SceneObject>,
) -> Position {
    let mut p = object.position;
    let mut visited: Vec<&Id> = vec![&object.id];
    let mut next = object.parent_id.as_ref();
    while let Some(parent) = next.filter(|id| !visited.contains(id)).and_then(&parent_of) {
        let scaled = [
            p[0] * parent.scale[0],
            p[1] * parent.scale[1],
            p[2] * parent.scale[2],
        ];
        let r = rotate(parent.orientation, scaled);
        p = [
            parent.position[0] + r[0],
            parent.position[1] + r[1],
            parent.position[2] + r[2],
        ];
        visited.push(&parent.id);
        next = parent.parent_id.as_ref();
    }
    p
}

/// Rotate `v` by the unit quaternion `[x, y, z, w]`:
/// `v + 2w (u x v) + 2 u x (u x v)` with `u = [x, y, z]`.
fn rotate(q: [f64; 4], v: Position) -> Position {
    let cross = |a: Position, b: Position| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let u = [q[0], q[1], q[2]];
    let uv = cross(u, v);
    let uuv = cross(u, uv);
    [
        v[0] + 2.0 * (q[3] * uv[0] + uuv[0]),
        v[1] + 2.0 * (q[3] * uv[1] + uuv[1]),
        v[2] + 2.0 * (q[3] * uv[2] + uuv[2]),
    ]
}

/// `true` if `object`, whose region-local position is `position` (see
/// [`region_local_position`]), matches `filter` (§SceneObject/query):
/// "All filter properties are combined with logical AND."
///
/// `name` is a case-insensitive substring match, as [`asset_matches`]
/// does; an object with a `null` name matches no `name` filter.
/// `visualType` is an exact match ignoring ASCII case (RFC 6838 §4.2).
/// `parentId` `null` "returns only root-level objects". `withinRadius`
/// and `withinBounds` include their boundary.
pub fn object_matches(
    filter: &SceneObjectFilter,
    object: &SceneObject,
    position: Position,
) -> bool {
    filter_matches(filter, &|c| {
        c.region_id.as_ref().is_none_or(|r| *r == object.region_id)
            && c.name
                .as_ref()
                .is_none_or(|q| object.name.as_ref().is_some_and(|n| name_contains(n, q)))
            && c.visual_type.as_ref().is_none_or(|t| {
                object
                    .visual_type
                    .as_ref()
                    .is_some_and(|v| v.eq_ignore_ascii_case(t))
            })
            && c.owner_id
                .as_ref()
                .is_none_or(|o| object.owner_id.as_ref() == Some(o))
            && c.interactable
                .is_none_or(|want| object.interactable == want)
            && c.visible.is_none_or(|want| object.visible == want)
            && c.physics_mode
                .as_ref()
                .is_none_or(|m| *m == object.physics_mode)
            && c.parent_id
                .as_ref()
                .is_none_or(|want| object.parent_id.as_ref() == want.as_ref())
            && c.within_radius.as_ref().is_none_or(|w| {
                let d2: f64 = (0..3).map(|i| (position[i] - w.center[i]).powi(2)).sum();
                d2 <= w.radius * w.radius
            })
            && c.within_bounds
                .as_ref()
                .is_none_or(|b| (0..3).all(|i| b.min[i] <= position[i] && position[i] <= b.max[i]))
    })
}

/// Order two objects by `sort` ("Sort properties: `createdAt`, `name`";
/// RFC 8620 §5.5: later comparators break ties of earlier ones), then by
/// `id` so the order is total.
///
/// ponytail: `name` compares by Unicode code point and ignores the
/// comparator's `collation`; objects without a name go last in either
/// direction, as [`compare_assets`] does.
pub fn compare_objects(
    a: &SceneObject,
    b: &SceneObject,
    sort: &[SceneObjectComparator],
) -> Ordering {
    sort.iter()
        .map(|c| {
            let directed = |ord: Ordering| if c.is_ascending { ord } else { ord.reverse() };
            match c.property {
                SceneObjectSortProperty::CreatedAt => {
                    directed(utc_key(&a.created_at).cmp(utc_key(&b.created_at)))
                }
                SceneObjectSortProperty::Name => match (&a.name, &b.name) {
                    (Some(x), Some(y)) => directed(x.cmp(y)),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                },
                _ => Ordering::Equal,
            }
        })
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.id.as_ref().cmp(b.id.as_ref()))
}
