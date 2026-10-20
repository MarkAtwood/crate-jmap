//! `/query` filter evaluation and ordering for SceneAsset
//! (draft-atwood-jmap-scene-00 §SceneAsset/query).
//!
//! Pure functions a backend's `query_objects` can apply to its stored
//! objects; the reference `MemoryBackend` uses them.

use std::cmp::Ordering;

use jmap_scene_types::query::{
    Filter, Operator, SceneAssetComparator, SceneAssetFilter, SceneAssetSortProperty,
};
use jmap_scene_types::SceneAsset;

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
            && c.name.as_ref().is_none_or(|q| {
                asset
                    .name
                    .as_ref()
                    .is_some_and(|n| n.to_lowercase().contains(&q.to_lowercase()))
            })
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
