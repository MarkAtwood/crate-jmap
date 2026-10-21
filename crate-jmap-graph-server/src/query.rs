//! `/query` filter evaluation and ordering for Graph, GraphNode and
//! GraphEdge (draft-atwood-jmap-graph-00 §Graph/query, §GraphNode/query,
//! §GraphEdge/query).
//!
//! Pure functions a backend's `query_objects` can apply to its stored
//! objects; the reference `MemoryBackend` uses them. They do not check
//! rights: the backend omits records of Graphs the caller may not read
//! before applying them.

use std::cmp::Ordering;

use jmap_graph_types::query::{
    Filter, GraphComparator, GraphEdgeComparator, GraphEdgeFilterCondition, GraphEdgeSortProperty,
    GraphFilter, GraphNodeComparator, GraphNodeFilterCondition, GraphNodeSortProperty,
    GraphSortProperty, Operator,
};
use jmap_graph_types::{Graph, GraphEdge, GraphNode};
use jmap_types::UTCDate;

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

/// The sortable part of a UTCDate: `YYYY-MM-DDTHH:MM:SS`. Fractional
/// seconds are dropped so that values with and without them compare
/// chronologically.
fn utc_key(d: &UTCDate) -> &str {
    let s: &str = d.as_ref();
    s.get(..19).unwrap_or(s)
}

/// Apply `is_ascending` to `o`.
fn direction(o: Ordering, is_ascending: bool) -> Ordering {
    if is_ascending {
        o
    } else {
        o.reverse()
    }
}

/// `true` if `graph`, as the caller sees it, matches `filter`: "The
/// Graph's `isSubscribed` property equals this value."
pub fn graph_matches(filter: &GraphFilter, graph: &Graph) -> bool {
    filter_matches(filter, &|c| {
        c.is_subscribed.is_none_or(|s| s == graph.is_subscribed)
    })
}

/// Order two Graphs by `sort` ("The following sort properties are
/// supported: `name`, `created`, `updated`"; RFC 8620 §5.5: later
/// comparators break ties of earlier ones), then by `id` so the order is
/// total.
///
/// ponytail: `name` compares by Unicode code point and ignores the
/// comparator's `collation`.
pub fn compare_graphs(a: &Graph, b: &Graph, sort: &[GraphComparator]) -> Ordering {
    sort.iter()
        .map(|c| {
            let o = match c.property {
                GraphSortProperty::Name => a.name.cmp(&b.name),
                GraphSortProperty::Created => utc_key(&a.created).cmp(utc_key(&b.created)),
                GraphSortProperty::Updated => utc_key(&a.updated).cmp(utc_key(&b.updated)),
                _ => Ordering::Equal,
            };
            direction(o, c.is_ascending)
        })
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.id.cmp(&b.id))
}

/// `true` if `node` matches `cond`: "The node is in this Graph."
pub fn node_matches(cond: &GraphNodeFilterCondition, node: &GraphNode) -> bool {
    node.graph_id == cond.graph_id
}

/// Order two nodes by `sort` (`created`, `updated`), then by `id`.
pub fn compare_nodes(a: &GraphNode, b: &GraphNode, sort: &[GraphNodeComparator]) -> Ordering {
    sort.iter()
        .map(|c| {
            let o = match c.property {
                GraphNodeSortProperty::Created => utc_key(&a.created).cmp(utc_key(&b.created)),
                GraphNodeSortProperty::Updated => utc_key(&a.updated).cmp(utc_key(&b.updated)),
                _ => Ordering::Equal,
            };
            direction(o, c.is_ascending)
        })
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.id.cmp(&b.id))
}

/// `true` if `edge` matches `cond`: "All properties given must match."
/// `fromId` matches when "The edge's `fromId` is one of these ids", and `toId`
/// likewise; `type` when "The edge's `type` equals this value exactly."
pub fn edge_matches(cond: &GraphEdgeFilterCondition, edge: &GraphEdge) -> bool {
    edge.graph_id == cond.graph_id
        && cond
            .from_id
            .as_ref()
            .is_none_or(|ids| ids.contains(&edge.from_id))
        && cond
            .to_id
            .as_ref()
            .is_none_or(|ids| ids.contains(&edge.to_id))
        && cond.edge_type.as_ref().is_none_or(|t| *t == edge.edge_type)
}

/// Order two edges by `sort` (`created`, `updated`), then by `id`.
pub fn compare_edges(a: &GraphEdge, b: &GraphEdge, sort: &[GraphEdgeComparator]) -> Ordering {
    sort.iter()
        .map(|c| {
            let o = match c.property {
                GraphEdgeSortProperty::Created => utc_key(&a.created).cmp(utc_key(&b.created)),
                GraphEdgeSortProperty::Updated => utc_key(&a.updated).cmp(utc_key(&b.updated)),
                _ => Ordering::Equal,
            };
            direction(o, c.is_ascending)
        })
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.id.cmp(&b.id))
}
