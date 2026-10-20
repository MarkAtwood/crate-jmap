//! `VTCCall/query` filter evaluation and ordering
//! (draft-atwood-jmap-vtc-00 §VTCCall/query).
//!
//! Pure functions a backend's `query_objects` can apply to its stored
//! calls; the reference `MemoryBackend` uses them.

use std::cmp::Ordering;

use jmap_types::UTCDate;
use jmap_vtc_types::query::{
    Filter, Operator, VtcCallComparator, VtcCallFilter, VtcCallFilterCondition, VtcCallSortProperty,
};
use jmap_vtc_types::VtcCall;

use crate::helpers::utc_key;

/// `true` if `call` matches `filter` (RFC 8620 §5.5 filter algebra).
///
/// `was_participant(user_id)` answers the `hasParticipant` condition:
/// "Filter to calls in which this userId is or was a participant."
///
/// A filter operator this version does not know matches nothing.
pub fn call_matches(
    filter: &VtcCallFilter,
    call: &VtcCall,
    was_participant: &impl Fn(&str) -> bool,
) -> bool {
    match filter {
        Filter::Condition(cond) => condition_matches(cond, call, was_participant),
        Filter::Operator(op) => {
            let mut each = op
                .conditions
                .iter()
                .map(|f| call_matches(f, call, was_participant));
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

/// "All filter properties are combined with logical AND."
fn condition_matches(
    cond: &VtcCallFilterCondition,
    call: &VtcCall,
    was_participant: &impl Fn(&str) -> bool,
) -> bool {
    let bound = |field: &Option<Option<jmap_types::Id>>, want: &jmap_types::Id| {
        field.as_ref().and_then(Option::as_ref) == Some(want)
    };
    let at_or_after =
        |t: Option<&UTCDate>, bound: &UTCDate| t.is_some_and(|t| utc_key(t) >= utc_key(bound));
    let before =
        |t: Option<&UTCDate>, bound: &UTCDate| t.is_some_and(|t| utc_key(t) < utc_key(bound));
    cond.state.as_ref().is_none_or(|s| *s == call.state)
        && cond.call_type.as_ref().is_none_or(|t| *t == call.call_type)
        && cond
            .initiator_id
            .as_ref()
            .is_none_or(|u| *u == call.initiator_id)
        && cond
            .chat_id
            .as_ref()
            .is_none_or(|c| bound(&call.chat_id, c))
        && cond
            .space_id
            .as_ref()
            .is_none_or(|s| bound(&call.space_id, s))
        && cond.has_participant.as_deref().is_none_or(was_participant)
        && cond
            .created_after
            .as_ref()
            .is_none_or(|b| at_or_after(Some(&call.created_at), b))
        && cond
            .created_before
            .as_ref()
            .is_none_or(|b| before(Some(&call.created_at), b))
        && cond
            .ended_after
            .as_ref()
            .is_none_or(|b| at_or_after(call.ended_at.as_ref(), b))
        && cond
            .ended_before
            .as_ref()
            .is_none_or(|b| before(call.ended_at.as_ref(), b))
}

/// Order two calls by `sort` (RFC 8620 §5.5: later comparators break ties
/// of earlier ones), then by `id` so the order is total.
///
/// ponytail: the draft does not say where a `null` `endedAt` sorts; this
/// puts calls that have not ended after ended ones in either direction.
pub fn compare_calls(a: &VtcCall, b: &VtcCall, sort: &[VtcCallComparator]) -> Ordering {
    sort.iter()
        .map(|c| {
            let ord = match c.property {
                VtcCallSortProperty::CreatedAt => {
                    utc_key(&a.created_at).cmp(utc_key(&b.created_at))
                }
                VtcCallSortProperty::EndedAt => {
                    return match (&a.ended_at, &b.ended_at) {
                        (Some(x), Some(y)) => directed(utc_key(x).cmp(utc_key(y)), c),
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => Ordering::Equal,
                    }
                }
                _ => Ordering::Equal,
            };
            directed(ord, c)
        })
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.id.as_ref().cmp(b.id.as_ref()))
}

fn directed(ord: Ordering, c: &VtcCallComparator) -> Ordering {
    if c.is_ascending {
        ord
    } else {
        ord.reverse()
    }
}
