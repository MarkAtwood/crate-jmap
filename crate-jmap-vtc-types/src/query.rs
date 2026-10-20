//! `VTCCall/query` filter and comparator types
//! (draft-atwood-jmap-vtc-00 §VTCCall/query; RFC 8620 §5.5).
//!
//! # Excluded from extras preservation
//!
//! Like every filter and comparator type in the workspace, these carry no
//! flatten-extras `extra` field and no `Other(String)` sort property: a
//! server that silently drops a filter clause or sort key it does not
//! understand returns the wrong result set. The condition carries
//! `#[serde(deny_unknown_fields)]`, so an unknown clause fails the decode
//! and servers map that to `unsupportedFilter`. See the workspace
//! AGENTS.md "Filter algebra and control enums are explicitly EXCLUDED".
//!
//! # Construction from outside this crate
//!
//! ```rust
//! use jmap_vtc_types::query::{VtcCallFilter, VtcCallFilterCondition};
//! use jmap_vtc_types::CallState;
//!
//! let mut cond = VtcCallFilterCondition::default();
//! cond.state = Some(CallState::Active);
//! let filter = VtcCallFilter::Condition(cond);
//! ```

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

use crate::{CallState, CallType};

pub use jmap_types::query::{Filter, FilterOperator, Operator};

/// Filter for `VTCCall/query`.
pub type VtcCallFilter = Filter<VtcCallFilterCondition>;

/// Filter condition for `VTCCall/query`
/// (draft-atwood-jmap-vtc-00 §VTCCall/query).
///
/// "All filter properties are combined with logical AND." `chatId` and
/// `spaceId` are "Only valid when `urn:ietf:params:jmap:chat` is present;
/// servers MUST return `unsupportedFilter` otherwise."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VtcCallFilterCondition {
    /// The call's `state` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<CallState>,
    /// The call's `callType` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_type: Option<CallType>,
    /// The call's `initiatorId` must equal this userId.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initiator_id: Option<String>,
    /// The call's `chatId` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<Id>,
    /// The call's `spaceId` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Id>,
    /// This userId "is or was a participant" in the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_participant: Option<String>,
    /// The call was "created at or after this time".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<UTCDate>,
    /// The call was "created before this time".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_before: Option<UTCDate>,
    /// The call "ended at or after this time".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_after: Option<UTCDate>,
    /// The call "ended before this time".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_before: Option<UTCDate>,
}

/// Sort property for `VTCCall/query`: "Sort properties: `createdAt`,
/// `endedAt`. Default sort: `createdAt` descending."
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VtcCallSortProperty {
    /// Sort by `createdAt`.
    CreatedAt,
    /// Sort by `endedAt`.
    EndedAt,
}

/// RFC 8620 §5.5 `Comparator` for `VTCCall/query`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VtcCallComparator {
    /// The property to sort by.
    pub property: VtcCallSortProperty,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl VtcCallComparator {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: VtcCallSortProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn condition_carries_every_spec_filter_property() {
        // Oracle: draft-atwood-jmap-vtc-00 L725-L755 lists `state`,
        // `callType`, `initiatorId`, `chatId`, `spaceId`, `hasParticipant`,
        // `createdAfter`, `createdBefore`, `endedAfter`, `endedBefore`.
        let wire = json!({
            "state": "active",
            "callType": "room",
            "initiatorId": "user:bob@example.com",
            "chatId": "C1",
            "spaceId": "S1",
            "hasParticipant": "user:alice@example.com",
            "createdAfter": "2026-06-01T00:00:00Z",
            "createdBefore": "2026-07-01T00:00:00Z",
            "endedAfter": "2026-06-02T00:00:00Z",
            "endedBefore": "2026-07-02T00:00:00Z"
        });
        let cond: VtcCallFilterCondition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(cond.state, Some(CallState::Active));
        assert_eq!(cond.call_type, Some(CallType::Room));
        assert_eq!(serde_json::to_value(&cond).unwrap(), wire);
    }

    #[test]
    fn unknown_filter_property_is_rejected() {
        // Oracle: RFC 8620 §5.5 "unsupportedFilter"; workspace policy maps
        // an unknown clause to a decode failure.
        assert!(serde_json::from_value::<VtcCallFilter>(json!({"bogus": 1})).is_err());
        assert!(serde_json::from_value::<VtcCallFilter>(json!({
            "operator": "AND",
            "conditions": [{"state": "ended"}, {"bogus": 1}]
        }))
        .is_err());
    }

    #[test]
    fn comparator_accepts_only_spec_sort_properties() {
        // Oracle: L759 "Sort properties: `createdAt`, `endedAt`."
        let c: VtcCallComparator = serde_json::from_value(json!({"property": "endedAt"})).unwrap();
        assert_eq!(c.property, VtcCallSortProperty::EndedAt);
        // RFC 8620 §5.5 isAscending "default: true".
        assert!(c.is_ascending);
        assert!(
            serde_json::from_value::<VtcCallComparator>(json!({"property": "subject"})).is_err()
        );
    }
}
